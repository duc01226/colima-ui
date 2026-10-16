//! Installed release-app measurement. The observer and fixture CLI stay outside
//! the measured Job. UI/engine transitions require an interactive native host.
use crate::windows_test_fixture as fixture;
#[path = "../support/metrics_budget.rs"]
mod budget;

use crate::{
    api_reference,
    child_process::ChildProcess,
    commands::metrics_collector::{CollectorStatus, TickObservation},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("ENVIRONMENT-BLOCKED: {name} is required"))
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Point {
    elapsed_ms: u64,
    cpu_seconds: f64,
    resident_bytes: u64,
    live_processes: u32,
    total_processes: u32,
}

fn point(child: &mut ChildProcess, start: Instant) -> Result<Point, String> {
    if child
        .try_wait()
        .map_err(|_| "Cannot observe installed app lifetime")?
        .is_some()
    {
        return Err("Installed app exited/restarted during the same-process run".into());
    }
    #[cfg(windows)]
    {
        let usage = child
            .resource_usage()
            .map_err(|e| format!("Missing required process sample: {e}"))?;
        if usage.live_processes == 0 || usage.resident_bytes == 0 {
            return Err("Missing required live app process sample".into());
        }
        Ok(Point {
            elapsed_ms: start.elapsed().as_millis() as u64,
            cpu_seconds: usage.cpu_seconds,
            resident_bytes: usage.resident_bytes,
            live_processes: usage.live_processes,
            total_processes: usage.total_processes,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = start;
        Err("ENVIRONMENT-BLOCKED: Windows Job accounting required".into())
    }
}

struct Recorder {
    start: Instant,
    child: Arc<Mutex<ChildProcess>>,
    points: Arc<Mutex<Vec<Point>>>,
    error: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    evidence: PathBuf,
}
impl Recorder {
    fn start(child: ChildProcess, evidence: PathBuf) -> Self {
        let child = Arc::new(Mutex::new(child));
        let points = Arc::new(Mutex::new(Vec::<Point>::new()));
        let error = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let start = Instant::now();
        let (c, p, e, s) = (child.clone(), points.clone(), error.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                let mut next = start;
                while !s.load(Ordering::Acquire) {
                    if Instant::now().saturating_duration_since(next) > Duration::from_millis(500) {
                        return Err("Required one-second process sample was missed".into());
                    }
                    let mut child = c.lock().map_err(|_| "Process owner unavailable")?;
                    let sample = point(&mut child, start)?;
                    drop(child);
                    let mut points = p.lock().map_err(|_| "Measurement storage unavailable")?;
                    if points.len() >= 1800 {
                        return Err("Measurement exceeded bounded 30-minute recording".into());
                    }
                    points.push(sample);
                    drop(points);
                    next += Duration::from_secs(1);
                    std::thread::sleep(next.saturating_duration_since(Instant::now()));
                }
                Ok(())
            })();
            if let Err(failure) = result {
                *e.lock().unwrap() = Some(failure);
            }
        });
        Self {
            start,
            child,
            points,
            error,
            stop,
            thread: Some(thread),
            evidence,
        }
    }
    fn check(&self) {
        let error = self.error.lock().unwrap().clone();
        assert!(error.is_none(), "{}", error.as_deref().unwrap_or(""));
    }
    fn mark(&self) -> u64 {
        self.check();
        self.start.elapsed().as_millis() as u64
    }
    fn current(&self) -> Point {
        self.check();
        point(&mut self.child.lock().unwrap(), self.start).expect("complete process measurement")
    }
    fn pid(&self) -> u32 {
        self.child.lock().unwrap().id()
    }
}
impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let (Ok(points), Ok(error)) = (self.points.lock(), self.error.lock()) {
            let raw = serde_json::json!({"points":*points,"measurementError":*error,"acceptance":"Not an acceptance receipt; raw process samples retained even on failure"});
            if std::fs::write(
                &self.evidence,
                serde_json::to_vec_pretty(&raw).unwrap_or_default(),
            )
            .is_err()
            {
                eprintln!("Failed to retain raw process evidence");
            }
        }
        // Last ChildProcess owner closes the Job; no engine belongs to it.
    }
}

