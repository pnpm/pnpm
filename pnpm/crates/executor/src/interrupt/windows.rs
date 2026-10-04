//! The Windows side of the interrupt relay: the console's control event
//! reaches every attached process at once, so nothing is relayed, and a
//! child that would sit on the interrupt forever is ended by pnpm.

use super::{RELAYED_INTERRUPTS, RelayEntry, visit_targets};
use std::{
    ptr,
    sync::{Once, atomic::Ordering},
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::PROCESSENTRY32W;

pub(super) fn install_handler() {
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

    pnpm_fs::install_temp_file_cleanup();
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        // SAFETY: the handler is a plain `extern "system"` function whose
        // only state is this module's atomics.
        unsafe {
            SetConsoleCtrlHandler(Some(relay_console_event), 1);
        }
    });
}

/// Report a console interrupt as handled while children are still
/// running, which is what keeps pnpm alive. The console delivers the same
/// event to every process attached to it, so the children already have
/// it and nothing needs relaying.
///
/// Returning `FALSE` passes the event on to the default handler, which
/// terminates pnpm.
unsafe extern "system" fn relay_console_event(event: u32) -> windows_sys::core::BOOL {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};

    if !matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT) {
        return 0;
    }
    super::INTERRUPTS.fetch_add(1, Ordering::Relaxed);
    let mut still_listening = false;
    visit_targets(|entry, target| {
        still_listening |= interrupt_child(entry, target);
    });
    windows_sys::core::BOOL::from(still_listening)
}

/// Handle the console's interrupt for the child holding `entry` and report
/// whether pnpm keeps waiting for it.
///
/// A child that is a `cmd` hosting a batch script answers the event with a
/// "Terminate batch job (Y/N)?" prompt once the command it ran returns, and
/// waits on the answer forever, so waiting it out would hold the terminal
/// hostage ([pnpm/pnpm#14860](https://github.com/pnpm/pnpm/issues/14860)).
/// pnpm ends such a child itself: on the first interrupt once it is a `cmd`
/// that has sat without child processes for a grace, and at once on the
/// next one whatever it is.
fn interrupt_child(entry: &RelayEntry, target: i32) -> bool {
    let Some(step) = count_interrupt(entry, target) else {
        return false;
    };
    if step >= RELAYED_INTERRUPTS {
        return false;
    }
    if step == 0 {
        end_child_once_idle(entry, target);
    } else {
        terminate_child(entry, target);
    }
    true
}

/// Count an interrupt for the child `target` names and return the count
/// before it, or `None` when the entry has turned over since the walk read
/// it. The check and the count share the entry's lock, which a release
/// takes to clear the target and a claim to reset the count, so a stale
/// event never spends a newly registered child's first interrupt.
fn count_interrupt(entry: &RelayEntry, target: i32) -> Option<usize> {
    entry.lock_handle();
    let step = (entry.target.load(Ordering::Relaxed) == target).then(|| {
        entry.relays.fetch_add(1, Ordering::Relaxed)
    });
    entry.unlock_handle();
    step
}

/// How long a child may sit without child processes of its own after the
/// console's interrupt before pnpm ends it. A script still shutting down
/// keeps its process running under the shell, however long that takes, so
/// only a shell whose command has returned and that has not exited itself
/// outlasts the grace: `cmd` waiting on its batch-termination answer.
const WINDOWS_INTERRUPT_GRACE: Duration = Duration::from_secs(1);

/// How often the child's process tree is checked during the grace.
const WINDOWS_INTERRUPT_POLL_MS: u32 = 100;

/// The exit code a process ended by `Ctrl+C` reports, so a child pnpm has
/// to end reads the same as one the console event ended on its own.
const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;

/// End `pid` once it has had no child processes for the grace, unless it
/// exits first or is not a `cmd`. Any other shell, or a script run without
/// one, may still be shutting down in its own process, and has no prompt
/// to get stuck on.
///
/// The watch runs on a handle of its own, which pins the process object,
/// so the pid it checks cannot be recycled under it and the process it
/// ends is always the child. A failed thread spawn ends the child at once
/// instead: a panic would abort the process from this non-unwinding
/// console callback, and waiting out a stuck child is the one outcome to
/// preclude.
fn end_child_once_idle(entry: &RelayEntry, pid: i32) {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};

    let Some(handle) = duplicate_child_handle(entry, pid) else {
        return;
    };
    let watch = move || {
        if is_cmd(pid as u32) {
            watch_until_idle(handle as HANDLE, pid as u32);
        }
        // SAFETY: the handle is this thread's own, and nothing uses it
        // after the watch.
        unsafe {
            CloseHandle(handle as HANDLE);
        }
    };
    if std::thread::Builder::new().spawn(watch).is_err() {
        // SAFETY: the thread never started, so the handle is still ours.
        unsafe {
            CloseHandle(handle as HANDLE);
        }
        terminate_child(entry, pid);
    }
}

