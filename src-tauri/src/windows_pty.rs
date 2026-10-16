//! ConPTY with atomic Job membership at creation. The portable-pty 0.9 Windows
//! spawn API cannot set creation attributes; keep that platform seam here.
//! See Microsoft Learn: creating-a-pseudoconsole-session and
//! UpdateProcThreadAttribute (PROC_THREAD_ATTRIBUTE_JOB_LIST, Windows 10+).

use crate::child_process::native::Job;
use portable_pty::{ExitStatus, PtySize};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Command;
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Console::*;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::*;

pub struct Master {
    console: HPCON,
    size: Mutex<PtySize>,
    stop_input: Arc<std::sync::atomic::AtomicBool>,
    input_thread: Option<std::thread::JoinHandle<()>>,
    output_thread: Option<std::thread::JoinHandle<()>>,
}
// An exclusively owned kernel console has no Rust references or thread affinity.
unsafe impl Send for Master {}
impl Drop for Master {
    fn drop(&mut self) {
        // The independent output pump below keeps draining through this call.
        self.stop_input
            .store(true, std::sync::atomic::Ordering::Release);
        unsafe {
            ClosePseudoConsole(self.console);
        }
        if let Some(thread) = self.input_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.output_thread.take() {
            let _ = thread.join();
        }
    }
}
impl Master {
    pub fn resize(&self, size: PtySize) -> io::Result<()> {
        let coord = coord(size)?;
        hr(unsafe { ResizePseudoConsole(self.console, coord) })?;
        *self
            .size
            .lock()
            .map_err(|_| io::Error::other("Console unavailable"))? = size;
        Ok(())
    }
}

#[derive(Debug)]
pub struct Child {
    process: OwnedHandle,
    job: Arc<Job>,
}
impl Child {
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0;
                if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                self.job.terminate();
                Ok(Some(ExitStatus::with_exit_code(code)))
            }
            _ => Err(io::Error::last_os_error()),
        }
    }
    pub fn kill(&mut self) -> io::Result<()> {
        self.job.terminate();
        Ok(())
    }
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        if unsafe { WaitForSingleObject(self.process.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        self.try_wait()?
            .ok_or_else(|| io::Error::other("Terminal did not exit"))
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        self.job.terminate();
        let _ = self.wait();
    }
}

/// A bounded in-process output pipe. Its pump must drain ConPTY even when the
/// caller has closed its reader; otherwise ClosePseudoConsole can deadlock.
struct Output {
    bytes: Mutex<std::collections::VecDeque<u8>>,
    wake: std::sync::Condvar,
    ended: std::sync::atomic::AtomicBool,
}
struct Reader(Arc<Output>);
impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut bytes = self
            .0
            .bytes
            .lock()
            .map_err(|_| io::Error::other("Console output unavailable"))?;
        while bytes.is_empty() && !self.0.ended.load(std::sync::atomic::Ordering::Acquire) {
            bytes = self
                .0
                .wake
                .wait(bytes)
                .map_err(|_| io::Error::other("Console output unavailable"))?;
        }
        let count = buf.len().min(bytes.len());
        for slot in &mut buf[..count] {
            *slot = bytes.pop_front().unwrap();
        }
        Ok(count)
    }
}

struct Writer(std::sync::mpsc::SyncSender<Vec<u8>>);
impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > 65536 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Terminal input exceeds 64 KiB",
            ));
        }
        self.0.try_send(bytes.to_vec()).map_err(|_| {
            io::Error::new(io::ErrorKind::WouldBlock, "Terminal input closed or busy")
        })?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub struct Opened {
    pub master: Master,
    pub child: Child,
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
}

