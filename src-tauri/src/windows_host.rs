//! Native host facts shared by startup, diagnostics and disk admission.
//! Failures remain errors; a Unix command or public temporary directory is not
//! a Windows fallback.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use windows_sys::core::GUID;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_Profile, SHGetKnownFolderPath,
};

fn known_folder(id: &GUID) -> io::Result<PathBuf> {
    let mut raw = std::ptr::null_mut();
    // The shell allocates this buffer, including on some failure paths.
    let result = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut raw) };
    let path = if result >= 0 && !raw.is_null() {
        let mut len = 0;
        unsafe {
            while *raw.add(len) != 0 {
                len += 1;
            }
            Some(PathBuf::from(OsString::from_wide(
                std::slice::from_raw_parts(raw, len),
            )))
        }
    } else {
        None
    };
    unsafe { CoTaskMemFree(raw.cast()) };
    path.filter(|p| p.is_absolute()).ok_or_else(|| {
        io::Error::other("Windows could not resolve the current user's data directory")
    })
}

pub fn app_data_dir() -> io::Result<PathBuf> {
    Ok(known_folder(&FOLDERID_LocalAppData)?.join("ColimaUI"))
}

pub fn profile_dir() -> io::Result<PathBuf> {
    known_folder(&FOLDERID_Profile)
}

/// Available bytes respect the caller's quota, rather than total free space.
pub fn disk_space(path: &Path) -> io::Result<(u64, u64)> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide = wide_path(path)?;
    let (mut available, mut total) = (0, 0);
    if unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            &mut total,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok((available, total))
}

pub fn total_memory() -> io::Result<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(status.ullTotalPhys)
}

pub fn cpu_cores() -> io::Result<u32> {
    use windows_sys::Win32::System::Threading::GetActiveProcessorCount;
    let count = unsafe { GetActiveProcessorCount(u16::MAX) };
    if count == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(count)
    }
}

pub(crate) fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Path contains a null character",
        ));
    }
    wide.push(0);
    Ok(wide)
}

#[cfg(test)]
mod tests {
    use super::*;

    // TC-WINUI-001/014: HOME is irrelevant to both native consumers; never cwd.
    #[test]
    fn native_data_location_is_absolute_and_shared() {
        let expected = known_folder(&FOLDERID_LocalAppData)
            .unwrap()
            .join("ColimaUI");
        assert_eq!(crate::path_util::try_app_data_dir().unwrap(), expected);
        assert!(expected.is_absolute());
    }

    #[test]
    fn host_facts_are_real_and_unknown_volumes_are_errors() {
        assert!(total_memory().unwrap() > 0);
        assert!(cpu_cores().unwrap() > 0);
        let data = app_data_dir().unwrap();
        let (free, total) = disk_space(data.parent().unwrap()).unwrap();
        assert!(total > 0 && free <= total);
        assert!(disk_space(&data.join(format!("missing-{}", std::process::id()))).is_err());
    }
}
