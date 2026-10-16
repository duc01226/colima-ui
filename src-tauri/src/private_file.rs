//! Owner-only local files. Privacy is established at creation, before bytes are
//! written, and checked on the opened handle before bytes are read.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

pub fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = native::create(path)?;
    let result = native::verify(&file)
        .and_then(|()| file.write_all(bytes))
        .and_then(|()| file.sync_all());
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

pub fn read(path: &Path, maximum: usize) -> io::Result<Vec<u8>> {
    let file = native::open(path)?;
    native::verify(&file)?;
    let mut bytes = Vec::new();
    file.take(maximum.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Private file exceeds its size limit",
        ));
    }
    Ok(bytes)
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "File is not private to the current account",
    )
}

#[cfg(unix)]
mod native {
    use super::*;
    use std::fs::OpenOptions;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    pub fn create(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
    }

    pub fn open(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
    }

    pub fn verify(file: &File) -> io::Result<()> {
        let m = file.metadata()?;
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o077 != 0
            || m.nlink() != 1
        {
            return Err(denied());
        }
        Ok(())
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::Foundation::{
        CloseHandle, LocalFree, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::Authorization::*;
    use windows_sys::Win32::Security::*;
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct LocalAllocation(*mut c_void);
    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }

    struct ProcessToken(HANDLE);
    impl Drop for ProcessToken {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    // usize elements keep TOKEN_USER aligned; its SID points into this buffer.
    struct CurrentUser(Vec<usize>);
    impl CurrentUser {
        fn load() -> io::Result<Self> {
            let mut token = std::ptr::null_mut();
            if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = ProcessToken(token);
            let mut needed = 0;
            unsafe {
                GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut needed);
            }
            if needed < std::mem::size_of::<TOKEN_USER>() as u32 || needed > 65536 {
                return Err(denied());
            }
            let mut data = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
            if unsafe {
                GetTokenInformation(
                    token.0,
                    TokenUser,
                    data.as_mut_ptr().cast(),
                    needed,
                    &mut needed,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(data))
        }

        fn sid(&self) -> PSID {
            unsafe { (*(self.0.as_ptr().cast::<TOKEN_USER>())).User.Sid }
        }

        fn descriptor(&self) -> io::Result<LocalAllocation> {
            let mut text = std::ptr::null_mut();
            if unsafe { ConvertSidToStringSidW(self.sid(), &mut text) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let text_owner = LocalAllocation(text.cast());
            let mut len = 0;
            unsafe {
                while *text.add(len) != 0 {
                    len += 1;
                }
            }
            let sid = String::from_utf16(unsafe { std::slice::from_raw_parts(text, len) })
                .map_err(|_| denied())?;
            drop(text_owner);
            // Protected DACL, one full-access ACE, current account only. Set the
            // owner explicitly too (an elevated token may default to a group).
            let sddl: Vec<u16> = format!("O:{sid}D:P(A;;FA;;;{sid})")
                .encode_utf16()
                .chain([0])
                .collect();
            let mut sd = std::ptr::null_mut();
            if unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut sd,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(LocalAllocation(sd))
        }
    }

    fn open_file(path: &Path, create: bool) -> io::Result<File> {
        let wide = crate::windows_host::wide_path(path)?;
        let descriptor = if create {
            Some(CurrentUser::load()?.descriptor()?)
        } else {
            None
        };
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.as_ref().map_or(std::ptr::null_mut(), |sd| sd.0),
            bInheritHandle: 0,
        };
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                if create {
                    GENERIC_READ | GENERIC_WRITE
                } else {
                    GENERIC_READ
                },
                FILE_SHARE_READ | FILE_SHARE_DELETE,
                if create {
                    &attributes
                } else {
                    std::ptr::null()
                },
                if create { CREATE_NEW } else { OPEN_EXISTING },
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_handle(handle) })
    }

    pub fn create(path: &Path) -> io::Result<File> {
        open_file(path, true)
    }
    pub fn open(path: &Path) -> io::Result<File> {
        open_file(path, false)
    }

    pub fn verify(file: &File) -> io::Result<()> {
        let handle = file.as_raw_handle();
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0
            || info.nNumberOfLinks != 1
        {
            return Err(denied());
        }
        let mut owner = std::ptr::null_mut();
        let mut acl = std::ptr::null_mut();
        let mut sd = std::ptr::null_mut();
        let result = unsafe {
            GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                &mut acl,
                std::ptr::null_mut(),
                &mut sd,
            )
        };
        let descriptor = LocalAllocation(sd);
        if result != 0 {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
        let user = CurrentUser::load()?;
        let (mut control, mut revision) = (0, 0);
        if owner.is_null()
            || acl.is_null()
            || unsafe { EqualSid(owner, user.sid()) } == 0
            || unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) }
                == 0
            || control & SE_DACL_PROTECTED == 0
            || unsafe { (*acl).AceCount } != 1
        {
            return Err(denied());
        }
        let mut ace = std::ptr::null_mut();
        if unsafe { GetAce(acl, 0, &mut ace) } == 0 || ace.is_null() {
            return Err(denied());
        }
        let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        // ACE type 0 is ACCESS_ALLOWED_ACE_TYPE; no inherited/object/callback
        // ACE can broaden this deliberately narrow private-file contract.
        if allowed.Header.AceType != 0
            || allowed.Header.AceFlags != 0
            || allowed.Mask != FILE_ALL_ACCESS
            || unsafe {
                EqualSid(
                    std::ptr::addr_of!(allowed.SidStart).cast_mut().cast(),
                    user.sid(),
                )
            } == 0
        {
            return Err(denied());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "colimaui-private-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // TC-WINUI-014: a new private file is owner-readable, bounded, and cannot
    // overwrite prior data. Native second-account denial is separate proof.
    #[test]
    fn owner_can_read_without_overwriting_or_unbounded_input() {
        let dir = Scratch::new();
        let path = dir.0.join("private");
        write_new(&path, b"owned bytes").unwrap();
        assert_eq!(read(&path, 11).unwrap(), b"owned bytes");
        assert!(read(&path, 10).is_err());
        assert!(write_new(&path, b"replacement").is_err());
        assert_eq!(read(&path, 11).unwrap(), b"owned bytes");
        assert!(write_new(&dir.0.join("absent/record"), b"bytes").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn permissive_files_links_and_aliases_are_refused() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = Scratch::new();
        let path = dir.0.join("private");
        write_new(&path, b"owned bytes").unwrap();
        let alias = dir.0.join("alias");
        symlink(&path, &alias).unwrap();
        assert!(read(&alias, 20).is_err());
        let link = dir.0.join("hard");
        std::fs::hard_link(&path, &link).unwrap();
        assert!(read(&path, 20).is_err());
        std::fs::remove_file(link).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read(&path, 20).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn ordinary_inherited_acl_and_hard_links_are_refused() {
        let dir = Scratch::new();
        let ordinary = dir.0.join("ordinary");
        std::fs::write(&ordinary, b"not published privately").unwrap();
        assert!(read(&ordinary, 100).is_err());
        let private = dir.0.join("private");
        write_new(&private, b"owned bytes").unwrap();
        std::fs::hard_link(&private, dir.0.join("alias")).unwrap();
        assert!(read(&private, 20).is_err());
    }
}
