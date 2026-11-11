//! Walking the Windows process list.
//!
//! A snapshot is the only way to ask what a process's parent is or what
//! image it runs, which is what tells a `cmd` stuck on its
//! "Terminate batch job (Y/N)?" prompt apart from a script still shutting
//! down.

use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    },
};

/// Whether the process `pid` names runs the image `image`, matched without
/// regard to case.
///
/// `None` when the process list cannot be read.
#[must_use]
pub fn runs_image(pid: u32, image: &str) -> Option<bool> {
    any_process(|entry| entry.th32ProcessID == pid && image_is(&entry.szExeFile, image))
}

/// Whether any process names `pid` as its parent, other than one running
/// `ignoring`.
///
/// `ignoring` is for the console host Windows starts for a console process
/// that has no console to share, which is a child in the list but not one
/// the caller is waiting on. `None` when the process list cannot be read.
#[must_use]
pub fn has_children(pid: u32, ignoring: &str) -> Option<bool> {
    any_process(|entry| entry.th32ParentProcessID == pid && !image_is(&entry.szExeFile, ignoring))
}

/// Whether any process in a snapshot `matches`, or `None` when the snapshot
/// cannot be taken or read.
fn any_process(mut matches: impl FnMut(&PROCESSENTRY32W) -> bool) -> Option<bool> {
    let snapshot = Snapshot::take()?;
    // SAFETY: `PROCESSENTRY32W` holds nothing but integers and a character
    // array, so an all-zero value is a valid one to read into.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    // SAFETY: the snapshot handle is open for the whole walk, owned by the
    // guard, and the pointer is to a fully initialized stack local of
    // exactly the structure the calls write, whose `dwSize` is set as they
    // require, and which outlives them.
    unsafe {
        let mut more = Process32FirstW(snapshot.0, &raw mut entry) != 0;
        while more {
            if matches(&entry) {
                return Some(true);
            }
            more = Process32NextW(snapshot.0, &raw mut entry) != 0;
        }
    }
    Some(false)
}

/// An open process snapshot, closed when dropped.
struct Snapshot(windows_sys::Win32::Foundation::HANDLE);

impl Snapshot {
    fn take() -> Option<Self> {
        // SAFETY: both arguments are taken by value, and a handle the call
        // returns is this call's to own.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        (snapshot != INVALID_HANDLE_VALUE).then_some(Self(snapshot))
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        // SAFETY: `self.0` is the handle the snapshot returned, owned by
        // this guard, and a guard is dropped once, so it closes once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Whether a NUL-terminated image name is `name`, ignoring case.
fn image_is(exe_file: &[u16], name: &str) -> bool {
    let length = exe_file
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(exe_file.len());
    String::from_utf16_lossy(&exe_file[..length]).eq_ignore_ascii_case(name)
}
