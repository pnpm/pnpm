use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex},
};
#[cfg(unix)]
use std::{
    io::Read, os::unix::process::CommandExt, path::Path, process::Stdio, ptr, time::Duration,
};

#[cfg(not(target_family = "wasm"))]
use tokio::sync::watch;

use crate::process::{Child, Command};

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

/// How long a cancelled task has to stop on `SIGTERM` before it is killed.
/// Other task runners force the stop after half a second (turborepo), two
/// seconds (moon), or five (Nx).
#[cfg(unix)]
const CANCEL_GRACE: Duration = Duration::from_secs(2);

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
        let (roots, descendants) = {
            let roots: Vec<Root> = executions
                .iter()
                .filter_map(|(id, execution)| {
                    execution
                        .signal_target()
                        .map(|target| Root { id: *id, target })
                })
                .collect();
            let descendants = descendant_processes(&root_pids(&roots));
            (roots, descendants)
        };
        for (_, execution) in executions {
            execution.stop();
        }
        #[cfg(unix)]
        {
            // A second `SIGTERM` reads as "quit now" to many tools, so a
            // descendant the signal to its group reached is not sent another.
            let targets: Vec<i32> = roots
                .iter()
                .map(|root| root.target)
                .collect();
            for &pid in &descendants {
                if !in_signalled_group(pid, &targets) {
                    signal(pid, Stop::Graceful);
                }
            }
            kill_after_grace(&self.state, roots);
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
    // A child that joins a job by hand is created suspended and resumed once
    // it is in, so nothing it starts in between escapes the job.
    #[cfg(windows)]
    let suspended = process_tracker.is_some() || crate::job_control::assigns_children();
    #[cfg(windows)]
    if suspended {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;
        command.creation_flags(CREATE_SUSPENDED);
    }
    let spawned = spawn(command);
    #[cfg(windows)]
    if suspended {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0);
    }
    let child = spawned?;
    crate::job_control::assign_child(&child);
    // A task a bail may cancel gets a job of its own, so the cancellation
    // reaches everything the task started.
    #[cfg(windows)]
    let job = process_tracker.and_then(|_| crate::job_control::TaskJob::contain(&child));
    #[cfg(windows)]
    let child = if suspended { resume_suspended(child)? } else { child };
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
        _registration: registration,
        relay,
        #[cfg(unix)]
        watched,
    })
}

