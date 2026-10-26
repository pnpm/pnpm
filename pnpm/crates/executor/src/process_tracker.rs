use std::{collections::HashMap, io, sync::Mutex};
#[cfg(unix)]
use std::{
    io::Read,
    os::unix::process::CommandExt,
    path::Path,
    process::Stdio,
    ptr,
    time::{Duration, Instant},
};

#[cfg(not(target_family = "wasm"))]
use tokio::sync::watch;

use crate::process::{Child, Command};

/// Tracks the processes started by one command so a bailing task can stop
/// other work that is still in flight.
pub struct ProcessTracker {
    state: Mutex<TrackerState>,
    separate_process_groups: bool,
}

impl Default for ProcessTracker {
    fn default() -> Self {
        Self { state: Mutex::new(TrackerState::default()), separate_process_groups: true }
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
    },
    #[cfg(not(target_family = "wasm"))]
    Emulated(watch::Sender<bool>),
}

impl RunningExecution {
    fn cancel(&self) {
        match self {
            Self::Process { pid, separate_process_group } => {
                terminate_process(*pid, *separate_process_group);
            }
            #[cfg(not(target_family = "wasm"))]
            Self::Emulated(sender) => {
                let _ = sender.send(true);
            }
        }
    }
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
        Self { state: Mutex::new(TrackerState::default()), separate_process_groups: false }
    }

    /// Cancel every registered execution. Returns `true` only to the caller
    /// that initiated cancellation.
    pub fn cancel(&self) -> bool {
        let executions = {
            let mut state = self.state.lock().expect("process tracker lock is not poisoned");
            if state.cancelled {
                return false;
            }
            state.cancelled = true;
            state.executions
                .values()
                .cloned()
                .collect::<Vec<_>>()
        };
        #[cfg(unix)]
        let roots: Vec<u32> = executions
            .iter()
            .filter_map(|exec| match exec {
                RunningExecution::Process { pid, .. } => Some(*pid),
                RunningExecution::Emulated(_) => None,
            })
            .collect();
        #[cfg(unix)]
        let descendants = descendant_processes(&roots);
        for execution in executions {
            execution.cancel();
        }
        #[cfg(unix)]
        for pid in descendants {
            terminate_descendant(pid);
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
            drop(state);
            execution.cancel();
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
    // The child joins the relay only once it exists, and the watchdog's
    // first `sh` widens the gap. What reaches pnpm from here on is relayed
    // by `SpawningChild::joined` rather than by the handler, which cannot
    // see a child that has not joined.
    let mut spawning = crate::interrupt::spawning_child();
    let child = command.spawn()?;
    spawning.spawned();
    crate::job_control::assign_child(&child);
    // Only Unix gives a child a process group of its own, and only then
    // must a relayed signal address that group rather than the child.
    let own_process_group = cfg!(unix) && separate_process_group;
    #[cfg(unix)]
    let (child, watched) = watch_process_group(child, own_process_group)?;
    let relay = crate::interrupt::relay_to_child(child.id(), own_process_group);
    spawning.joined(&relay);
    let registration = process_tracker.map(|tracker| {
        tracker.register(RunningExecution::Process {
            pid: child.id(),
            separate_process_group: own_process_group,
        })
    });
    Ok(SpawnedChild {
        child,
        own_process_group,
        _registration: registration,
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
            terminate_process(child.id(), true);
            let _ = child.wait();
            Err(error)
        }
    }
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
    _registration: Option<Registration<'tracker>>,
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
    /// started still shutting down. A group that drains is no longer
    /// watched, so whatever the child left running in it is not ended by
    /// pnpm's own exit; one that does not drain keeps its watch.
    pub fn wait(&mut self) -> io::Result<crate::process::ExitStatus> {
        let status = self.child.wait()?;
        if self.own_process_group
            && self.relay.relayed()
            && !wait_for_process_group(self.child.id())
        {
            return Ok(status);
        }
        #[cfg(unix)]
        if std::mem::take(&mut self.watched) {
            group_watchdog::release(self.child.id());
        }
        Ok(status)
    }
}

/// How long the process group of a signalled child is given to drain.
///
/// Every process in the group was sent the signal pnpm relayed, so one
/// still running at the end of this has declined to stop, and waiting on
/// it is waiting on something that has already refused. A container
/// runtime gives a process it stops the same order of grace, which is
/// enough for a dev server to close its connections.
#[cfg(unix)]
const PROCESS_GROUP_SHUTDOWN_GRACE: Duration = Duration::from_secs(30);

/// Block until no process of the group led by `leader` is still running,
/// and report whether the group drained within
/// [`PROCESS_GROUP_SHUTDOWN_GRACE`].
#[cfg(unix)]
fn wait_for_process_group(leader: u32) -> bool {
    let Ok(leader) = i32::try_from(leader) else { return true };
    let table = cfg!(target_os = "linux").then(|| Path::new("/proc"));
    let deadline = Instant::now() + PROCESS_GROUP_SHUTDOWN_GRACE;
    while group_is_running(leader, table) {
        if Instant::now() >= deadline {
            tracing::warn!(
                "a process the script started has not stopped, so pnpm's own exit ends it",
            );
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

#[cfg(not(unix))]
fn wait_for_process_group(_: u32) -> bool {
    true
}

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

#[cfg(unix)]
fn terminate_process(pid: u32, separate_process_group: bool) {
    let Ok(pid) = i32::try_from(pid) else { return };
    let target = if separate_process_group { -pid } else { pid };
    // SAFETY: a negative target signals the process group created by
    // `prepare_command`; a positive target signals the foreground child.
    // ESRCH is harmless when the process exited concurrently.
    unsafe {
        libc::kill(target, libc::SIGKILL);
    }
}

#[cfg(unix)]
fn terminate_descendant(pid: i32) {
    // SAFETY: every PID comes from the OS process list as a descendant of
    // this process. ESRCH is harmless when it exited concurrently.
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
}

#[cfg(unix)]
fn descendant_processes(roots: &[u32]) -> Vec<i32> {
    if roots.is_empty() {
        return Vec::new();
    }
    let Some(listing) = process_listing() else {
        return Vec::new();
    };
    let children = parse_parent_child_pids(&listing);
    let mut descendants = Vec::new();
    let mut stack = roots.to_vec();
    while let Some(parent) = stack.pop() {
        for &pid in children
            .get(&parent)
            .into_iter()
            .flatten()
        {
            stack.push(pid);
            if let Ok(pid) = i32::try_from(pid) {
                descendants.push(pid);
            }
        }
    }
    descendants
}

/// How long `ps` gets before [`process_listing`] gives up on it.
///
/// The listing is the only way cancellation reaches a descendant of a
/// child that shares pnpm's process group, so giving up on it leaves that
/// descendant running. The deadline is here to turn a `ps` that hangs
/// into a reported miss rather than a hang of pnpm's own, which is why it
/// is far longer than any listing takes: a loaded machine must not reach
/// it. Until it passes, cancellation waits, because a kill that misses
/// the scripts is worse than one that is late.
#[cfg(unix)]
const PROCESS_LISTING_DEADLINE: Duration = Duration::from_secs(10);

/// Run `ps` and read its whole listing. A `ps` that does not answer
/// before [`PROCESS_LISTING_DEADLINE`] is killed and its output
/// discarded, because a partial listing would name the wrong parents.
///
/// `None` says the listing could not be read, which is not the same as a
/// tree with nothing in it: the descendants it would have named are left
/// running, and a trace record says so.
#[cfg(unix)]
fn process_listing() -> Option<String> {
    let mut command = Command::new("/bin/ps");
    command
        .args(["-A", "-o", "pid=", "-o", "ppid="])
        .stdout(Stdio::piped());
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let output = std::thread::spawn(move || {
        let mut listing = String::new();
        stdout.read_to_string(&mut listing).map(|_| listing)
    });
    if !exits_before(&mut child, PROCESS_LISTING_DEADLINE) {
        let _ = child.kill();
        let _ = child.wait();
        // Reading the pipe is left to the thread that holds it: whatever
        // else inherited it can keep it open for as long as it lives, and
        // nothing here needs what it says any more.
        drop(output);
        tracing::warn!(
            "the process listing did not arrive, so a process a cancelled script started may still be running",
        );
        return None;
    }
    output.join().ok()?.ok()
}

/// Poll a child until it exits or `deadline` passes, reporting whether it
/// exited.
#[cfg(unix)]
fn exits_before(child: &mut crate::process::Child, deadline: Duration) -> bool {
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if started.elapsed() >= deadline => return false,
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => return false,
        }
    }
}

/// The child pids of every parent named in a `pid ppid` listing.
#[cfg(unix)]
fn parse_parent_child_pids(listing: &str) -> HashMap<u32, Vec<u32>> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for line in listing.lines() {
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(parent)) = (fields.next(), fields.next()) else {
            continue;
        };
        let (Ok(pid), Ok(parent)) = (pid.parse(), parent.parse()) else {
            continue;
        };
        children
            .entry(parent)
            .or_default()
            .push(pid);
    }
    children
}

#[cfg(windows)]
fn terminate_process(pid: u32, _separate_process_group: bool) {
    use std::os::windows::process::CommandExt;

    use crate::process::Stdio;

    let Some(taskkill) = taskkill_path() else { return };
    let _ = Command::new(taskkill)
        .args(["/pid", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn();
}

#[cfg(target_family = "wasm")]
fn terminate_process(pid: u32, _separate_process_group: bool) {
    if let Err(error) = pnpm_process::kill(pid) {
        eprintln!("Failed to terminate child process {pid}: {error}");
    }
}

#[cfg(windows)]
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

#[cfg(unix)]
mod group_watchdog;
#[cfg(unix)]
mod process_table;

#[cfg(all(test, unix))]
mod tests;
