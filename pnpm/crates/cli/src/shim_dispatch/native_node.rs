//! The native `node` dispatcher: executable-name-detected, with the managed
//! executable recorded in a sibling target file.

use super::{dispatch_target, trusted_shim_settings};
use pnpm_cmd_shim::CONTEXT_AWARE_DISPATCHER_NAME;
use std::{
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};

const NODE_TARGET_FILE_NAME: &str = ".pnpm-shim-v1-node-target";

pub(crate) fn native_node_dispatcher_is_installed(global_bin_dir: &Path) -> bool {
    let dispatcher = global_bin_dir
        .join(format!("{CONTEXT_AWARE_DISPATCHER_NAME}{}", std::env::consts::EXE_SUFFIX));
    let node = global_bin_dir.join(node_executable_name());
    global_bin_dir.join(NODE_TARGET_FILE_NAME).is_file()
        && same_file::is_same_file(dispatcher, node).unwrap_or(false)
}

#[cfg(windows)]
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

pub(crate) fn install_native_node_dispatcher(
    global_bin_dir: &Path,
    global_target: &Path,
) -> io::Result<()> {
    let dispatcher = global_bin_dir
        .join(format!("{CONTEXT_AWARE_DISPATCHER_NAME}{}", std::env::consts::EXE_SUFFIX));
    let node = global_bin_dir.join(node_executable_name());
    let target_file = global_bin_dir.join(NODE_TARGET_FILE_NAME);
    pnpm_fs::write_atomic(&target_file, &encode_path(global_target))?;
    crate::executable_link::replace_executable(&dispatcher, &node)
}

pub(super) fn try_native_node_dispatch(argv: &[OsString]) -> Option<i32> {
    let executable = std::env::current_exe().ok()?;
    if !is_node_executable(executable.file_name()?) {
        return None;
    }
    let target_file = executable.parent()?.join(NODE_TARGET_FILE_NAME);
    let Some(global_target) = fs::read(&target_file).ok().and_then(|bytes| decode_path(&bytes))
    else {
        eprintln!("pnpm: cannot read the global Node.js target at {}", target_file.display());
        return Some(1);
    };
    if same_file::is_same_file(&global_target, &executable).unwrap_or(false) {
        eprintln!(
            "pnpm: the global Node.js target at {} points back at the dispatcher",
            target_file.display(),
        );
        return Some(1);
    }
    let settings = trusted_shim_settings();
    Some(dispatch_target(
        "node",
        None,
        &global_target,
        &argv[1..],
        &settings.shims,
        &settings.state_dir,
    ))
}

fn node_executable_name() -> String {
    format!("node{}", std::env::consts::EXE_SUFFIX)
}

#[cfg(windows)]
fn is_node_executable(name: &OsStr) -> bool {
    name.to_str().is_some_and(|name| name.eq_ignore_ascii_case("node.exe"))
}

#[cfg(not(windows))]
fn is_node_executable(name: &OsStr) -> bool {
    name == "node"
}

#[cfg(unix)]
fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn decode_path(bytes: &[u8]) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt as _;
    Some(PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

#[cfg(windows)]
fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt as _;
    path.as_os_str().encode_wide().flat_map(u16::to_le_bytes).collect()
}

#[cfg(windows)]
fn decode_path(bytes: &[u8]) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt as _;
    let mut chunks = bytes.chunks_exact(2);
    let path = OsString::from_wide(
        &chunks.by_ref().map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]])).collect::<Vec<_>>(),
    );
    chunks.remainder().is_empty().then(|| PathBuf::from(path))
}