/// Resume a child spawned suspended. One that cannot be resumed would never
/// run, so it is ended and the failure returned instead.
#[cfg(windows)]
fn resume_suspended(mut child: Child) -> io::Result<Child> {
    if crate::job_control::resume(&child) {
        return Ok(child);
    }
    let error = io::Error::last_os_error();
    let _ = child.kill();
    let _ = child.wait();
    Err(error)
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
    /// started still shutting down. pnpm then stops watching the group, so
    /// whatever the child left running in it is not ended by pnpm's own
    /// exit.
    pub fn wait(&mut self) -> io::Result<crate::process::ExitStatus> {
        let status = self.child.wait()?;
        if self.own_process_group && self.relay.relayed() {
            wait_for_process_group(self.child.id());
        }
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
impl Stop {
    fn signal(self) -> libc::c_int {
        match self {
            Self::Graceful => libc::SIGTERM,
            Self::Forced => libc::SIGKILL,
        }
    }
}

#[cfg(unix)]
impl RunningExecution {
    /// What a signal for this execution addresses: the process group the
    /// child leads, or the child alone.
    fn signal_target(&self) -> Option<i32> {
        match self {
            Self::Process { pid, separate_process_group } => {
                let pid = i32::try_from(*pid).ok()?;
                Some(if *separate_process_group { -pid } else { pid })
            }
            Self::Emulated(_) => None,
        }
    }
}

#[cfg(unix)]
fn terminate_process(pid: u32, separate_process_group: bool, stop: Stop) {
    let execution = RunningExecution::Process { pid, separate_process_group };
    if let Some(target) = execution.signal_target() {
        signal(target, stop);
    }
}

/// Signal `target`: a negative target signals the process group created by
/// `prepare_command`, a positive one a child or a descendant pnpm listed.
#[cfg(unix)]
fn signal(target: i32, stop: Stop) {
    // SAFETY: plain `kill`; ESRCH is harmless when the target exited
    // concurrently.
    unsafe {
        libc::kill(target, stop.signal());
    }
}

/// Whether `pid` is in a process group that one of `targets` signals.
#[cfg(unix)]
fn in_signalled_group(pid: i32, targets: &[i32]) -> bool {
    // SAFETY: `getpgid` only reads the process table, and fails with -1 for a
    // process that has exited.
    let group = unsafe { libc::getpgid(pid) };
    group > 0 && targets.contains(&-group)
}

/// A cancelled execution: the tracker's id for it, and what a signal for it
/// addresses (see [`RunningExecution::signal_target`]).
#[cfg(unix)]
#[derive(Clone, Copy)]
struct Root {
    id: usize,
    target: i32,
}

/// The pids of `roots`, group leaders included.
#[cfg(unix)]
fn root_pids(roots: &[Root]) -> Vec<u32> {
    roots
        .iter()
        .map(|root| root.target.unsigned_abs())
        .collect()
}

/// Whether the process group `target` names (negative) still has a member.
/// While it does, its id cannot be handed to another process or group, so
/// a signal to it reaches only the group pnpm created.
#[cfg(unix)]
fn group_has_member(target: i32) -> bool {
    // SAFETY: signal 0 only probes.
    unsafe { libc::kill(target, 0) == 0 }
}

/// What is left of the cancelled roots, as far as its identity is still
/// pinned: groups that still have a member, and roots their waiter has not
/// reaped, which therefore still hold their pid.
#[cfg(unix)]
struct Survivors {
    groups: Vec<i32>,
    unreaped: Vec<u32>,
}

#[cfg(unix)]
impl Survivors {
    fn find(state: &Mutex<TrackerState>, roots: &[Root]) -> Self {
        let registered: Vec<Root> = {
            let state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            roots
                .iter()
                .filter(|root| state.executions.contains_key(&root.id))
                .copied()
                .collect()
        };
        Self {
            groups: roots
                .iter()
                .map(|root| root.target)
                .filter(|&target| target < 0 && group_has_member(target))
                .collect(),
            unreaped: root_pids(&registered),
        }
    }

    fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.unreaped.is_empty()
    }

    /// Kill the survivors and the descendants of the unreaped roots, as a
    /// scan finds them now rather than as they were at the cancellation, so
    /// no pid recycled in between is hit.
    fn kill(&self) {
        let descendants = descendant_processes(&self.unreaped);
        for &group in &self.groups {
            if group_has_member(group) {
                signal(group, Stop::Forced);
            }
        }
        for &pid in &self.unreaped {
            if let Ok(pid) = i32::try_from(pid) {
                signal(pid, Stop::Forced);
            }
        }
        for pid in descendants {
            signal(pid, Stop::Forced);
        }
    }
}

/// The threads waiting out the grace of a cancellation, for
/// [`finish_cancellations`] to join.
#[cfg(unix)]
static GRACE_WATCHES: Mutex<Vec<std::thread::JoinHandle<()>>> = Mutex::new(Vec::new());

/// Kill what is left of `roots` once [`CANCEL_GRACE`] has passed, from a
/// thread of its own that ends as soon as nothing is left. A thread that
/// cannot be started kills them at once instead: waiting on a task that
/// ignores `SIGTERM` would never end.
///
/// Only what is still provably the cancelled task is killed (see
/// [`Survivors`]), since a pid recorded two seconds earlier may name another
/// process by then. A descendant that left the task's group, and whose
/// parent exited during the grace, is no longer found and keeps running.
#[cfg(unix)]
fn kill_after_grace(state: &Arc<Mutex<TrackerState>>, roots: Vec<Root>) {
    if roots.is_empty() {
        return;
    }
    let deadline = std::time::Instant::now() + CANCEL_GRACE;
    let roots: Arc<[Root]> = roots.into();
    let watch = {
        let state = Arc::clone(state);
        let roots = Arc::clone(&roots);
        move || {
            loop {
                let survivors = Survivors::find(&state, &roots);
                if survivors.is_empty() {
                    return;
                }
                if std::time::Instant::now() >= deadline {
                    survivors.kill();
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };
    match std::thread::Builder::new().name("pnpm-cancel-grace".to_string()).spawn(watch) {
        Ok(handle) => GRACE_WATCHES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(handle),
        Err(_) => Survivors::find(state, &roots).kill(),
    }
}

/// Wait until every task a cancellation stopped has ended, killing those
/// still running when their grace runs out. pnpm calls it on its way out,
/// so a task that ignores `SIGTERM` behind a shell that did not cannot
/// outlive the grace by outliving pnpm.
#[cfg(unix)]
pub(crate) fn finish_cancellations() {
    let watches = std::mem::take(
        &mut *GRACE_WATCHES.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    for watch in watches {
        let _ = watch.join();
    }
}

#[cfg(not(unix))]
pub(crate) fn finish_cancellations() {}

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

/// Run `ps` and read its whole listing, giving up on anything that does not
/// finish promptly. A `ps` that hangs is killed and its output discarded: a
/// partial listing would name the wrong parents.
#[cfg(unix)]
fn process_listing() -> Option<String> {
    let mut command = Command::new("/bin/ps");
    command
        .args(["-A", "-o", "pid=", "-o", "ppid="])
        .stdout(Stdio::piped());
    let mut child = spawn(&mut command).ok()?;
    let mut stdout = child.stdout.take()?;
    let output = std::thread::spawn(move || {
        let mut listing = String::new();
        stdout.read_to_string(&mut listing).map(|_| listing)
    });
    let completed = wait_briefly(&mut child);
    if !completed {
        let _ = child.kill();
        let _ = child.wait();
    }
    let listing = output.join().ok()?.ok()?;
    completed.then_some(listing)
}

/// Poll a child for up to half a second, reporting whether it exited.
#[cfg(unix)]
fn wait_briefly(child: &mut crate::process::Child) -> bool {
    for _ in 0..50 {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => return false,
        }
    }
    false
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

/// The fallback for a task whose [`TaskJob`] could not be made: `taskkill`
/// reaches only the descendants whose parent links still lead to the task.
///
/// [`TaskJob`]: crate::job_control::TaskJob
#[cfg(windows)]
fn terminate_process(pid: u32, _separate_process_group: bool, _stop: Stop) {
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
fn terminate_process(pid: u32, _separate_process_group: bool, _stop: Stop) {
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
