//! Existing application setting rows. Consumers own key meanings; this owner
//! keeps SQL/typed read failures consistent for desktop and read-only clients.

use rusqlite::{params, Connection, OptionalExtension};

pub fn read(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT setting_value FROM app_settings WHERE setting_key = ?1",
        [key],
        |row| row.get(0),
    )
    .optional()
}

pub fn write(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO app_settings (setting_key, setting_value, updated_at) VALUES (?1, ?2, datetime('now')) ON CONFLICT(setting_key) DO UPDATE SET setting_value = excluded.setting_value, updated_at = datetime('now')", params![key, value])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // TC-WINUI-001/005/014: unreadable selection is an error, never absence or
    // permission to select another engine/reference. Upsert retains other rows.
    #[test]
    fn missing_unreadable_and_saved_values_remain_distinct() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(read(&conn, "choice").is_err());
        conn.execute_batch("CREATE TABLE app_settings(setting_key TEXT PRIMARY KEY, setting_value, updated_at TEXT)").unwrap();
        assert_eq!(read(&conn, "choice").unwrap(), None);
        write(&conn, "unrelated", "retained").unwrap();
        write(&conn, "choice", "first").unwrap();
        write(&conn, "choice", "next").unwrap();
        assert_eq!(read(&conn, "choice").unwrap().as_deref(), Some("next"));
        assert_eq!(
            read(&conn, "unrelated").unwrap().as_deref(),
            Some("retained")
        );
        conn.execute(
            "UPDATE app_settings SET setting_value = x'00ff' WHERE setting_key = 'choice'",
            [],
        )
        .unwrap();
        assert!(read(&conn, "choice").is_err());
    }
}
