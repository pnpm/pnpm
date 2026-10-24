//! The Windows side of the interrupt relay: the console's control event
//! reaches every attached process at once, so nothing is relayed, and a
//! child that would sit on the interrupt forever is ended by pnpm.

use super::{RELAYED_INTERRUPTS, RelayEntry, visit_targets};
use pnpm_ffi_process::{ProcessHandle, Wait};
use pnpm_ffi_signal::{Handled, Interrupt};
use std::{
    sync::{Once, atomic::Ordering},
    time::{Duration, Instant},
};

pub(super) fn install_handler() {
    pnpm_fs::install_temp_file_cleanup();
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| pnpm_ffi_signal::on_console_event(relay_console_event));
}

/// Report a console interrupt as handled while children are still
/// running, which is what keeps pnpm alive. The console delivers the same
/// event to every process attached to it, so the children already have
/// it and nothing needs relaying.
///
/// Reporting it unhandled passes the event to the default handler, which
/// terminates pnpm.
fn relay_console_event(_interrupt: Interrupt) -> Handled {
    super::INTERRUPTS.fetch_add(1, Ordering::Relaxed);
    let mut still_listening = false;
    visit_targets(|entry, target| {
        still_listening |= interrupt_child(entry, target);
    });
    if still_listening { Handled::Yes } else { Handled::No }
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
    let _handle = entry.handle.lock().expect("relay handle lock is not poisoned");
    (entry.target.load(Ordering::Relaxed) == target).then(|| {
        entry.relays.fetch_add(1, Ordering::Relaxed)
    })
}

/// How long a child may sit without child processes of its own after the
/// console's interrupt before pnpm ends it. A script still shutting down
/// keeps its process running under the shell, however long that takes, so
/// only a shell whose command has returned and that has not exited itself
/// outlasts the grace: `cmd` waiting on its batch-termination answer.
const WINDOWS_INTERRUPT_GRACE: Duration = Duration::from_secs(1);

/// How often the child's process tree is checked during the grace.
const WINDOWS_INTERRUPT_POLL: Duration = Duration::from_millis(100);

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
    let Ok(pid) = u32::try_from(pid) else { return };
    let Some(handle) = duplicate_child_handle(entry, pid) else {
        return;
    };
    let watch = move || {
        if is_cmd(pid) {
            watch_until_idle(&handle, pid);
        }
    };
    if std::thread::Builder::new().spawn(watch).is_err() {
        terminate_child(entry, i32::try_from(pid).unwrap_or(0));
    }
}

/// Wait for `process` to exit, and end it once it has had no child
/// processes for the grace.
fn watch_until_idle(process: &ProcessHandle, pid: u32) {
    let mut idle_since = Instant::now();
    while process.wait(WINDOWS_INTERRUPT_POLL) == Wait::StillRunning {
        if has_child_processes(pid) {
            idle_since = Instant::now();
        } else if idle_since.elapsed() >= WINDOWS_INTERRUPT_GRACE {
            process.terminate(STATUS_CONTROL_C_EXIT);
            return;
        }
    }
}

/// A handle of the caller's own to the child holding `entry`, when the
/// entry still names `pid`. The watch runs on a thread of its own, so it
/// cannot borrow the entry's.
fn duplicate_child_handle(entry: &RelayEntry, pid: u32) -> Option<ProcessHandle> {
    let target = i32::try_from(pid).ok()?;
    // Under the lock the entry cannot turn over, as in `terminate_child`.
    let handle = entry.handle.lock().expect("relay handle lock is not poisoned");
    if entry.target.load(Ordering::Relaxed) != target {
        return None;
    }
    handle.as_ref()?.duplicate()
}

/// Whether `pid` runs `cmd.exe`. A process list that cannot be read makes
/// it not one, which leaves the child to the next interrupt.
fn is_cmd(pid: u32) -> bool {
    pnpm_ffi_process::runs_image(pid, "cmd.exe").unwrap_or(false)
}

/// Whether any process names `pid` as its parent, other than the console
/// host Windows starts for a console process that has no console to share.
///
/// A process left behind by an earlier holder of the pid reads as a child
/// too, and so does everything when the process list cannot be read.
/// Either only keeps pnpm waiting, and the next interrupt still ends the
/// child.
fn has_child_processes(pid: u32) -> bool {
    pnpm_ffi_process::has_children(pid, "conhost.exe").unwrap_or(true)
}

/// End `pid` when the relay entry still names it. The termination goes
/// through the handle the entry carries, never through a fresh lookup by
/// pid, so a recycled pid cannot make it hit an unrelated process.
fn terminate_child(entry: &RelayEntry, pid: i32) {
    if pid <= 0 || entry.target.load(Ordering::Acquire) != pid {
        return;
    }
    // Under the lock the entry cannot turn over: a release clears the
    // target and a claim retires the handle, each holding the lock.
    let handle = entry.handle.lock().expect("relay handle lock is not poisoned");
    if entry.target.load(Ordering::Relaxed) == pid
        && let Some(handle) = handle.as_ref()
    {
        handle.terminate(STATUS_CONTROL_C_EXIT);
    }
}

#[cfg(test)]
mod tests;
