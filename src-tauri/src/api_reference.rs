//! Private, exact-instance handoff for the external CLI. The existing database
//! stores only a random instance ID; the credential is in an owner-only file.
//! Failed publication and stale instances never fall back to another port.

use rand::RngCore;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const INSTANCE_KEY: &str = "internal.api_instance";
const MAX_REFERENCE_BYTES: usize = 1024;
static PUBLISHER: OnceLock<Arc<Publisher>> = OnceLock::new();

// Intentionally no Debug: parsing and error reporting must not print a token.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiReference {
    instance: String,
    port: u16,
    token: String,
}

impl ApiReference {
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn token(&self) -> &str {
        &self.token
    }
}

fn unavailable() -> io::Error {
    io::Error::other("Private app reference unavailable; launch ColimaUI and retry")
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn record_path(data: &Path, instance: &str) -> io::Result<PathBuf> {
    if !is_hex(instance, 32) {
        return Err(unavailable());
    }
    Ok(data.join("api").join(format!("{instance}.json")))
}

fn parse_reference(bytes: &[u8], instance: &str) -> io::Result<ApiReference> {
    let reference: ApiReference = serde_json::from_slice(bytes).map_err(|_| unavailable())?;
    if reference.instance != instance
        || !is_hex(instance, 32)
        || !(11420..=11429).contains(&reference.port)
        || !reference
            .token
            .strip_prefix("colima-")
            .is_some_and(|s| is_hex(s, 64))
    {
        return Err(unavailable());
    }
    Ok(reference)
}

/// Reads metadata without creating a database or printing parse/private bytes.
pub fn read_latest() -> io::Result<ApiReference> {
    read_latest_in(&crate::path_util::try_app_data_dir()?)
}

fn read_latest_in(data: &Path) -> io::Result<ApiReference> {
    let conn =
        Connection::open_with_flags(data.join("knowledge.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| unavailable())?;
    conn.busy_timeout(std::time::Duration::from_millis(250))
        .map_err(|_| unavailable())?;
    let instance = crate::app_settings::read(&conn, INSTANCE_KEY)
        .map_err(|_| unavailable())?
        .ok_or_else(unavailable)?;
    let path = record_path(data, &instance)?;
    let bytes = crate::private_file::read(&path, MAX_REFERENCE_BYTES).map_err(|_| unavailable())?;
    parse_reference(&bytes, &instance)
}

struct Publisher {
    data: PathBuf,
    instance: String,
    closed: Mutex<bool>,
}

impl Publisher {
    fn prepare(conn: &Connection, data: PathBuf) -> io::Result<Self> {
        let mut random = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut random);
        let instance: String = random.iter().map(|b| format!("{b:02x}")).collect();
        // Supersede the old marker before binding or publishing. If this app's
        // private-file creation fails, CLI access refuses instead of selecting
        // an older live app. Late older publication cannot change this marker.
        crate::app_settings::write(conn, INSTANCE_KEY, &instance).map_err(|_| unavailable())?;
        Ok(Self {
            data,
            instance,
            closed: Mutex::new(false),
        })
    }

    fn publish(&self, port: u16, token: &str) -> io::Result<()> {
        let closed = self.closed.lock().map_err(|_| unavailable())?;
        if *closed {
            return Err(unavailable());
        }
        let reference = ApiReference {
            instance: self.instance.clone(),
            port,
            token: token.to_string(),
        };
        let bytes = serde_json::to_vec(&reference).map_err(|_| unavailable())?;
        parse_reference(&bytes, &self.instance)?;
        let path = record_path(&self.data, &self.instance)?;
        std::fs::create_dir_all(path.parent().ok_or_else(unavailable)?)?;
        crate::private_file::write_new(&path, &bytes).map_err(|_| unavailable())
    }

    fn close(&self) {
        if let Ok(mut closed) = self.closed.lock() {
            *closed = true;
            if let Ok(path) = record_path(&self.data, &self.instance) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

pub fn prepare() -> io::Result<()> {
    let conn = crate::commands::knowledge_bank::get_db()
        .lock()
        .map_err(|_| unavailable())?;
    let publisher = Arc::new(Publisher::prepare(
        &conn,
        crate::path_util::try_app_data_dir()?,
    )?);
    PUBLISHER.set(publisher).map_err(|_| unavailable())
}

pub fn publish(port: u16, token: &str) -> io::Result<()> {
    PUBLISHER
        .get()
        .ok_or_else(unavailable)?
        .publish(port, token)
}

pub fn close() {
    if let Some(publisher) = PUBLISHER.get() {
        publisher.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture {
        data: PathBuf,
        conn: Option<Connection>,
    }
    impl Fixture {
        fn new() -> Self {
            let data = std::env::temp_dir().join(format!(
                "colimaui-reference-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&data).unwrap();
            let conn = Connection::open(data.join("knowledge.db")).unwrap();
            conn.execute_batch("CREATE TABLE app_settings(setting_key TEXT PRIMARY KEY, setting_value TEXT NOT NULL, updated_at TEXT)").unwrap();
            Self {
                data,
                conn: Some(conn),
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            drop(self.conn.take());
            let _ = std::fs::remove_dir_all(&self.data);
        }
    }
    fn token() -> String {
        format!("colima-{}", "a".repeat(64))
    }

    // TC-WINUI-001/014: match the current instance, never first public health.
    #[test]
    fn newer_instance_wins_and_old_cleanup_cannot_remove_it() {
        let f = Fixture::new();
        let old = Publisher::prepare(f.conn.as_ref().unwrap(), f.data.clone()).unwrap();
        old.publish(11420, &token()).unwrap();
        let new = Publisher::prepare(f.conn.as_ref().unwrap(), f.data.clone()).unwrap();
        assert!(read_latest_in(&f.data).is_err());
        new.publish(11429, &token()).unwrap();
        assert_eq!(read_latest_in(&f.data).unwrap().port(), 11429);
        old.close();
        assert_eq!(read_latest_in(&f.data).unwrap().port(), 11429);
        new.close();
        assert!(read_latest_in(&f.data).is_err());
        assert!(new.publish(11429, &token()).is_err());
    }

    #[test]
    fn failed_publication_and_late_old_publication_never_reuse_old_access() {
        let f = Fixture::new();
        let old = Publisher::prepare(f.conn.as_ref().unwrap(), f.data.clone()).unwrap();
        let new = Publisher::prepare(f.conn.as_ref().unwrap(), f.data.clone()).unwrap();
        old.publish(11420, &token()).unwrap();
        assert!(read_latest_in(&f.data).is_err());
        assert!(new.publish(80, &token()).is_err());
        assert!(read_latest_in(&f.data).is_err());
        assert!(record_path(&f.data, "../elsewhere").is_err());
        let error = parse_reference(b"private malformed bytes", &"a".repeat(32))
            .err()
            .unwrap();
        assert!(!error.to_string().contains("private malformed bytes"));
    }

    #[test]
    fn wrong_instance_port_token_and_extra_fields_are_rejected() {
        let id = "a".repeat(32);
        for port in [11419, 11430] {
            let bytes = serde_json::to_vec(&ApiReference {
                instance: id.clone(),
                port,
                token: token(),
            })
            .unwrap();
            assert!(parse_reference(&bytes, &id).is_err());
        }
        let bytes = serde_json::to_vec(&ApiReference {
            instance: id.clone(),
            port: 11420,
            token: token(),
        })
        .unwrap();
        assert!(parse_reference(&bytes, &"b".repeat(32)).is_err());
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["token"] = "invalid".into();
        assert!(parse_reference(&serde_json::to_vec(&value).unwrap(), &id).is_err());
        value["token"] = token().into();
        value["extra"] = "ignored?".into();
        assert!(parse_reference(&serde_json::to_vec(&value).unwrap(), &id).is_err());
    }

    // TestSpec=TC-WINUI-014; TestSpec=TC-WINUI-038 — owner auth boundary, not native outsider ACL proof.
    // TC-WINUI-014: real private-file read → production auth/router boundary.
    // Reading metadata or public health alone never grants resource access.
    #[tokio::test]
    async fn private_reference_authorizes_owner_while_public_health_does_not() {
        use axum::body::{to_bytes, Body};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let f = Fixture::new();
        let publisher = Publisher::prepare(f.conn.as_ref().unwrap(), f.data.clone()).unwrap();
        let actual = crate::auth::get_api_token();
        publisher.publish(11420, &actual).unwrap();
        let reference = read_latest_in(&f.data).unwrap();
        let router = crate::api_server::build_router();
        let health = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        let body = to_bytes(health.into_body(), 4096).await.unwrap();
        assert!(!String::from_utf8_lossy(&body).contains(&actual));

        // Normal Bearer observer; unlike events it neither needs a query token
        // nor starts a long-lived subscription/sampling task.
        let endpoint = "/api/metrics/status";
        let denied = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(endpoint)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        let owner = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(endpoint)
                    .header("authorization", format!("Bearer {}", reference.token()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(owner.status(), StatusCode::OK);
        drop(owner);
        let wrong = router
            .oneshot(
                Request::builder()
                    .uri(endpoint)
                    .header("authorization", "Bearer colima-invalid")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
        publisher.close();
        assert!(read_latest_in(&f.data).is_err());
    }
}