pub fn spawn(command: Command, size: PtySize) -> io::Result<Opened> {
    let program = std::path::Path::new(command.get_program());
    if !program.is_absolute() || !program.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Terminal executable must be an existing absolute path",
        ));
    }
    let application = wide(command.get_program())?;
    let mut line = command_line(&command)?;
    let mut env = environment(&command)?;
    let cwd = command
        .get_current_dir()
        .map(|p| wide(p.as_os_str()))
        .transpose()?;
    let (input_read, input_write) = pipe()?;
    let (output_read, output_write) = pipe()?;
    let mut console = 0;
    hr(unsafe {
        CreatePseudoConsole(
            coord(size)?,
            input_read.as_raw_handle(),
            output_write.as_raw_handle(),
            0,
            &mut console,
        )
    })?;
    let mut master = Master {
        console,
        size: Mutex::new(size),
        stop_input: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        input_thread: None,
        output_thread: None,
    };
    // Reverse local drop order matters: on any later acquisition error these
    // duplicate pipe ends must close before Master joins its output reader.
    // Keep them until process creation succeeds on the normal path.
    let parent_pipe_ends = (input_read, output_write);

    let output = Arc::new(Output {
        bytes: Mutex::new(std::collections::VecDeque::new()),
        wake: std::sync::Condvar::new(),
        ended: std::sync::atomic::AtomicBool::new(false),
    });
    let sink = output.clone();
    // Start before any subsequent fallible operation, including process creation.
    // This remains active until ConPTY closes its output, including error paths.
    master.output_thread = Some(
        std::thread::Builder::new()
            .name("console-output".into())
            .spawn(move || {
                let mut file = File::from(output_read);
                let mut buf = [0; 8192];
                while let Ok(n) = file.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    if let Ok(mut bytes) = sink.bytes.lock() {
                        bytes.extend(&buf[..n]);
                        if bytes.len() > 1024 * 1024 {
                            let excess = bytes.len() - 512 * 1024;
                            bytes.drain(..excess);
                            while bytes.front().is_some_and(|b| b & 0xc0 == 0x80) {
                                bytes.pop_front();
                            }
                        }
                    }
                    sink.wake.notify_one();
                }
                sink.ended.store(true, std::sync::atomic::Ordering::Release);
                sink.wake.notify_all();
            })?,
    );

    let job = Arc::new(Job::new()?);
    let mut attributes = Attributes::new(2)?;
    attributes.set(
        PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
        console as *const _,
        std::mem::size_of::<HPCON>(),
    )?;
    let job_handle = job.raw();
    attributes.set(
        PROC_THREAD_ATTRIBUTE_JOB_LIST,
        std::ptr::addr_of!(job_handle).cast(),
        std::mem::size_of_val(&job_handle),
    )?;
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of_val(&startup) as u32;
    startup.lpAttributeList = attributes.ptr();
    let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            env.as_mut_ptr().cast(),
            cwd.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
            &startup.StartupInfo,
            &mut process,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let thread = unsafe { OwnedHandle::from_raw_handle(process.hThread) };
    let child = Child {
        process: unsafe { OwnedHandle::from_raw_handle(process.hProcess) },
        job,
    };
    drop(thread);
    drop(parent_pipe_ends);
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(8);
    let stop = master.stop_input.clone();
    master.input_thread = Some(
        std::thread::Builder::new()
            .name("console-input".into())
            .spawn(move || {
                let mut file = File::from(input_write);
                while !stop.load(std::sync::atomic::Ordering::Acquire) {
                    match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                        Ok(bytes) => {
                            if file.write_all(&bytes).is_err() {
                                break;
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    }
                }
            })?,
    );
    Ok(Opened {
        master,
        child,
        reader: Box::new(Reader(output)),
        writer: Box::new(Writer(tx)),
    })
}

