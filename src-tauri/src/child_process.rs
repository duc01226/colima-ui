//! Literal-argv child ownership and bounded output. Windows children enter a
//! kill-on-close Job while suspended, before their first instruction. Unix
//! children use a separate process group. No consumer/job identifiers live here.

use std::io::{self, Read};
use std::ops::{Deref, DerefMut};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::{Duration, Instant};

pub struct ChildProcess {
    child: Child,
    tree: Arc<ProcessTree>,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
pub struct TreeUsage {
    pub cpu_seconds: f64,
    pub resident_bytes: u64,
    pub live_processes: u32,
    pub total_processes: u32,
}

struct ProcessTree {
    #[cfg(unix)]
    pid: u32,
    terminated: AtomicBool,
    #[cfg(windows)]
    job: native::Job,
}
impl ProcessTree {
    fn terminate(&self) {
        if self.terminated.swap(true, Ordering::SeqCst) {
            return;
        }
        #[cfg(windows)]
        self.job.terminate();
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.pid as i32), libc::SIGKILL);
        }
    }
}
static STOPPING: AtomicBool = AtomicBool::new(false);
static TREES: LazyLock<Mutex<Vec<Weak<ProcessTree>>>> = LazyLock::new(|| Mutex::new(Vec::new()));

pub fn active_child_count() -> usize {
    TREES.lock().map_or(0, |trees| {
        trees.iter().filter(|tree| tree.strong_count() > 0).count()
    })
}

/// Prevent late acquisition and terminate all owned short/streaming descendants.
/// The caller separately settles consumer jobs/files and interactive sessions.
pub fn shutdown_children() {
    STOPPING.store(true, Ordering::SeqCst);
    let trees = TREES.lock().unwrap_or_else(|e| e.into_inner());
    for tree in trees.iter().filter_map(Weak::upgrade) {
        tree.terminate();
    }
}

impl Deref for ChildProcess {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}
impl DerefMut for ChildProcess {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl ChildProcess {
    /// Cumulative CPU (including exited descendants) and current summed live
    /// working sets for this owned Windows Job. Observation never discovers
    /// unrelated processes by name or parent-PID heuristics.
    #[cfg(windows)]
    pub fn resource_usage(&self) -> io::Result<TreeUsage> {
        self.tree.job.resource_usage()
    }

    pub fn spawn(mut command: Command) -> io::Result<Self> {
        if STOPPING.load(Ordering::SeqCst) {
            return Err(io::Error::other("Application is closing"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        let job = {
            use std::os::windows::process::CommandExt;
            use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
            // Preserve an existing CLI console for inherited stdin/Ctrl-C.
            // A GUI-launched background command must not create a new window.
            let console_flag =
                if unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() }.is_null() {
                    CREATE_NO_WINDOW
                } else {
                    0
                };
            command.creation_flags(CREATE_SUSPENDED | console_flag);
            native::Job::new()?
        };
        let child = command.spawn()?;
        let owned = Self {
            tree: Arc::new(ProcessTree {
                #[cfg(unix)]
                pid: child.id(),
                terminated: AtomicBool::new(false),
                #[cfg(windows)]
                job,
            }),
            child,
        };
        let mut trees = TREES
            .lock()
            .map_err(|_| io::Error::other("Child registry unavailable"))?;
        if STOPPING.load(Ordering::SeqCst) {
            return Err(io::Error::other("Application is closing"));
        }
        trees.retain(|tree| tree.strong_count() > 0);
        trees.push(Arc::downgrade(&owned.tree));
        #[cfg(windows)]
        {
            let mut owned = owned;
            if let Err(e) = owned.tree.job.assign_and_resume(&owned.child) {
                owned.terminate();
                let _ = owned.child.wait();
                return Err(e);
            }
            Ok(owned)
        }
        #[cfg(not(windows))]
        Ok(owned)
    }

    pub fn terminate(&mut self) {
        self.tree.terminate();
        let _ = self.child.kill();
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        self.terminate();
        let _ = self.child.wait();
    }
}

/// Readers drain concurrently, retain only the declared limit, and signal the
/// waiter to terminate on overflow. Neither a full pipe nor an orphaned helper
/// can keep a deadline from settling.
pub fn capture(mut command: Command, timeout: Duration, limit: usize) -> io::Result<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = ChildProcess::spawn(command)?;
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout = reader(
        child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Missing output pipe"))?,
        limit,
        overflow.clone(),
    );
    let stderr = reader(
        child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("Missing error pipe"))?,
        limit,
        overflow.clone(),
    );
    let deadline = Instant::now() + timeout;
    let status = loop {
        if overflow.load(Ordering::Acquire) {
            break Err(io::Error::other("Process output exceeds its size limit"));
        }
        if Instant::now() >= deadline {
            break Err(io::Error::new(io::ErrorKind::TimedOut, "Process timed out"));
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => break Err(e),
        }
    };
    // Settle descendants even if their parent exited with success; they must
    // not retain our pipes or outlive the operation that acquired them.
    child.terminate();
    let _ = child.wait();
    let stdout = stdout
        .join()
        .map_err(|_| io::Error::other("Output reader failed"))??;
    let stderr = stderr
        .join()
        .map_err(|_| io::Error::other("Error reader failed"))??;
    if overflow.load(Ordering::Acquire) {
        return Err(io::Error::other("Process output exceeds its size limit"));
    }
    Ok(Output {
        status: status?,
        stdout,
        stderr,
    })
}

fn reader(
    mut input: impl Read + Send + 'static,
    limit: usize,
    overflow: Arc<AtomicBool>,
) -> std::thread::JoinHandle<io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut buffer = [0; 8192];
        loop {
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            let keep = count.min(limit.saturating_sub(bytes.len()));
            bytes.extend_from_slice(&buffer[..keep]);
            if keep < count {
                overflow.store(true, Ordering::Release);
            }
        }
        Ok(bytes)
    })
}