#[derive(Deserialize)]
struct Envelope {
    success: bool,
    data: Option<CollectorStatus>,
}
struct Observer {
    client: reqwest::Client,
    reference: api_reference::ApiReference,
    pid: u32,
    version: String,
    context: String,
    engine: String,
}
impl Observer {
    async fn status(&self) -> CollectorStatus {
        let mut response = self
            .client
            .get(format!(
                "http://127.0.0.1:{}/api/metrics/status",
                self.reference.port()
            ))
            .bearer_auth(self.reference.token())
            .send()
            .await
            .expect("private status request failed");
        assert!(
            response.status().is_success(),
            "private status request refused"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.expect("status body unavailable") {
            assert!(
                bytes.len() + chunk.len() <= 256 * 1024,
                "status body exceeded bound"
            );
            bytes.extend_from_slice(&chunk);
        }
        let envelope: Envelope =
            serde_json::from_slice(&bytes).expect("invalid collector observation");
        assert!(envelope.success);
        let status = envelope.data.expect("missing collector observation");
        assert_eq!(
            status.process_id, self.pid,
            "private reference selected another application"
        );
        assert_eq!(
            status.version, self.version,
            "installed app version differs"
        );
        assert_eq!(
            status.interval_ms, 2000,
            "qualify the two-second sampling contract"
        );
        if let Some(engine) = &status.engine {
            assert_eq!(engine.context.as_deref(), Some(self.context.as_str()));
            assert_eq!(
                engine.engine_id.as_deref(),
                Some(self.engine.as_str()),
                "engine identity changed"
            );
        }
        status
    }
    async fn wait(
        &self,
        recorder: &Recorder,
        reason: &str,
        matches: impl Fn(&CollectorStatus) -> bool,
    ) -> CollectorStatus {
        println!("{reason}");
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            recorder.check();
            let status = self.status().await;
            if matches(&status) {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "required native transition not observed: {reason}"
            );
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    async fn window(&self, recorder: &Recorder, count: usize) -> serde_json::Value {
        let initial = self.status().await;
        let generation = initial
            .engine
            .as_ref()
            .and_then(|engine| engine.generation)
            .expect("ready original generation");
        let first = initial.recent_ticks.last().map_or(0, |tick| tick.sequence);
        let started = Instant::now();
        let cpu_before = recorder.current().cpu_seconds;
        let mut ticks = BTreeMap::<u64, TickObservation>::new();
        loop {
            recorder.check();
            let status = self.status().await;
            assert!(
                status.subscribers > 0,
                "monitoring closed during measurement"
            );
            let engine = status.engine.as_ref().expect("selected engine observation");
            assert!(
                engine.ready && engine.generation == Some(generation),
                "engine lost during required steady window"
            );
            for tick in status
                .recent_ticks
                .into_iter()
                .filter(|tick| tick.sequence > first)
            {
                ticks.insert(tick.sequence, tick);
            }
            if started.elapsed() >= Duration::from_secs(60) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let cpu_seconds = recorder.current().cpu_seconds - cpu_before;
        let wall_seconds = started.elapsed().as_secs_f64();
        let ticks: Vec<_> = ticks.into_values().collect();
        let (cpu, p95) = budget::window(count, cpu_seconds, wall_seconds, &ticks)
            .expect("TC021 CPU/tick budget failed");
        serde_json::json!({"cpuSeconds":cpu_seconds,"wallSeconds":wall_seconds,"cpuPercent":cpu,"p95TickMs":p95,"ticks":ticks})
    }
}

// This is a real ignored native case on every host. Non-Windows execution
// refuses its prerequisite rather than returning a successful zero-test run.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "interactive installed Windows release app, dedicated engine, 20/200 workloads and original-engine recovery"]
async fn installed_monitoring_cost() {
    let mut fixture = fixture::Fixture::acquire().expect("dedicated Windows fixture required");
    let count: usize = required("COLIMAUI_TEST_METRICS_COUNT").parse().unwrap();
    assert!([20, 200].contains(&count));
    let path = PathBuf::from(required("COLIMAUI_TEST_APP_PATH"))
        .canonicalize()
        .expect("installed app path");
    let expected_hash = required("COLIMAUI_TEST_APP_SHA256");
    assert_eq!(
        digest(&std::fs::read(&path).unwrap()),
        expected_hash,
        "installed app differs from selected release artifact"
    );
    assert!(api_reference::read_latest().is_err(), "close other app instances and use a clean disposable profile before starting the installed run");
    let mut command = Command::new(&path);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = ChildProcess::spawn(command).expect("installed app Job ownership");
    let recorder = Recorder::start(
        child,
        fixture
            .work
            .parent()
            .unwrap()
            .join("metrics-process-samples.json"),
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    let observer = loop {
        recorder.check();
        if let Ok(reference) = api_reference::read_latest() {
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap();
            // PID read-back below rejects stale/current-other references.
            break Observer {
                client,
                reference,
                pid: recorder.pid(),
                version: required("COLIMAUI_TEST_APP_VERSION"),
                context: fixture.context.clone(),
                engine: fixture.engine_id.clone(),
            };
        }
        assert!(
            Instant::now() < deadline,
            "installed app private reference unavailable"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let ready = observer
        .wait(
            &recorder,
            "Keep monitoring closed; waiting for original engine readiness.",
            |s| {
                s.subscribers == 0
                    && !s.collecting
                    && s.engine.as_ref().is_some_and(|engine| engine.ready)
            },
        )
        .await;
    let generation = ready.engine.unwrap().generation.unwrap();
    let baseline_start = recorder.mark();
    let ticks_before = ready.started_ticks;
    let until = Instant::now() + Duration::from_secs(60);
    while Instant::now() < until {
        recorder.check();
        let status = observer.status().await;
        assert_eq!(status.subscribers, 0);
        assert!(!status.collecting);
        assert_eq!(status.started_ticks, ticks_before);
        assert!(status
            .engine
            .is_some_and(|engine| engine.ready && engine.generation == Some(generation)));
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let fleet_start = recorder.mark();
    for index in 0..count {
        recorder.check();
        let name = fixture.name(&format!("metrics-{index}"));
        fixture.claim("container", &name);
        fixture
            .text(&[
                "run",
                "--pull=never",
                "-d",
                "--name",
                &name,
                "--label",
                &format!("io.colimaui.test-run={}", fixture.run),
                "--restart",
                "unless-stopped",
                &fixture.image,
                "sleep",
                "3600",
            ])
            .expect("create owned fleet");
    }
    assert_eq!(
        fixture.text(&["ps", "-q"]).unwrap().lines().count(),
        count,
        "full dedicated fleet must be ready"
    );
    observer
        .wait(
            &recorder,
            "Open Activity monitoring in the installed app now.",
            |s| {
                s.subscribers > 0
                    && s.recent_ticks
                        .last()
                        .is_some_and(|tick| !tick.failed && tick.samples == count)
            },
        )
        .await;
    let normal = observer.window(&recorder, count).await;
    observer
        .wait(
            &recorder,
            "Stop the external test engine now; keep monitoring open. Waiting for observed loss.",
            |s| s.engine.as_ref().is_some_and(|engine| !engine.ready),
        )
        .await;
    observer.wait(&recorder, "Restart the ORIGINAL external test engine now. Waiting for its identity and full fleet to recover.", |s| s.engine.as_ref().is_some_and(|engine| engine.ready && engine.generation.is_some_and(|g| g > generation)) && s.recent_ticks.last().is_some_and(|tick| !tick.failed && tick.samples == count)).await;
    let recovered = observer.window(&recorder, count).await;
    let peak_end = recorder.mark();
    let closed = observer
        .wait(
            &recorder,
            "Close Activity monitoring now. Waiting for the sampler to settle.",
            |s| s.subscribers == 0 && !s.collecting,
        )
        .await;
    let idle_until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < idle_until {
        recorder.check();
        let status = observer.status().await;
        assert_eq!(status.subscribers, 0);
        assert!(!status.collecting);
        assert_eq!(
            status.started_ticks, closed.started_ticks,
            "closed monitoring did sampling work"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    recorder.check();
    let points = recorder.points.lock().unwrap().clone();
    let baseline: Vec<_> = points
        .iter()
        .filter(|p| p.elapsed_ms >= baseline_start && p.elapsed_ms < fleet_start)
        .map(|p| p.resident_bytes)
        .collect();
    let loaded: Vec<_> = points
        .iter()
        .filter(|p| p.elapsed_ms >= fleet_start && p.elapsed_ms <= peak_end)
        .map(|p| p.resident_bytes)
        .collect();
    let (baseline, peak, growth) = budget::resident_growth(count, &baseline, &loaded)
        .expect("same-process pre-fleet resident budget failed");
    assert_eq!(
        digest(&std::fs::read(&path).unwrap()),
        expected_hash,
        "installed artifact changed during measurement"
    );
    fixture.cleanup().expect("owned fleet cleanup");
    let receipt = serde_json::json!({"result":"PASS","runId":fixture.run,"providerDeclared":fixture.provider,"context":fixture.context,
        "engineFingerprint":digest(fixture.engine_id.as_bytes()),"appSha256":expected_hash,"appVersion":observer.version,"processId":observer.pid,
        "workloads":count,"intervalMs":2000,"normal":normal,"recovered":recovered,"baselineResidentBytes":baseline,"peakResidentBytes":peak,"residentGrowthBytes":growth,
        "baselineStartMs":baseline_start,"firstFleetCreationMs":fleet_start,"peakEndMs":peak_end,"points":points,
        "scope":"owned installed app Job; cumulative CPU including exited descendants; summed current live WorkingSetSize; engine/fixture/observer excluded",
        "idleBeforeAndAfter":true,"ownedCleanup":true,"fullQualification":"PENDING"});
    std::fs::write(
        fixture.work.parent().unwrap().join("metrics.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
