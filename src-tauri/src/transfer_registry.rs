//! What transfers exist, independently of the window that started them.
//!
//! `streaming_cmd::RUNNING` already tracks in-flight *processes*, but it is built
//! for killing them: entries appear only once the child is spawned, disappear the
//! moment it is reaped, and carry nothing a user could read. That is the wrong
//! shape for answering "what is going on" after a reload.
//!
//! Three concrete failures this exists to fix:
//!
//!  - **A job could finish before anyone was listening.** `spawn_job` starts the
//!    command before returning the job id, so a spawn failure emitted
//!    `transfer.failed` microseconds later — for an id the caller did not have yet.
//!    Registration happens before the spawn now, and terminal states are kept for
//!    [`RETENTION`] so a client that missed the event can still read the outcome.
//!
//!  - **A reload lost the job.** Nothing could enumerate transfers, so a webview
//!    that reloaded left a `docker save` writing to disk with no way to see or
//!    cancel it. [`list`] is that enumeration.
//!
//!  - **`overwrite: false` was decided too early.** The destination is checked when
//!    a job starts and written when it ends; two jobs aimed at one path both passed
//!    that check. This is the only place that knows about the other job, so the
//!    conflict is refused here.
//!
//! Deliberately **not** persisted: `kill_all_streams` runs on `ExitRequested`, so no
//! transfer outlives the process and a file claiming otherwise would only ever be
//! wrong.

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, TryLockError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a finished transfer stays readable.
///
/// Long enough for a webview to reload and reconcile, short enough that the list
/// stays about *now*. After this a job is simply absent, which callers read as
/// "finished and already reported".
const RETENTION: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferStatus {
    /// Registered, but the child process has not been spawned yet.
    Starting,
    Running,
    Success,
    Failed,
    Cancelled,
}

impl TransferStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Success | Self::Failed | Self::Cancelled)
    }
}

/// What a client can learn about one transfer.
///
/// `target_label` is a name — an image reference, a container path — never a host
/// path. Host paths are what the user chose and are of no use to a viewer who
/// already knows where they saved it; keeping them out means this can be logged and
/// sent over SSE without a redaction pass.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSnapshot {
    pub job_id: String,
    /// `save` | `load` | `cp-in` | `cp-out` | `sbom`, matching the job id prefix.
    pub kind: String,
    pub status: TransferStatus,
    pub bytes: u64,
    pub total_estimate: Option<u64>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub target_label: String,
    /// Redacted failure text, when the job failed.
    pub error: Option<String>,
}

struct Entry {
    publication: Arc<Mutex<()>>,
    snapshot: TransferSnapshot,
    /// Where this job will write, for the conflict check. Never serialised.
    destination: Option<PathBuf>,
    /// Set by [`request_cancel`] before the child exists, so a cancel that arrives
    /// during startup is not silently dropped.
    cancel_requested: bool,
}

static CLOSING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

static REGISTRY: LazyLock<Mutex<HashMap<String, Entry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn lock_registry() -> std::sync::MutexGuard<'static, HashMap<String, Entry>> {
    // A panic while holding this lock cannot leave the map inconsistent — every
    // mutation is a single insert or field assignment — so poisoning carries no
    // information worth acting on. Discarding the map instead would be actively
    // harmful: `list` would report no transfers and a reconciling client would
    // conclude everything had finished.
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Drop terminal entries older than [`RETENTION`].
///
/// Called from the mutating paths rather than on a timer: the map only grows when
/// something touches it, so that is exactly when it needs trimming.
fn prune(map: &mut HashMap<String, Entry>) {
    let cutoff = now_ms().saturating_sub(RETENTION.as_millis() as u64);
    map.retain(|_, e| match e.snapshot.finished_at {
        Some(at) => at > cutoff,
        None => true,
    });
}

