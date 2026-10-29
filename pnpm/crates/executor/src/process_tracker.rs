use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex},
};
#[cfg(unix)]
use std::{os::unix::process::CommandExt, path::Path, ptr, time::Duration};

#[cfg(not(target_family = "wasm"))]
use tokio::sync::watch;

use crate::process::{Child, Command};
#[cfg(unix)]
pub(crate) use grace::finish_cancellations;
#[cfg(unix)]
use signal::terminate_process;
#[cfg(windows)]
use windows::terminate_process;

/// Tracks the processes started by one command so a bailing task can stop
/// other work that is still in flight.
///
/// A cancellation stops each task the way the contract in
/// `PROCESS_LIFETIME.md` sets out. On Unix the task's process group, or the
/// task and its descendants, get `SIGTERM`, and whatever is still running
/// after a grace of two seconds gets `SIGKILL`. Windows has no signal that
/// reaches a task without also reaching pnpm's console, so the Job Object
/// holding the task is terminated at once.
pub struct ProcessTracker {
    state: Arc<Mutex<TrackerState>>,
    separate_process_groups: bool,
}

impl Default for ProcessTracker {
    fn default() -> Self {
        Self { state: Arc::default(), separate_process_groups: true }
    }
}

#[derive(Default)]
struct TrackerState {
    cancelled: bool,
    next_id: usize,
    executions: HashMap<usize, RunningExecution>,
}

#[derive(Clone)]
enum RunningExecution {
    Process {
        pid: u32,
        separate_process_group: bool,
        #[cfg(windows)]
        job: Option<crate::job_control::TaskJob>,
    },
    /// The process group of a Unix child pnpm has reaped while it waits for
    /// the rest of the group. The leader's pid is free for reuse, so only the
    /// group is ever signalled, and only while it still has a member, which
    /// keeps its id from being reused.
    #[cfg(unix)]
    Group { leader: u32 },
    #[cfg(not(target_family = "wasm"))]
    Emulated(watch::Sender<bool>),
}

impl RunningExecution {
    /// Ask the execution to stop: `SIGTERM` for a Unix process. Windows and
    /// WebAssembly have nothing gentler than [`Self::kill`].
    fn stop(&self) {
        self.end(Stop::Graceful);
    }

    /// End the execution without giving it a chance to clean up.
    fn kill(&self) {
        self.end(Stop::Forced);
    }

    fn end(&self, stop: Stop) {
        match self {
            #[cfg(windows)]
            Self::Process { job: Some(job), .. } => {
                let _ = stop;
                job.terminate();
            }
            Self::Process { pid, separate_process_group, .. } => {
                terminate_process(*pid, *separate_process_group, stop);
            }
            #[cfg(unix)]
            Self::Group { leader } => signal::signal_group(*leader, stop),
            #[cfg(not(target_family = "wasm"))]
            Self::Emulated(sender) => {
                let _ = sender.send(true);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Stop {
    Graceful,
    Forced,
}

impl ProcessTracker {
    /// Track children without giving them a process group of their own.
    /// On Unix they stay in the terminal's foreground group, so terminal
    /// signals reach them directly and a child reading the terminal is
    /// not stopped as a background job; cancellation in exchange reaches
    /// each child and its scanned descendants, not a group at once. Only
    /// Unix spawns into a separate group, so on other platforms this
    /// matches [`ProcessTracker::default`]. Without a controlling
    /// terminal there is no foreground group to stay in, and every child
    /// gets its own group regardless (see [`spawn_child`]).
    #[must_use]
    pub fn foreground() -> Self {
        Self { state: Arc::default(), separate_process_groups: false }
    }

    /// Cancel every registered execution. Returns `true` only to the caller
    /// that initiated cancellation.
    ///
    /// On Unix every process of the executions gets `SIGTERM` now, and a
    /// thread of its own kills those still running once the grace has
    /// passed, so the caller does not wait it out. pnpm waits for that thread
    /// before it exits (see [`crate::before_exit`]).
    #[expect(
        clippy::must_use_candidate,
        reason = "clippy does not see the state's interior mutability through its `Arc`; most callers cancel without needing to know whether they were first"
    )]
    pub fn cancel(&self) -> bool {
        let executions = {
            let mut state = self.state.lock().expect("process tracker lock is not poisoned");
            if state.cancelled {
                return false;
            }
            state.cancelled = true;
            state.executions
                .iter()
                .map(|(&id, execution)| (id, execution.clone()))
                .collect::<Vec<_>>()
        };
        #[cfg(unix)]
        grace::stop_gracefully(&self.state, executions);
        #[cfg(not(unix))]
        for (_, execution) in executions {
            execution.stop();
        }
        true
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state.lock().expect("process tracker lock is not poisoned").cancelled
    }

    #[cfg(not(target_family = "wasm"))]
    pub(crate) fn track_emulated(&self) -> EmulatedCancellation<'_> {
        let (sender, receiver) = watch::channel(false);
        let registration = self.register(RunningExecution::Emulated(sender));
        EmulatedCancellation { receiver, _registration: registration }
    }

    fn register(&self, execution: RunningExecution) -> Registration<'_> {
        let mut state = self.state.lock().expect("process tracker lock is not poisoned");
        if state.cancelled {
            // A task that starts after the cancellation has done no work yet
            // that a graceful stop would save.
            drop(state);
            execution.kill();
            return Registration { tracker: self, id: None };
        }
        let id = state.next_id;
        state.next_id += 1;
        state.executions.insert(id, execution);
        Registration { tracker: self, id: Some(id) }
    }
}

