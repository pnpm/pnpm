use std::{
    collections::HashMap,
    io,
    process::{Child, Command},
    sync::Mutex,
};
use tokio::sync::watch;

#[cfg(unix)]
use std::{io::Read, os::unix::process::CommandExt, process::Stdio, time::Duration};

/// Tracks the processes started by one recursive command so a bailing task
/// can stop the other work that is still in flight.
#[derive(Default)]
pub struct ProcessTracker {
    state: Mutex<TrackerState>,
}

#[derive(Default)]
struct TrackerState {
    cancelled: bool,
    next_id: usize,
    executions: HashMap<usize, RunningExecution>,
}

#[derive(Clone)]
enum RunningExecution {
    Process(u32),
    Emulated(watch::Sender<bool>),
}

impl RunningExecution {
    fn cancel(&self) {
        match self {
            Self::Process(pid) => terminate_process_tree(*pid),
            Self::Emulated(sender) => {
                let _ = sender.send(true);
            }
        }
    }
}

impl ProcessTracker {
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
            terminate_process(pid);
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

/// Spawn a child and optionally register it for recursive-command
/// cancellation. On Unix a tracked child starts a process group so
/// cancellation also reaches descendants.
pub fn spawn_child<'tracker>(
    command: &mut Command,
    process_tracker: Option<&'tracker ProcessTracker>,
) -> io::Result<SpawnedChild<'tracker>> {
    if process_tracker.is_some() {
        prepare_command(command);
    }
    let child = command.spawn()?;
    let registration =
        process_tracker.map(|tracker| tracker.register(RunningExecution::Process(child.id())));
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
fn terminate_process_tree(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else { return };
    // SAFETY: a negative PID asks `kill` to signal the process group whose
    // ID is the tracked child's PID. The child was placed in that group by
    // `prepare_command`; ESRCH is harmless when it exited concurrently.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

#[cfg(unix)]
fn terminate_process(pid: i32) {
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
fn terminate_process_tree(pid: u32) {
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