/// Record a transfer that is about to start.
///
/// Fails when another *unfinished* job already writes to `destination`. That check
/// belongs here and nowhere else: `resolve_destination` sees only the filesystem,
/// where the first job's output does not exist yet.
pub fn register(
    job_id: &str,
    kind: &str,
    target_label: &str,
    total_estimate: Option<u64>,
    destination: Option<PathBuf>,
) -> Result<(), String> {
    let mut map = lock_registry();
    if CLOSING.load(std::sync::atomic::Ordering::Acquire) {
        return Err("Application is closing".into());
    }
    prune(&mut map);
    if map.contains_key(job_id) {
        return Err("Transfer ID is already registered".into());
    }

    if let Some(dest) = &destination {
        if let Some(other) = map.values().find(|e| {
            !e.snapshot.status.is_terminal()
                && e.destination
                    .as_deref()
                    .is_some_and(|path| crate::validation::same_host_path(path, dest))
        }) {
            return Err(format!(
                "Another transfer ({}) is already writing to {}. Wait for it or cancel it first.",
                other.snapshot.job_id,
                dest.display()
            ));
        }
    }

    map.insert(
        job_id.to_string(),
        Entry {
            publication: Arc::new(Mutex::new(())),
            snapshot: TransferSnapshot {
                job_id: job_id.to_string(),
                kind: kind.to_string(),
                status: TransferStatus::Starting,
                bytes: 0,
                total_estimate,
                started_at: now_ms(),
                finished_at: None,
                target_label: target_label.to_string(),
                error: None,
            },
            destination,
            cancel_requested: false,
        },
    );
    Ok(())
}

/// Admit shutdown under the registration lock. A refused restart changes no
/// state; an admitted shutdown cannot acquire a late transfer and cancel it
/// without the operator's explicit permission.
pub fn begin_shutdown(cancel_active: bool) -> Result<(), String> {
    let map = lock_registry();
    if !cancel_active && map.values().any(|e| !e.snapshot.status.is_terminal()) {
        return Err("Finish or explicitly cancel active transfers before restarting".into());
    }
    CLOSING.store(true, std::sync::atomic::Ordering::Release);
    Ok(())
}

/// Mark the child as spawned. Returns false when the job was cancelled first.
pub fn set_running(job_id: &str) -> bool {
    let mut map = lock_registry();
    match map.get_mut(job_id) {
        Some(entry) if entry.cancel_requested || entry.snapshot.status.is_terminal() => false,
        Some(entry) => {
            entry.snapshot.status = TransferStatus::Running;
            true
        }
        None => false,
    }
}

pub fn update_bytes(job_id: &str, bytes: u64) {
    if let Some(entry) = lock_registry().get_mut(job_id) {
        entry.snapshot.bytes = bytes;
    }
}

/// Record the outcome.
///
/// Called after the destination has been published, not when the child is reaped —
/// a client that sees `Success` here can rely on the file being there.
pub fn settle(job_id: &str, status: TransferStatus, bytes: u64, error: Option<String>) {
    complete(job_id, status, bytes, error);
}

/// Choose a terminal result under the same lock that accepts cancellation.
/// A completed output is already Success; non-file completion honors an earlier
/// accepted cancel. The caller emits this actual outcome on its transports.
pub fn complete(
    job_id: &str,
    mut status: TransferStatus,
    mut bytes: u64,
    mut error: Option<String>,
) -> TransferStatus {
    // An accepted producer may resume after its completed snapshot expired.
    // Its observed outcome remains truthful; absence never starts new work.
    let Some(publication) = publication_gate(job_id) else {
        return status;
    };
    let _publication = publication.lock().unwrap_or_else(|e| e.into_inner());
    let logged;
    {
        let mut map = lock_registry();
        if let Some(entry) = map.get_mut(job_id) {
            if entry.snapshot.status.is_terminal() || !status.is_terminal() {
                return entry.snapshot.status;
            }
            if status == TransferStatus::Success && entry.cancel_requested {
                status = TransferStatus::Cancelled;
                bytes = 0;
                error = None;
            }
            logged = Some(finish_entry(entry, status, bytes, error.clone()));
        } else {
            return status;
        }
        prune(&mut map);
    }
    record_outcome(job_id, status, bytes, error, logged);
    status
}