fn coord(size: PtySize) -> io::Result<COORD> {
    if size.rows == 0
        || size.cols == 0
        || size.rows > i16::MAX as u16
        || size.cols > i16::MAX as u16
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid terminal grid",
        ));
    }
    Ok(COORD {
        X: size.cols as i16,
        Y: size.rows as i16,
    })
}
fn hr(result: i32) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::other(format!(
            "Console operation failed ({result:#x})"
        )))
    } else {
        Ok(())
    }
}
fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
    if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(read),
            OwnedHandle::from_raw_handle(write),
        )
    })
}
fn wide(text: &OsStr) -> io::Result<Vec<u16>> {
    let mut value: Vec<_> = text.encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Null character in terminal command",
        ));
    }
    value.push(0);
    Ok(value)
}
fn command_line(command: &Command) -> io::Result<Vec<u16>> {
    let mut result = Vec::new();
    for arg in std::iter::once(command.get_program()).chain(command.get_args()) {
        if !result.is_empty() {
            result.push(32);
        }
        result.push(34);
        let mut slashes = 0;
        for c in wide(arg)?.into_iter().take_while(|c| *c != 0) {
            if c == 92 {
                slashes += 1;
                continue;
            }
            result.extend(std::iter::repeat_n(
                92,
                if c == 34 { 2 * slashes + 1 } else { slashes },
            ));
            slashes = 0;
            result.push(c);
        }
        result.extend(std::iter::repeat_n(92, slashes * 2));
        result.push(34);
    }
    if result.len() >= 32767 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Terminal command too long",
        ));
    }
    result.push(0);
    Ok(result)
}
fn environment(command: &Command) -> io::Result<Vec<u16>> {
    let mut values: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    for (key, value) in command.get_envs() {
        values.retain(|existing, _| !existing.eq_ignore_ascii_case(key));
        if let Some(value) = value {
            values.insert(key.into(), value.into());
        }
    }
    let mut entries: Vec<Vec<u16>> = values
        .iter()
        .map(|(k, v)| {
            let mut entry = wide(k)?;
            entry.pop();
            entry.push(61);
            entry.extend(wide(v)?);
            Ok(entry)
        })
        .collect::<io::Result<_>>()?;
    entries.sort_by(|a, b| {
        let value = unsafe {
            windows_sys::Win32::Globalization::CompareStringOrdinal(
                a.as_ptr(),
                a.len() as i32 - 1,
                b.as_ptr(),
                b.len() as i32 - 1,
                1,
            )
        };
        value.cmp(&2)
    });
    let mut block: Vec<u16> = entries.into_iter().flatten().collect();
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    Ok(block)
}

