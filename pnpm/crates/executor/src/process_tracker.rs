use std::{
    collections::HashMap,
    io,
    process::{Child, Command},
    sync::Mutex,
};
use tokio::sync::watch;

#[cfg(unix)]
use std::{io::Read, os::unix::process::CommandExt, process::Stdio, time::Duration};

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
    /// matches [`ProcessTracker::default`].
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
            state.executions.values().cloned().collect::<Vec<_>>()
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
pub fn spawn_child<'tracker>(
    command: &mut Command,
    process_tracker: Option<&'tracker ProcessTracker>,
) -> io::Result<SpawnedChild<'tracker>> {
    if process_tracker.is_some_and(|tracker| tracker.separate_process_groups) {
        prepare_command(command);
    }
    let child = command.spawn()?;
    crate::job_control::assign_child(&child);
    let registration = process_tracker.map(|tracker| {
        tracker.register(RunningExecution::Process {
            pid: child.id(),
            separate_process_group: tracker.separate_process_groups,
        })
    });
    Ok(SpawnedChild { child, _registration: registration })
}

pub struct SpawnedChild<'tracker> {
    child: Child,
    _registration: Option<Registration<'tracker>>,
}

impl SpawnedChild<'_> {
    pub fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    pub fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
        self.child.wait()
    }
}

pub(crate) struct EmulatedCancellation<'tracker> {
    receiver: watch::Receiver<bool>,
    _registration: Registration<'tracker>,
}

impl EmulatedCancellation<'_> {
    pub(crate) fn receiver(&self) -> watch::Receiver<bool> {
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
        self.tracker
            .state
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
    let mut command = Command::new("/bin/ps");
    command.args(["-A", "-o", "pid=", "-o", "ppid="]).stdout(Stdio::piped());
    let Ok(mut child) = command.spawn() else {
        return Vec::new();
    };
    let Some(mut stdout) = child.stdout.take() else {
        return Vec::new();
    };
    let output = std::thread::spawn(move || {
        let mut listing = String::new();
        stdout.read_to_string(&mut listing).map(|_| listing)
    });
    let mut completed = false;
    for _ in 0..50 {
        match child.try_wait() {
            Ok(Some(_)) => {
                completed = true;
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break,
        }
    }
    if !completed {
        let _ = child.kill();
        let _ = child.wait();
    }
    let Ok(Ok(listing)) = output.join() else {
        return Vec::new();
    };
    if !completed {
        return Vec::new();
    }

    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for line in listing.lines() {
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(parent)) = (fields.next(), fields.next()) else {
            continue;
        };
        let (Ok(pid), Ok(parent)) = (pid.parse(), parent.parse()) else {
            continue;
        };
        children.entry(parent).or_default().push(pid);
    }
    let mut descendants = Vec::new();
    let mut stack = vec![root];
    while let Some(parent) = stack.pop() {
        for &pid in children.get(&parent).into_iter().flatten() {
            stack.push(pid);
            if let Ok(pid) = i32::try_from(pid) {
                descendants.push(pid);
            }
        }
    }
    descendants
}

#[cfg(windows)]
fn terminate_process(pid: u32, _separate_process_group: bool) {
    use std::{os::windows::process::CommandExt, process::Stdio};

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

#[cfg(all(test, unix))]
mod tests;