/// Publication and Success form one admission boundary with request_cancel.
/// The publisher must perform its fallible preparation BEFORE changing output.
pub(crate) fn commit_output(
    job_id: &str,
    publish: impl FnOnce() -> Result<u64, String>,
) -> Result<Option<u64>, String> {
    let publication = publication_gate(job_id).ok_or("Transfer no longer exists")?;
    let _publication = publication.lock().unwrap_or_else(|e| e.into_inner());
    {
        let map = lock_registry();
        let entry = map.get(job_id).ok_or("Transfer no longer exists")?;
        if CLOSING.load(std::sync::atomic::Ordering::Acquire)
            || entry.cancel_requested
            || entry.snapshot.status.is_terminal()
        {
            return Ok(None);
        }
        if entry.snapshot.status != TransferStatus::Running {
            return Err("Transfer has not started".into());
        }
    }
    // Filesystem I/O holds this job's gate, never the process-global map.
    let bytes = publish()?;
    let logged = {
        let mut map = lock_registry();
        let entry = map
            .get_mut(job_id)
            .ok_or("Transfer disappeared during publication")?;
        let logged = finish_entry(entry, TransferStatus::Success, bytes, None);
        prune(&mut map);
        logged
    };
    record_outcome(job_id, TransferStatus::Success, bytes, None, Some(logged));
    Ok(Some(bytes))
}

fn finish_entry(
    entry: &mut Entry,
    status: TransferStatus,
    bytes: u64,
    error: Option<String>,
) -> (String, String, u64) {
    entry.snapshot.status = status;
    entry.snapshot.bytes = bytes;
    entry.snapshot.error = error;
    entry.snapshot.finished_at = Some(now_ms());
    entry.destination = None;
    (
        entry.snapshot.kind.clone(),
        entry.snapshot.target_label.clone(),
        now_ms().saturating_sub(entry.snapshot.started_at),
    )
}

fn record_outcome(
    job_id: &str,
    status: TransferStatus,
    bytes: u64,
    error: Option<String>,
    logged: Option<(String, String, u64)>,
) {
    // Written here, not where the job was started: a record saying "began
    // exporting" is rubbish once the export fails, and this is the one place
    // every transfer passes through exactly once on its way to a real outcome.
    // The registry lock is released first — recording opens a database, and
    // holding this lock across that would put every other job behind it.
    if let Some((kind, label, elapsed)) = logged {
        let outcome = match status {
            TransferStatus::Success => crate::commands::activity::ActivityOutcome::Ok,
            // A cancel is a decision, not a fault: the user asked for it, and a
            // log that reads it as a failure teaches people to ignore failures.
            // Nor is it `Denied`, which says nothing ever ran — a cancelled
            // export did run, took time, and may have left half a file behind.
            TransferStatus::Cancelled => crate::commands::activity::ActivityOutcome::Cancelled,
            _ => crate::commands::activity::ActivityOutcome::Failed,
        };
        let mut entry = crate::commands::activity::ActivityEntry::new(
            crate::commands::activity::ActivityKind::Task,
            &kind,
            "transfer",
            job_id,
        )
        .named(&label)
        .took(elapsed as i64);
        entry.outcome = outcome;
        if let Some(reason) = error {
            entry.detail = reason;
        } else {
            // Bytes rather than a formatted size: this store is read by a UI that
            // formats for its own locale, and a number never needs reparsing.
            entry.detail = format!("{bytes} bytes transferred");
        }
        crate::commands::activity::record(entry);
    }
}

/// Everything worth showing: unfinished jobs, plus recent outcomes.
pub fn list() -> Vec<TransferSnapshot> {
    let mut map = lock_registry();
    prune(&mut map);
    let mut out: Vec<TransferSnapshot> = map.values().map(|e| e.snapshot.clone()).collect();
    // Newest first, and never dependent on HashMap iteration order.
    out.sort_by_key(|a| std::cmp::Reverse(a.started_at));
    out
}

/// Settles a job if nothing else did, when it goes out of scope.
///
/// The job runner can stop without reaching a terminal branch — a panic inside the
/// blocking task, or the runtime dropping it during shutdown. The entry would then
/// sit at `Running` forever, and because [`prune`] only drops *finished* entries it
/// would hold its destination claim for the life of the process: that path could
/// never be exported to again. Call [`JobGuard::done`] on the normal paths so this
/// only fires when something went wrong.
pub struct JobGuard {
    job_id: String,
    settled: bool,
}

