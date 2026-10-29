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
    pnpm_ffi_fs::system_directory().map(|directory| directory.join("taskkill.exe"))
}