/// Spawn a child and optionally register it for cancellation. The default
/// tracker gives each Unix child its own process group; a foreground tracker
/// preserves the caller's process group and discovers descendants at cancel.
/// Without a controlling terminal a Unix child gets its own group either
/// way: a relayed signal then reaches the script behind a shell that would
/// not pass it on, and [`SpawnedChild::wait`] outlasts that shell.
///
/// Every child also joins the interrupt relay for as long as the returned
/// handle lives, so a terminal signal reaches it and pnpm waits for it. A
/// Unix child with a group of its own is watched as well, so the group ends
/// with pnpm should pnpm die before the child.
pub fn spawn_child<'tracker>(
    command: &mut Command,
    process_tracker: Option<&'tracker ProcessTracker>,
) -> io::Result<SpawnedChild<'tracker>> {
    let separate_process_group =
        process_tracker.is_some_and(|tracker| tracker.separate_process_groups)
            || spawns_without_terminal();
    if separate_process_group {
        prepare_command(command);
    }
    let child = spawn(command)?;
    crate::job_control::assign_child(&child);
    // A task a bail may cancel gets a job of its own, so the cancellation
    // reaches everything the task started.
    #[cfg(windows)]
    let job = process_tracker.and_then(|_| crate::job_control::TaskJob::contain(&child));
    // Only Unix gives a child a process group of its own, and only then
    // must a relayed signal address that group rather than the child.
    let own_process_group = cfg!(unix) && separate_process_group;
    #[cfg(unix)]
    let (child, watched) = watch_process_group(child, own_process_group)?;
    let relay = crate::interrupt::relay_to_child(child.id(), own_process_group);
    let registration = process_tracker.map(|tracker| {
        tracker.register(RunningExecution::Process {
            pid: child.id(),
            separate_process_group: own_process_group,
            #[cfg(windows)]
            job,
        })
    });
    Ok(SpawnedChild {
        child,
        own_process_group,
        registration,
        relay,
        #[cfg(unix)]
        watched,
    })
}

/// Have the watchdog watch `child`'s group when the child leads one.
/// Returns whether the group is watched.
///
/// A child left unwatched would be the very orphan the watchdog exists to
/// prevent, so if the watch cannot be set up the child's group is killed
/// before the failure is returned.
#[cfg(unix)]
fn watch_process_group(mut child: Child, own_process_group: bool) -> io::Result<(Child, bool)> {
    if !own_process_group {
        return Ok((child, false));
    }
    match group_watchdog::watch(child.id()) {
        Ok(watched) => Ok((child, watched)),
        Err(error) => {
            terminate_process(child.id(), true, Stop::Forced);
            let _ = child.wait();
            Err(error)
        }
    }
}

/// Spawn `command` so that it cannot race the start of the group watchdog.
/// Every Unix spawn of this crate but the watchdog's goes through here.
pub(crate) fn spawn(command: &mut Command) -> io::Result<Child> {
    #[cfg(unix)]
    let _spawning = group_watchdog::spawning();
    command.spawn()
}

#[cfg(unix)]
fn spawns_without_terminal() -> bool {
    !crate::interrupt::has_controlling_terminal()
}

#[cfg(not(unix))]
fn spawns_without_terminal() -> bool {
    false
}

pub struct SpawnedChild<'tracker> {
    child: Child,
    own_process_group: bool,
    registration: Option<Registration<'tracker>>,
    relay: crate::interrupt::SignalRelay,
    #[cfg(unix)]
    watched: bool,
}