#[cfg(windows)]
pub(crate) mod native {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    use windows_sys::Win32::System::JobObjects::*;
    use windows_sys::Win32::System::Threading::*;

    #[derive(Debug)]
    pub struct Job(HANDLE);
    // Exclusive ownership moves with ChildProcess; kernel handles are not
    // thread-affine. No borrowed native pointer is stored here.
    unsafe impl Send for Job {}
    // Job operations use synchronized kernel state; there is no borrowed memory.
    unsafe impl Sync for Job {}
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    impl Job {
        pub fn resource_usage(&self) -> io::Result<TreeUsage> {
            use windows_sys::Win32::System::ProcessStatus::{
                GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
            };
            // Bounded storage, aligned for the native ULONG_PTR array. Refuse
            // truncation instead of undercounting a larger process tree.
            #[repr(C)]
            struct ProcessIds {
                assigned: u32,
                count: u32,
                ids: [usize; 1024],
            }
            let mut ids = ProcessIds {
                assigned: 0,
                count: 0,
                ids: [0; 1024],
            };
            if unsafe {
                QueryInformationJobObject(
                    self.0,
                    JobObjectBasicProcessIdList,
                    std::ptr::addr_of_mut!(ids).cast(),
                    std::mem::size_of_val(&ids) as u32,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if ids.count != ids.assigned || ids.count as usize > ids.ids.len() {
                return Err(io::Error::other("Incomplete owned process list"));
            }
            let mut resident_bytes = 0u64;
            for &pid in &ids.ids[..ids.count as usize] {
                let pid =
                    u32::try_from(pid).map_err(|_| io::Error::other("Invalid owned process ID"))?;
                let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
                if process.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let process = Handle(process);
                let mut belongs = 0;
                if unsafe { IsProcessInJob(process.0, self.0, &mut belongs) } == 0 || belongs == 0 {
                    return Err(io::Error::other("Owned process changed during measurement"));
                }
                let mut memory = PROCESS_MEMORY_COUNTERS::default();
                memory.cb = std::mem::size_of_val(&memory) as u32;
                if unsafe {
                    GetProcessMemoryInfo(
                        process.0,
                        &mut memory,
                        std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                    )
                } == 0
                {
                    return Err(io::Error::last_os_error());
                }
                resident_bytes = resident_bytes
                    .checked_add(memory.WorkingSetSize as u64)
                    .ok_or_else(|| io::Error::other("Owned resident-byte overflow"))?;
            }
            let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            if unsafe {
                QueryInformationJobObject(
                    self.0,
                    JobObjectBasicAccountingInformation,
                    std::ptr::addr_of_mut!(accounting).cast(),
                    std::mem::size_of_val(&accounting) as u32,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let ticks = accounting
                .TotalUserTime
                .checked_add(accounting.TotalKernelTime)
                .filter(|ticks| *ticks >= 0)
                .ok_or_else(|| io::Error::other("Invalid owned CPU counters"))?;
            Ok(TreeUsage {
                cpu_seconds: ticks as f64 / 10_000_000.0,
                resident_bytes,
                live_processes: ids.count,
                total_processes: accounting.TotalProcesses,
            })
        }

        pub fn raw(&self) -> HANDLE {
            self.0
        }
        pub fn new() -> io::Result<Self> {
            let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Self(raw);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if unsafe {
                SetInformationJobObject(
                    raw,
                    JobObjectExtendedLimitInformation,
                    std::ptr::addr_of!(limits).cast(),
                    std::mem::size_of_val(&limits) as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }

        pub fn terminate(&self) {
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }

        pub fn assign_and_resume(&self, child: &Child) -> io::Result<()> {
            if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // std::process closes the primary thread handle. The child is still
            // suspended and has not run user code: enumerate its sole initial
            // thread, validate its owner, then resume only after Job assignment.
            let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let snapshot = Handle(snapshot);
            let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
            entry.dwSize = std::mem::size_of_val(&entry) as u32;
            let mut threads = Vec::new();
            let mut found = unsafe { Thread32First(snapshot.0, &mut entry) };
            while found != 0 {
                if entry.th32OwnerProcessID == child.id() {
                    threads.push(entry.th32ThreadID);
                }
                entry.dwSize = std::mem::size_of_val(&entry) as u32;
                found = unsafe { Thread32Next(snapshot.0, &mut entry) };
            }
            if threads.len() != 1 {
                return Err(io::Error::other(
                    "Cannot identify the suspended child thread",
                ));
            }
            let thread = unsafe {
                OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
                    0,
                    threads[0],
                )
            };
            if thread.is_null() {
                return Err(io::Error::last_os_error());
            }
            let thread = Handle(thread);
            if unsafe { GetProcessIdOfThread(thread.0) } != child.id() {
                return Err(io::Error::other("Child thread ownership changed"));
            }
            if unsafe { ResumeThread(thread.0) } != 1 {
                return Err(io::Error::other("Cannot resume the owned child"));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn fixture(name: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", name, "--ignored", "--nocapture"]);
        command
    }

    #[test]
    #[ignore = "owned subprocess fixture invoked by the guarding tests"]
    fn output_fixture() {
        println!("literal UTF-8: 工具 with spaces");
        eprintln!("separate stderr");
    }

    #[test]
    #[ignore = "owned subprocess fixture invoked by the guarding tests"]
    fn tree_fixture() {
        let mut child = fixture("child_process::tests::leaf_fixture")
            .spawn()
            .unwrap();
        std::fs::write(
            std::env::var_os("COLIMA_PROCESS_FIXTURE").unwrap(),
            child.id().to_string(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_secs(30));
        child.wait().unwrap();
    }

    #[test]
    #[ignore = "owned subprocess fixture invoked by the guarding tests"]
    fn leaf_fixture() {
        std::thread::sleep(Duration::from_secs(30));
    }

    #[test]
    #[ignore = "isolated shutdown fixture; never mutates the parent test process"]
    fn shutdown_fixture() {
        let mut child = ChildProcess::spawn(fixture("child_process::tests::leaf_fixture")).unwrap();
        assert_eq!(active_child_count(), 1);
        shutdown_children();
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(!status.success());
                break;
            }
            assert!(Instant::now() < until, "owned child survived shutdown");
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(child);
        assert_eq!(active_child_count(), 0);
        assert!(ChildProcess::spawn(fixture("child_process::tests::output_fixture")).is_err());
        println!("SHUTDOWN-SETTLED");
    }

    #[test]
    fn application_shutdown_ends_owned_children_and_refuses_late_acquisition() {
        let output = capture(
            fixture("child_process::tests::shutdown_fixture"),
            Duration::from_secs(5),
            8192,
        )
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("SHUTDOWN-SETTLED"));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "owned CPU descendant fixture"]
    fn accounting_leaf_fixture() {
        let start = Instant::now();
        let mut value = 1u64;
        while start.elapsed() < Duration::from_millis(500) {
            for _ in 0..10000 {
                value =
                    std::hint::black_box(value.wrapping_mul(6364136223846793005).wrapping_add(1));
            }
        }
        std::hint::black_box(value);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "owned accounting parent fixture"]
    fn accounting_tree_fixture() {
        let mut child = fixture("child_process::tests::accounting_leaf_fixture")
            .spawn()
            .unwrap();
        assert!(child.wait().unwrap().success());
        std::fs::write(
            std::env::var_os("COLIMA_PROCESS_FIXTURE").unwrap(),
            "reaped",
        )
        .unwrap();
        std::thread::sleep(Duration::from_secs(30));
    }

    // TC021: after the CPU-heavy descendant is reaped, Job accounting must
    // retain its CPU beyond the still-live direct child's own process time.
    #[cfg(windows)]
    #[test]
    fn resource_usage_includes_exited_descendant_cpu_and_live_resident_bytes() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::GetProcessTimes;
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/process-accounting");
        std::fs::create_dir_all(&root).unwrap();
        let marker = root.join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(!marker.exists());
        let mut command = fixture("child_process::tests::accounting_tree_fixture");
        command
            .env("COLIMA_PROCESS_FIXTURE", &marker)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = ChildProcess::spawn(command).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !marker.exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let usage = child.resource_usage().unwrap();
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        assert_ne!(
            unsafe {
                GetProcessTimes(
                    child.as_raw_handle(),
                    &mut created,
                    &mut exited,
                    &mut kernel,
                    &mut user,
                )
            },
            0
        );
        let ticks =
            |time: FILETIME| ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64;
        let parent_cpu = (ticks(kernel) + ticks(user)) as f64 / 10_000_000.0;
        assert!(usage.total_processes >= 2);
        assert!(usage.live_processes >= 1);
        assert!(usage.resident_bytes > 0);
        assert!(
            usage.cpu_seconds - parent_cpu > 0.01,
            "exited descendant CPU was omitted"
        );
        drop(child);
        std::fs::remove_file(marker).unwrap();
    }

    // TC-WINUI-011/016: child output is honest/bounded and deadlines own the
    // entire tree, using real executable fixtures on each supported host.
    #[test]
    fn literal_output_is_preserved_and_overflow_is_not_success() {
        let out = capture(
            fixture("child_process::tests::output_fixture"),
            Duration::from_secs(5),
            4096,
        )
        .unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("literal UTF-8: 工具 with spaces"));
        assert!(String::from_utf8_lossy(&out.stderr).contains("separate stderr"));
        assert!(capture(
            fixture("child_process::tests::output_fixture"),
            Duration::from_secs(5),
            8
        )
        .is_err());
    }

    #[test]
    fn deadline_removes_descendants_and_settles_output() {
        let marker = std::env::temp_dir().join(format!(
            "colimaui-child-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut command = fixture("child_process::tests::tree_fixture");
        command.env("COLIMA_PROCESS_FIXTURE", &marker);
        let start = Instant::now();
        let error = capture(command, Duration::from_secs(3), 4096).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "descendant retained a pipe"
        );
        let pid: u32 = std::fs::read_to_string(&marker).unwrap().parse().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while running(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!running(pid), "owned descendant is still running");
        std::fs::remove_file(marker).unwrap();
    }

    #[cfg(unix)]
    fn running(pid: u32) -> bool {
        Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .ok()
            .is_some_and(|o| {
                o.status.success()
                    && !String::from_utf8_lossy(&o.stdout).trim().is_empty()
                    && !String::from_utf8_lossy(&o.stdout).trim().starts_with('Z')
            })
    }

    #[cfg(windows)]
    fn running(pid: u32) -> bool {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return false;
        }
        let mut code = 0;
        let result = unsafe { GetExitCodeProcess(process, &mut code) };
        unsafe {
            CloseHandle(process);
        }
        assert_ne!(result, 0, "missing required child state");
        code == STILL_ACTIVE as u32
    }
}