impl JobGuard {
    pub fn new(job_id: &str) -> Self {
        Self {
            job_id: job_id.to_string(),
            settled: false,
        }
    }

    /// The job reached a terminal state on its own; stand down.
    pub fn done(mut self) {
        self.settled = true;
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        settle(
            &self.job_id,
            TransferStatus::Failed,
            0,
            Some("The transfer stopped unexpectedly.".to_string()),
        );
        crate::streaming_cmd::disarm_cancel(&self.job_id);
    }
}

/// What a cancel request actually did.
///
/// A bare `bool` could not tell "there is no such job" from "it had already
/// finished", so callers showed a spinner that never resolved for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CancelOutcome {
    /// The job was running and has been asked to stop. The terminal state still
    /// arrives as an event.
    Cancelled,
    AlreadyFinished,
    UnknownJob,
}

/// Flag the job as cancelled and report what was there.
///
/// Killing the process is the caller's job — this only records intent, which is
/// what closes the window between registering and spawning.
pub fn is_cancel_requested(job_id: &str) -> bool {
    lock_registry()
        .get(job_id)
        .is_some_and(|entry| entry.cancel_requested)
}

pub(crate) fn is_unfinished(job_id: &str) -> bool {
    lock_registry()
        .get(job_id)
        .is_some_and(|entry| !entry.snapshot.status.is_terminal())
}

pub fn request_cancel(job_id: &str) -> CancelOutcome {
    let publication = match cancel_target(job_id) {
        Ok(gate) => gate,
        Err(outcome) => return outcome,
    };
    let _publication = publication.lock().unwrap_or_else(|e| e.into_inner());
    cancel_locked(job_id)
}

/// UI/HTTP/shutdown must not wait behind a final filesystem publication.
pub(crate) fn try_request_cancel(job_id: &str) -> Result<CancelOutcome, String> {
    let publication = match cancel_target(job_id) {
        Ok(gate) => gate,
        Err(outcome) => return Ok(outcome),
    };
    let _publication =
        match publication.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return Err(
                "Output publication is finishing. Wait for its outcome before cancelling again."
                    .into(),
            ),
        };
    Ok(cancel_locked(job_id))
}

fn publication_gate(job_id: &str) -> Option<Arc<Mutex<()>>> {
    lock_registry()
        .get(job_id)
        .map(|entry| entry.publication.clone())
}

fn cancel_target(job_id: &str) -> Result<Arc<Mutex<()>>, CancelOutcome> {
    let map = lock_registry();
    match map.get(job_id) {
        None => Err(CancelOutcome::UnknownJob),
        Some(entry) if entry.snapshot.status.is_terminal() => Err(CancelOutcome::AlreadyFinished),
        Some(entry) => Ok(entry.publication.clone()),
    }
}

fn cancel_locked(job_id: &str) -> CancelOutcome {
    let mut map = lock_registry();
    match map.get_mut(job_id) {
        None => CancelOutcome::UnknownJob,
        Some(entry) if entry.snapshot.status.is_terminal() => CancelOutcome::AlreadyFinished,
        Some(entry) => {
            entry.cancel_requested = true;
            CancelOutcome::Cancelled
        }
    }
}

/// Whether a job is claiming `path` right now. Test seam for the conflict rule.
#[cfg(test)]
fn destination_is_claimed(path: &std::path::Path) -> bool {
    lock_registry()
        .values()
        .any(|e| !e.snapshot.status.is_terminal() && e.destination.as_deref() == Some(path))
}