impl SpawnedChild<'_> {
    pub fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    /// Wait for the child, and after a relayed signal for its whole process
    /// group: a shell that died from the signal may have left the script it
    /// started still shutting down. pnpm then stops watching the group, so
    /// whatever the child left running in it is not ended by pnpm's own
    /// exit.
    pub fn wait(&mut self) -> io::Result<crate::process::ExitStatus> {
        let status = self.child.wait()?;
        // The pid is free for reuse from here on. A bail may still stop the
        // rest of the child's group, which the wait below sits on, but no
        // longer anything by the child's pid.
        if self.own_process_group && self.relay.relayed() {
            #[cfg(unix)]
            if let Some(registration) = &self.registration {
                registration.keep_group_only();
            }
            wait_for_process_group(self.child.id());
        }
        self.registration = None;
        #[cfg(unix)]
        if std::mem::take(&mut self.watched) {
            group_watchdog::release(self.child.id());
        }
        Ok(status)
    }
}

/// Block until no process of the group led by `leader` is still running.
#[cfg(unix)]
fn wait_for_process_group(leader: u32) {
    let Ok(leader) = i32::try_from(leader) else { return };
    let table = cfg!(target_os = "linux").then(|| Path::new("/proc"));
    while group_is_running(leader, table) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(not(unix))]
fn wait_for_process_group(_: u32) {}

/// Whether the group led by `leader` may still hold a process that has not
/// exited, once the members that are pnpm's own children are reaped.
///
/// A member that another process adopted stays in the group as a zombie
/// until that process reaps it, and the kernel still counts it. When pnpm
/// is a container's PID 1, the outer pnpm of a nested `pnpm run` adopts the
/// script while it waits for the inner pnpm to exit. `table`, laid out as
/// Linux lays out `/proc`, tells such zombies apart from the members that
/// are still shutting down.
#[cfg(unix)]
fn group_is_running(leader: i32, table: Option<&Path>) -> bool {
    let group = -leader;
    // SAFETY: `group` names the process group pnpm created for the
    // child. `waitpid` with `WNOHANG` never blocks, and `kill` with
    // signal 0 only probes; `ESRCH` says the group is empty.
    let empty = unsafe {
        while libc::waitpid(group, ptr::null_mut(), libc::WNOHANG) > 0 {}
        libc::kill(group, 0) != 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    };
    !empty && table.is_none_or(|table| process_table::has_running_member(table, leader))
}

#[cfg(not(target_family = "wasm"))]
pub(crate) struct EmulatedCancellation<'tracker> {
    receiver: watch::Receiver<bool>,
    _registration: Registration<'tracker>,
}

#[cfg(not(target_family = "wasm"))]
impl EmulatedCancellation<'_> {
    pub(crate) fn to_receiver(&self) -> watch::Receiver<bool> {
        self.receiver.clone()
    }
}

struct Registration<'tracker> {
    tracker: &'tracker ProcessTracker,
    id: Option<usize>,
}

impl Registration<'_> {
    /// Turn the registered child, which its waiter has just reaped, into the
    /// group it led (see [`RunningExecution::Group`]).
    #[cfg(unix)]
    fn keep_group_only(&self) {
        let Some(id) = self.id else { return };
        let mut state = self.tracker.state.lock().expect("process tracker lock is not poisoned");
        if let Some(execution) = state.executions.get_mut(&id)
            && let RunningExecution::Process { pid, separate_process_group: true } = *execution
        {
            *execution = RunningExecution::Group { leader: pid };
        }
    }
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        let Some(id) = self.id else { return };
        self.tracker.state
            .lock()
            .expect("process tracker lock is not poisoned")
            .executions
            .remove(&id);
    }
}

#[cfg(unix)]
fn prepare_command(command: &mut Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
fn prepare_command(_: &mut Command) {}

#[cfg(target_family = "wasm")]
fn terminate_process(pid: u32, _separate_process_group: bool, _stop: Stop) {
    if let Err(error) = pnpm_process::kill(pid) {
        eprintln!("Failed to terminate child process {pid}: {error}");
    }
}

#[cfg(not(unix))]
pub(crate) fn finish_cancellations() {}

#[cfg(unix)]
mod grace;
#[cfg(unix)]
mod group_watchdog;
#[cfg(unix)]
mod process_table;
#[cfg(unix)]
mod signal;
#[cfg(windows)]
mod windows;

#[cfg(all(test, unix))]
mod tests;