struct Attributes {
    storage: Vec<usize>,
}
impl Attributes {
    fn new(count: u32) -> io::Result<Self> {
        let mut size = 0;
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), count, 0, &mut size);
        }
        if size == 0 || size > 65536 {
            return Err(io::Error::other("Invalid process attribute size"));
        }
        let mut owner = Self {
            storage: vec![0; size.div_ceil(std::mem::size_of::<usize>())],
        };
        if unsafe { InitializeProcThreadAttributeList(owner.ptr(), count, 0, &mut size) } == 0 {
            // No initialized list exists yet; Drop must not delete one.
            owner.storage.clear();
            return Err(io::Error::last_os_error());
        }
        Ok(owner)
    }
    fn ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
    fn set(&mut self, key: u32, value: *const std::ffi::c_void, size: usize) -> io::Result<()> {
        if unsafe {
            UpdateProcThreadAttribute(
                self.ptr(),
                0,
                key as usize,
                value,
                size,
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        } == 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if !self.storage.is_empty() {
            unsafe {
                DeleteProcThreadAttributeList(self.ptr());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "owned ConPTY subprocess fixture invoked by the guarding native test"]
    fn console_fixture() {
        assert_eq!(
            std::env::var("PTY_LITERAL").unwrap(),
            "space 路径 \\\"quoted\\\""
        );
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "windows_pty::tests::leaf_fixture",
            "--ignored",
            "--nocapture",
        ]);
        let child = command.spawn().unwrap();
        std::fs::write(
            std::env::var_os("PTY_DESCENDANT").unwrap(),
            child.id().to_string(),
        )
        .unwrap();
        println!("READY:你好");
        io::stdout().flush().unwrap();
        for line in io::stdin().lock().lines() {
            let line = line.unwrap();
            if line.trim() == "END" {
                return;
            }
            println!("ECHO:{}", line.trim());
            io::stdout().flush().unwrap();
        }
    }

    #[test]
    #[ignore = "owned ConPTY descendant fixture invoked by its parent"]
    fn leaf_fixture() {
        std::thread::sleep(Duration::from_secs(30));
    }

    #[test]
    #[ignore = "owned failed-launch subprocess selected by its bounded parent"]
    fn failed_launch_fixture() {
        let program = std::path::PathBuf::from(std::env::var_os("PTY_FAILED_EXECUTABLE").unwrap());
        assert!(program.is_absolute() && program.is_file());
        for _ in 0..3 {
            let error = match spawn(
                Command::new(&program),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            ) {
                Err(error) => error,
                Ok(_) => panic!("nonexecutable fixture unexpectedly launched"),
            };
            // This native result comes from CreateProcessW, after acquiring
            // ConPTY, its output pump, Job and process attributes. Returning
            // it also proves the owned pumps settled on that error path.
            assert_eq!(error.raw_os_error(), Some(ERROR_BAD_EXE_FORMAT as i32));
        }
        println!("FAILED-CONSOLE-LAUNCH-SETTLED");
    }

    #[test]
    fn failed_console_launch_releases_owned_io_before_returning() {
        // Business intent: a failed terminal start must return, so the shared
        // session manager remains available for Close and explicit restart.
        struct Scratch(std::path::PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/tooling-tests")
            .join(format!(
                "console-failed-launch-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir_all(root.parent().unwrap()).unwrap();
        std::fs::create_dir(&root).unwrap();
        let scratch = Scratch(root.canonicalize().unwrap());
        let program = scratch.0.join("unlaunchable.exe");
        std::fs::write(&program, b"owned fixture, not an executable").unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "windows_pty::tests::failed_launch_fixture",
            "--ignored",
            "--nocapture",
        ]);
        command.env("PTY_FAILED_EXECUTABLE", program);
        // The parent owns the subprocess tree and converts a cleanup hang
        // into an assertion, before the external mutation-runner deadline.
        let result = crate::child_process::capture(command, Duration::from_secs(20), 131_072);
        assert!(
            result.is_ok(),
            "failed console acquisition did not settle: {result:?}"
        );
        let output = result.unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("FAILED-CONSOLE-LAUNCH-SETTLED"));
    }

    #[test]
    fn native_console_preserves_utf8_input_resize_exit_and_owned_io_cleanup() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "windows_pty::tests::console_fixture",
            "--ignored",
            "--nocapture",
        ]);
        command.env("PTY_LITERAL", "space 路径 \\\"quoted\\\"");
        let marker = std::env::temp_dir().join(format!("colima-pty-child-{}", std::process::id()));
        command.env("PTY_DESCENDANT", &marker);
        let mut opened = spawn(
            command,
            PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
        )
        .unwrap();
        let (tx, rx) = std::sync::mpsc::sync_channel(128);
        let mut reader = opened.reader;
        let pump = std::thread::spawn(move || {
            let mut buf = [0; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if tx.try_send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut output = Vec::new();
        let mut observe = |expected: &str| {
            let until = Instant::now() + Duration::from_secs(5);
            while !String::from_utf8_lossy(&output).contains(expected) {
                output.extend(
                    rx.recv_timeout(until.saturating_duration_since(Instant::now()))
                        .unwrap(),
                );
            }
        };
        observe("READY:你好");
        let pid: u32 = std::fs::read_to_string(&marker).unwrap().parse().unwrap();
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        assert!(!raw.is_null());
        let descendant = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut member = 0;
        assert_ne!(
            unsafe {
                windows_sys::Win32::System::JobObjects::IsProcessInJob(
                    descendant.as_raw_handle(),
                    opened.child.job.raw(),
                    &mut member,
                )
            },
            0
        );
        assert_ne!(
            member, 0,
            "descendant must inherit the atomic Job membership"
        );
        opened
            .master
            .resize(PtySize {
                rows: 41,
                cols: 117,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        assert!(opened
            .master
            .resize(PtySize {
                rows: 0,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0
            })
            .is_err());
        opened
            .writer
            .write_all("工具 with spaces\r".as_bytes())
            .unwrap();
        observe("ECHO:工具 with spaces");
        opened.writer.write_all(b"END\r").unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = opened.child.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < until, "console did not exit");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(status.success());
        // Dropping the master joins both native pumps; the client reader then
        // reaches EOF. No live pipe/thread is retained by a naturally ended PTY.
        drop(opened.writer);
        drop(opened.child);
        drop(opened.master);
        pump.join().unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(descendant.as_raw_handle(), 3000) },
            WAIT_OBJECT_0,
            "PTY exit left a live descendant"
        );
        std::fs::remove_file(marker).unwrap();
    }

    #[test]
    fn command_line_keeps_empty_quotes_backslashes_and_unicode_literal() {
        let mut command = Command::new(r"C:\Program Files\工具.exe");
        command.args(["", "hello world", "a\"b", "trailing\\", "é界"]);
        let line = command_line(&command).unwrap();
        let text = String::from_utf16(&line[..line.len() - 1]).unwrap();
        assert_eq!(text, "\"C:\\Program Files\\工具.exe\" \"\" \"hello world\" \"a\\\"b\" \"trailing\\\\\" \"é界\"");
        assert!(coord(PtySize {
            rows: 32768,
            cols: 1,
            pixel_width: 0,
            pixel_height: 0
        })
        .is_err());
    }
}