/// Wait for `process` to exit, and end it once it has had no child
/// processes for the grace.
fn watch_until_idle(process: windows_sys::Win32::Foundation::HANDLE, pid: u32) {
    use windows_sys::Win32::{
        Foundation::WAIT_TIMEOUT,
        System::Threading::{TerminateProcess, WaitForSingleObject},
    };

    let mut idle_since = Instant::now();
    // SAFETY: the caller keeps `process` open, with the access both calls
    // need, for the length of the watch.
    unsafe {
        while WaitForSingleObject(process, WINDOWS_INTERRUPT_POLL_MS) == WAIT_TIMEOUT {
            if has_child_processes(pid) {
                idle_since = Instant::now();
            } else if idle_since.elapsed() >= WINDOWS_INTERRUPT_GRACE {
                TerminateProcess(process, STATUS_CONTROL_C_EXIT);
                return;
            }
        }
    }
}

/// A handle of the caller's own to the child holding `entry`, when the
/// entry still names `pid`. It is passed around as an address because a
/// raw handle cannot cross threads.
fn duplicate_child_handle(entry: &RelayEntry, pid: i32) -> Option<usize> {
    use windows_sys::Win32::{
        Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE},
        System::Threading::GetCurrentProcess,
    };

    if pid <= 0 {
        return None;
    }
    let mut duplicate: HANDLE = ptr::null_mut();
    entry.lock_handle();
    // Under the lock the entry cannot turn over, as in `terminate_child`.
    let handle = entry.handle.load(Ordering::Relaxed);
    let duplicated = entry.target.load(Ordering::Relaxed) == pid
        && !handle.is_null()
        // SAFETY: the lock keeps the source handle open for the call, and
        // `duplicate` is a stack local that outlives it.
        && unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                handle,
                GetCurrentProcess(),
                &raw mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } != 0;
    entry.unlock_handle();
    duplicated.then_some(duplicate as usize)
}

/// Whether `pid` runs `cmd.exe`. A process list that cannot be read makes
/// it not one, which leaves the child to the next interrupt.
fn is_cmd(pid: u32) -> bool {
    any_process(|process| process.th32ProcessID == pid && exe_is(&process.szExeFile, "cmd.exe"))
        .unwrap_or(false)
}

/// Whether any process names `pid` as its parent, other than the console
/// host Windows starts for a console process that has no console to share.
///
/// A process left behind by an earlier holder of the pid reads as a child
/// too, and so does everything when the process list cannot be read.
/// Either only keeps pnpm waiting, and the next interrupt still ends the
/// child.
fn has_child_processes(pid: u32) -> bool {
    any_process(|process| {
        process.th32ParentProcessID == pid && !exe_is(&process.szExeFile, "conhost.exe")
    })
    .unwrap_or(true)
}

/// Whether any running process `matches`, or `None` when the process list
/// cannot be read.
fn any_process(mut matches: impl FnMut(&PROCESSENTRY32W) -> bool) -> Option<bool> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
        },
    };

    // SAFETY: the snapshot handle is checked before use and closed below,
    // and `process` is a stack local with `dwSize` set as the API requires.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut process: PROCESSENTRY32W = std::mem::zeroed();
        process.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut more = Process32FirstW(snapshot, &raw mut process) != 0;
        while more && !found {
            found = matches(&process);
            more = Process32NextW(snapshot, &raw mut process) != 0;
        }
        CloseHandle(snapshot);
        Some(found)
    }
}

/// Whether a NUL-terminated executable name is `name`, ignoring case.
fn exe_is(exe_file: &[u16], name: &str) -> bool {
    let len = exe_file
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(exe_file.len());
    String::from_utf16_lossy(&exe_file[..len]).eq_ignore_ascii_case(name)
}

/// End `pid` when the relay entry still names it. The termination goes
/// through the handle the entry carries, never through a fresh lookup by
/// pid, so a recycled pid cannot make it hit an unrelated process.
fn terminate_child(entry: &RelayEntry, pid: i32) {
    use windows_sys::Win32::System::Threading::TerminateProcess;

    if pid <= 0 || entry.target.load(Ordering::Acquire) != pid {
        return;
    }
    entry.lock_handle();
    // Under the lock the entry cannot turn over: a release clears the
    // target and a claim retires the handle, each holding the lock.
    if entry.target.load(Ordering::Relaxed) == pid {
        let handle = entry.handle.load(Ordering::Relaxed);
        if !handle.is_null() {
            // SAFETY: the handle was opened with exactly the access
            // `TerminateProcess` needs, and it is closed only by a claim
            // holding this lock. On an already-exited child the call is a
            // harmless no-op.
            unsafe {
                TerminateProcess(handle, STATUS_CONTROL_C_EXIT);
            }
        }
    }
    entry.unlock_handle();
}

#[cfg(test)]
mod tests;
