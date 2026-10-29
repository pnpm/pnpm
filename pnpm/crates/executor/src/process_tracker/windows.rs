//! The fallback for a Windows task whose
//! [`TaskJob`](crate::job_control::TaskJob) could not be made.

use super::Stop;
use crate::process::{Command, Stdio};

/// End `pid` with `taskkill`, which reaches only the descendants whose
/// parent links still lead to it.
pub(super) fn terminate_process(pid: u32, _separate_process_group: bool, _stop: Stop) {
    use std::os::windows::process::CommandExt;

    let Some(taskkill) = taskkill_path() else { return };
    let _ = Command::new(taskkill)
        .args(["/pid", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn();
}

fn taskkill_path() -> Option<std::path::PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, ptr};

    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    // SAFETY: the first call requests the required UTF-16 buffer length.
    // The second call writes into a buffer of that size, and its returned
    // length is checked before constructing the path.
    unsafe {
        let required = GetSystemDirectoryW(ptr::null_mut(), 0);
        if required == 0 {
            return None;
        }
        let mut buffer = vec![0_u16; required as usize];
        let length = GetSystemDirectoryW(buffer.as_mut_ptr(), required);
        if length == 0 || length >= required {
            return None;
        }
        Some(
            std::path::PathBuf::from(OsString::from_wide(&buffer[..length as usize]))
                .join("taskkill.exe"),
        )
    }
}