/// Forget everything. Tests only — the registry is process-global.
#[cfg(test)]
fn reset() {
    lock_registry().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    // TC032/036/037: the actual publication admission orders cancel and Success.
    #[test]
    fn accepted_cancel_refuses_publication_and_committed_output_refuses_late_cancel() {
        let _guard = exclusive();
        register("cancel-before-publish", "save", "fixture", None, None).unwrap();
        assert!(set_running("cancel-before-publish"));
        assert_eq!(
            request_cancel("cancel-before-publish"),
            CancelOutcome::Cancelled
        );
        assert_eq!(
            commit_output("cancel-before-publish", || panic!(
                "cancelled output must not publish"
            ))
            .unwrap(),
            None
        );
        assert_eq!(
            complete("cancel-before-publish", TransferStatus::Success, 100, None),
            TransferStatus::Cancelled
        );
        assert_eq!(
            list()
                .iter()
                .find(|job| job.job_id == "cancel-before-publish")
                .unwrap()
                .bytes,
            0
        );

        register("publish-before-cancel", "save", "fixture", None, None).unwrap();
        assert!(set_running("publish-before-cancel"));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (attempt_tx, attempt_rx) = std::sync::mpsc::channel();
        let publisher = std::thread::spawn(move || {
            commit_output("publish-before-cancel", || {
                entered_tx.send(()).unwrap();
                attempt_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                Ok(17)
            })
        });
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(
            try_request_cancel("publish-before-cancel").is_err(),
            "UI cancellation must refuse an in-progress commit without blocking or accepting it"
        );
        assert_eq!(
            list()
                .iter()
                .find(|job| job.job_id == "publish-before-cancel")
                .unwrap()
                .status,
            TransferStatus::Running
        );
        register("unrelated", "save", "fixture", None, None).unwrap();
        assert!(register("unrelated", "save", "fixture", None, None).is_err());
        assert_eq!(
            try_request_cancel("unrelated").unwrap(),
            CancelOutcome::Cancelled
        );
        assert_eq!(
            complete("unrelated", TransferStatus::Success, 1, None),
            TransferStatus::Cancelled
        );
        let canceller = std::thread::spawn(move || {
            attempt_tx.send(()).unwrap();
            request_cancel("publish-before-cancel")
        });
        assert_eq!(publisher.join().unwrap().unwrap(), Some(17));
        assert_eq!(canceller.join().unwrap(), CancelOutcome::AlreadyFinished);
        assert_eq!(
            complete("publish-before-cancel", TransferStatus::Cancelled, 0, None),
            TransferStatus::Success
        );
        let job = list()
            .into_iter()
            .find(|job| job.job_id == "publish-before-cancel")
            .unwrap();
        assert_eq!(job.status, TransferStatus::Success);
        assert_eq!(job.bytes, 17);
    }

    // TC011/032/036/037: retention only removes ended entries, so registering
    // more work cannot erase current claims, cancellation or restart authority.
    #[test]
    fn unfinished_transfers_keep_their_claims_when_more_work_is_registered() {
        let _guard = exclusive();
        let destination = std::env::temp_dir().join("colimaui-retained-claim.tar");
        register("queued", "save", "fixture", None, Some(destination.clone())).unwrap();
        register("other", "save", "fixture", None, None).unwrap();
        assert_eq!(list().len(), 2);
        assert!(destination_is_claimed(&destination));
        assert!(register("conflict", "save", "fixture", None, Some(destination)).is_err());
        assert!(begin_shutdown(false).is_err());
        assert_eq!(request_cancel("queued"), CancelOutcome::Cancelled);
        assert!(!set_running("queued"));
        assert!(set_running("other"));
        assert!(list()
            .iter()
            .any(|job| job.job_id == "other" && job.status == TransferStatus::Running));
        assert!(!set_running("unregistered"));
    }

    #[test]
    fn ended_transfers_cannot_run_or_change_their_first_outcome() {
        let _g = exclusive();
        for status in [
            TransferStatus::Success,
            TransferStatus::Failed,
            TransferStatus::Cancelled,
        ] {
            let id = format!("ended-{status:?}");
            register(&id, "save", "fixture", None, None).unwrap();
            settle(&id, status, 7, None);
            assert!(!set_running(&id));
            settle(&id, TransferStatus::Running, 9, None);
            settle(&id, TransferStatus::Failed, 11, Some("late".into()));
            let job = list().into_iter().find(|j| j.job_id == id).unwrap();
            assert_eq!(job.status, status);
            assert_eq!(job.bytes, 7);
            assert!(job.error.is_none());
        }
        assert!(!set_running("missing"));
    }

    #[test]
    #[ignore = "isolated subprocess entry point used by restart_admission_is_atomic"]
    fn shutdown_admission_fixture() {
        register("active", "save", "fixture", None, None).unwrap();
        assert!(begin_shutdown(false).is_err());
        register("still-allowed", "save", "fixture", None, None).unwrap();
        assert!(begin_shutdown(true).is_ok());
        assert!(register("late", "save", "fixture", None, None).is_err());
    }

    #[test]
    fn restart_admission_is_atomic_and_refusal_keeps_work_available() {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "transfer_registry::tests::shutdown_admission_fixture",
            "--ignored",
            "--nocapture",
        ]);
        let result =
            crate::child_process::capture(command, Duration::from_secs(10), 1024 * 1024).unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    /// The registry is process-global, so these tests must not interleave.
    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        guard
    }

    #[test]
    fn a_registered_job_is_listed_before_it_runs() {
        let _guard = exclusive();
        register("save-1", "save", "alpine:3.19", Some(10), None).unwrap();

        let listed = list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].status, TransferStatus::Starting);
        assert_eq!(listed[0].total_estimate, Some(10));
    }

    /// The failure this whole module exists for: an outcome that lands before the
    /// caller even holds the job id must still be readable afterwards.
    #[test]
    fn an_outcome_survives_long_enough_to_be_reconciled() {
        let _guard = exclusive();
        register("save-2", "save", "alpine:3.19", None, None).unwrap();
        settle("save-2", TransferStatus::Failed, 0, Some("boom".into()));

        let listed = list();
        assert_eq!(
            listed.len(),
            1,
            "a finished job must not vanish immediately"
        );
        assert_eq!(listed[0].status, TransferStatus::Failed);
        assert_eq!(listed[0].error.as_deref(), Some("boom"));
    }

    #[test]
    fn two_jobs_cannot_write_the_same_destination() {
        let _guard = exclusive();
        let dest = PathBuf::from("/tmp/colimaui-registry-test/out.tar");

        register("save-3", "save", "a", None, Some(dest.clone())).unwrap();
        let err = register("save-4", "save", "b", None, Some(dest.clone()))
            .expect_err("the second job must be refused");
        assert!(err.contains("save-3"), "unexpected: {}", err);

        // Once the first finishes the path is free, even though its entry is still
        // readable for the retention window.
        settle("save-3", TransferStatus::Success, 1, None);
        assert!(!destination_is_claimed(&dest));
        register("save-4", "save", "b", None, Some(dest)).expect("the path is free now");
    }

    #[test]
    fn different_destinations_do_not_conflict() {
        let _guard = exclusive();
        register("save-5", "save", "a", None, Some("/tmp/a.tar".into())).unwrap();
        register("save-6", "save", "b", None, Some("/tmp/b.tar".into()))
            .expect("unrelated paths are independent");
    }

    #[test]
    fn cancelling_distinguishes_missing_from_finished() {
        let _guard = exclusive();
        assert_eq!(request_cancel("nope"), CancelOutcome::UnknownJob);

        register("cp-out-1", "cp-out", "/tmp/x", None, None).unwrap();
        assert_eq!(request_cancel("cp-out-1"), CancelOutcome::Cancelled);

        settle("cp-out-1", TransferStatus::Cancelled, 0, None);
        assert_eq!(request_cancel("cp-out-1"), CancelOutcome::AlreadyFinished);
    }

    /// A cancel arriving before the child exists must not be lost.
    #[test]
    fn a_cancel_during_startup_stops_the_job_from_running() {
        let _guard = exclusive();
        register("save-7", "save", "a", None, None).unwrap();
        assert_eq!(request_cancel("save-7"), CancelOutcome::Cancelled);

        assert!(
            !set_running("save-7"),
            "a job cancelled during startup must not proceed"
        );
    }

    #[test]
    fn a_job_nobody_cancelled_proceeds() {
        let _guard = exclusive();
        register("save-8", "save", "a", None, None).unwrap();
        assert!(set_running("save-8"));
        assert_eq!(list()[0].status, TransferStatus::Running);
    }
}
