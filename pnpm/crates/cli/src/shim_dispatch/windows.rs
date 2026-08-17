//! The native `node.exe` dispatcher: argv0-detected, with the managed
//! executable recorded in a sibling target file.

use super::{dispatch_target, global_shims_setting};
use pnpm_cmd_shim::CONTEXT_AWARE_DISPATCHER_NAME;
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

const WINDOWS_NODE_TARGET_FILE_NAME: &str = ".pnpm-shim-v1-node-target";

pub(super) fn system_powershell_path() -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt as _;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    let mut buffer = vec![0u16; 260];
    loop {
        let buffer_len = u32::try_from(buffer.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "Windows system directory path is too long")
        })?;
        // SAFETY: `buffer` is writable for `buffer.len()` UTF-16 code units.
        let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer_len) };
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        let length = length as usize;
        if length < buffer.len() {
            buffer.truncate(length);
            return Ok(PathBuf::from(OsString::from_wide(&buffer))
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe"));
        }
        buffer.resize(length + 1, 0);
    }
}

pub(crate) fn install_windows_node_dispatcher(
    global_bin_dir: &Path,
    global_target: &Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;

    let dispatcher = global_bin_dir.join(format!("{CONTEXT_AWARE_DISPATCHER_NAME}.exe"));
    let node_exe = global_bin_dir.join("node.exe");
    let target_file = global_bin_dir.join(WINDOWS_NODE_TARGET_FILE_NAME);
    let staged_target =
        global_bin_dir.join(format!(".{WINDOWS_NODE_TARGET_FILE_NAME}.{}.tmp", std::process::id()));
    let encoded =
        global_target.as_os_str().encode_wide().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
    fs::write(&staged_target, encoded)?;
    let publish_target = crate::executable_link::replace_executable(&staged_target, &target_file);
    let _ = fs::remove_file(&staged_target);
    publish_target?;
    crate::executable_link::replace_executable(&dispatcher, &node_exe)
}

pub(super) fn try_windows_node_dispatch(argv: &[OsString]) -> Option<i32> {
    use std::os::windows::ffi::OsStringExt as _;

    let executable = std::env::current_exe().ok()?;
    if !executable
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("node.exe"))
    {
        return None;
    }
    let target_file = executable.parent()?.join(WINDOWS_NODE_TARGET_FILE_NAME);
    let global_target = match fs::read(&target_file).ok().and_then(|bytes| {
        let mut chunks = bytes.chunks_exact(2);
        let path = OsString::from_wide(
            &chunks
                .by_ref()
                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                .collect::<Vec<_>>(),
        );
        chunks.remainder().is_empty().then(|| PathBuf::from(path))
    }) {
        Some(target) => target,
        None => {
            eprintln!("pnpm: cannot read the global Node.js target at {}", target_file.display());
            return Some(1);
        }
    };
    // A target file pointing back at the dispatcher would recurse forever
    // through the no-candidate fallback.
    if same_file::is_same_file(&global_target, &executable).unwrap_or(false) {
        eprintln!(
            "pnpm: the global Node.js target at {} points back at the dispatcher",
            target_file.display(),
        );
        return Some(1);
    }
    Some(dispatch_target("node", None, &global_target, &argv[1..], &global_shims_setting()))
}
