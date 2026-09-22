use std::{
    collections::HashMap,
    io,
    process::{
        Child,
        Command,
    },
    sync::Mutex,
};
use tokio::sync::watch;

#[cfg(unix)]
use std::{
    io::Read,
    os::unix::process::CommandExt,
    process::Stdio,
    ptr,
    time::Duration,
};

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
    Process { pid: u32, separate_process_group: bool },
    Emulated(watch::Sender<bool>),
}

impl RunningExecution {
    fn cancel(&self) {
        match self {
            Self::Process { pid, separate_process_group } => {
                terminate_process(*pid, *separate_process_group);
            }
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
        let descendants = descendant_processes(std::process::id());
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
/// handle lives, so a terminal signal reaches it and pnpm waits for it.
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
    let child = command.spawn()?;
    crate::job_control::assign_child(&child);
    // Only Unix gives a child a process group of its own, and only then
    // must a relayed signal address that group rather than the child.
    let own_process_group = cfg!(unix) && separate_process_group;
    let relay = crate::interrupt::relay_to_child(child.id(), own_process_group);
    let registration = process_tracker.map(|tracker| {
        tracker.register(RunningExecution::Process {
            pid: child.id(),
            separate_process_group: own_process_group,
        })
    });
    Ok(SpawnedChild { child, own_process_group, _registration: registration, relay })
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
}

impl SpawnedChild<'_> {
    pub fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    /// Wait for the child, and after a relayed signal for its whole process
    /// group: a shell that died from the signal may have left the script it
    /// started still shutting down.
    pub fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
        let status = self.child.wait()?;
        if self.own_process_group && self.relay.relayed() {
            wait_for_process_group(self.child.id());
        }
        Ok(status)
    }
}

/// Block until no process of the group led by `leader` is left.
///
/// Members that became pnpm's children, as they do when pnpm is a
/// container's PID 1, are reaped along the way; the others are init's to
/// reap, and disappear from the group on their own.
#[cfg(unix)]
fn wait_for_process_group(leader: u32) {
    let Ok(leader) = i32::try_from(leader) else { return };
    let group = -leader;
    loop {
        // SAFETY: `group` names the process group pnpm created for the
        // child. `waitpid` with `WNOHANG` never blocks, and `kill` with
        // signal 0 only probes; `ESRCH` says the group is empty.
        let empty = unsafe {
            while libc::waitpid(group, ptr::null_mut(), libc::WNOHANG) > 0 {}
            libc::kill(group, 0) != 0
                && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        };
        if empty {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(not(unix))]
fn wait_for_process_group(_: u32) {}

pub(crate) struct EmulatedCancellation<'tracker> {
    receiver: watch::Receiver<bool>,
    _registration: Registration<'tracker>,
}

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
fn descendant_processes(root: u32) -> Vec<i32> {
    let Some(listing) = process_listing() else {
        return Vec::new();
    };
    let children = parse_parent_child_pids(&listing);
    let mut descendants = Vec::new();
    let mut stack = vec![root];
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
    let mut child = command.spawn().ok()?;
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
fn wait_briefly(child: &mut std::process::Child) -> bool {
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

#[cfg(windows)]
fn terminate_process(pid: u32, _separate_process_group: bool) {
    use std::{
        os::windows::process::CommandExt,
        process::Stdio,
    };

    let Some(taskkill) = taskkill_path() else { return };
    let _ = Command::new(taskkill)
        .args(["/pid", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn();
}

#[cfg(windows)]
fn taskkill_path() -> Option<std::path::PathBuf> {
    use std::{
        ffi::OsString,
        os::windows::ffi::OsStringExt,
        ptr,
    };
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

#[cfg(all(test, unix))]
mod tests;
