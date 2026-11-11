//! A guard against a child's process group outliving pnpm.
//!
//! A child that leads a process group of its own is out of reach of
//! whatever signals pnpm's group. A `SIGKILL` aimed at that group, which
//! is how Playwright's `webServer` stops the command it started, ends pnpm
//! and leaves the script running, holding the caller's pipes open
//! ([pnpm/pnpm#15555](https://github.com/pnpm/pnpm/issues/15555)). The
//! signal cannot be relayed, so a `sh` in a group of its own stands watch
//! instead: each group leader registers itself through a pipe before `exec`,
//! and pnpm releases the group when it is done with it. End of input means
//! pnpm died, and the watchdog kills every group still named, as the
//! signal would have had the children stayed in pnpm's group.
//!
//! One watchdog serves every group of a pnpm process. It starts before the
//! first group's leader is spawned, so a command that creates no group
//! starts no `sh`.
//!
//! Windows needs none of this: the Job Object in [`crate::job_control`]
//! ends the children when pnpm does.

use std::{
    io::{self, Write},
    process::{Child, ChildStdin, Stdio},
    sync::{Mutex, MutexGuard},
};

use crate::process::Command;

/// What the watchdog runs. A line `+<pgid>` on standard input starts a
/// watch over a group and `-<pgid>` ends one; end of input kills every
/// group still watched. It ignores the signals pnpm relays, so a signal
/// sent to every process pnpm started does not take it down before the
/// groups it watches.
///
/// `kill -9 -<pgid>` is the one spelling dash, bash, zsh and busybox `sh`
/// all take: dash refuses `--` after a signal given by number, and busybox
/// refuses `--` altogether.
///
/// `groups` keeps each watched pgid between spaces, so a release cuts one
/// out with two pattern expansions instead of a loop over every group.
const WATCHDOG_SCRIPT: &str = r#"trap '' INT TERM HUP
groups=' '
while read -r line; do
  group=${line#?}
  case $line in
    +*) groups="$groups$group " ;;
    -*)
      case $groups in
        *" $group "*) groups="${groups%% "$group" *} ${groups#* "$group" }" ;;
      esac
      ;;
  esac
done
for group in $groups; do kill -9 -"$group"; done"#;

/// The watch pnpm keeps over the process groups it creates.
static WATCH: Mutex<GroupWatch> = Mutex::new(GroupWatch::new());

/// Register the child's group before its executable can run.
pub(super) fn spawn(command: &mut Command) -> io::Result<(Child, bool)> {
    lock().spawn(command)
}

/// Stop watching the process group led by `leader`, so pnpm's own exit
/// leaves whatever still runs in it alone.
pub(super) fn release(leader: u32) {
    lock().release(leader);
}

fn lock() -> MutexGuard<'static, GroupWatch> {
    WATCH.lock().expect("group watch lock is not poisoned")
}

/// The groups under watch and the watchdog that kills them should pnpm
/// die. Dropping it closes the watchdog's pipe the way pnpm's death does.
pub(super) struct GroupWatch {
    watchdog: Option<GroupWatchdog>,
    /// The leader of every group under watch, so a watchdog started in
    /// place of one that died takes over all of them.
    leaders: Vec<u32>,
    shell_missing: bool,
}

impl GroupWatch {
    pub(super) const fn new() -> Self {
        Self { watchdog: None, leaders: Vec::new(), shell_missing: false }
    }

    /// Start or replace the watchdog.
    pub(super) fn start(&mut self) -> io::Result<bool> {
        if self.shell_missing {
            return Ok(false);
        }
        if let Some(watchdog) = &mut self.watchdog
            && watchdog.is_running()
        {
            return Ok(true);
        }
        self.start_watchdog()
    }

    pub(super) fn spawn(&mut self, command: &mut Command) -> io::Result<(Child, bool)> {
        if !self.start()? {
            return command
                .spawn()
                .map(|child| (child, false));
        }
        let watchdog = self.watchdog.as_mut().expect("started watchdog");
        let child = spawn::registered(command, watchdog)?;
        self.leaders.push(child.id());
        Ok((child, true))
    }

    /// Watch an already running group.
    #[cfg(test)]
    pub(super) fn watch(&mut self, leader: u32) -> io::Result<bool> {
        if self.shell_missing {
            return Ok(false);
        }
        self.leaders.push(leader);
        if let Some(watchdog) = &mut self.watchdog
            && watchdog.watch(leader).is_ok()
        {
            return Ok(true);
        }
        let started = self.start_watchdog();
        if !matches!(started, Ok(true)) {
            self.leaders.pop();
        }
        started
    }

    /// See [`release`].
    pub(super) fn release(&mut self, leader: u32) {
        let Some(index) = self.leaders
            .iter()
            .position(|&watched| watched == leader)
        else {
            return;
        };
        self.leaders.swap_remove(index);
        if let Some(watchdog) = &mut self.watchdog {
            // A watchdog that died already cannot be told, and needs no
            // telling: its replacement learns only the groups still watched.
            let _ = watchdog.release(leader);
        }
    }

    /// Start a watchdog over every group in [`Self::leaders`], replacing
    /// one that died.
    fn start_watchdog(&mut self) -> io::Result<bool> {
        if let Some(mut replaced) = self.watchdog.take()
            && replaced.is_running()
        {
            // pnpm still holds its pipe, so a watchdog that is only refusing
            // writes would never see end of input and could not be reaped.
            let _ = replaced.process.kill();
            let _ = replaced.process.wait();
        }
        let Some(mut watchdog) = GroupWatchdog::spawn()? else {
            self.shell_missing = true;
            return Ok(false);
        };
        for &leader in &self.leaders {
            watchdog.watch(leader)?;
        }
        self.watchdog = Some(watchdog);
        Ok(true)
    }
}

/// A `sh` that kills the process groups pnpm names to it if pnpm dies
/// before it has finished with them.
struct GroupWatchdog {
    process: Child,
    /// The pipe the watchdog reads. Its end of input is what tells the
    /// watchdog that pnpm is gone.
    lifeline: ChildStdin,
}

impl GroupWatchdog {
    /// Without a `sh` to run there is no watchdog. Any other failure to
    /// start one is returned.
    fn spawn() -> io::Result<Option<Self>> {
        let mut command = Command::new("sh");
        command
            .args(["-c", WATCHDOG_SCRIPT])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // A group of its own keeps it out of a kill aimed at pnpm's.
            .process_group(0);
        let spawned = pnpm_process::watchdog_spawning().spawn(&mut command);
        let mut process = match spawned {
            Ok(process) => process,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tracing::debug!(
                    target: "pacquet::process_tracker",
                    "no `sh` to watch process groups with",
                );
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let lifeline = process.stdin.take().expect("the watchdog's stdin is piped");
        Ok(Some(Self { process, lifeline }))
    }

    /// Whether the watchdog has not exited. One reaped elsewhere, such as
    /// under an inherited ignored `SIGCHLD`, counts as exited: its pid may
    /// name another process by now, so it must not be signalled.
    fn is_running(&mut self) -> bool {
        matches!(self.process.try_wait(), Ok(None))
    }

    fn watch(&mut self, leader: u32) -> io::Result<()> {
        self.send(&format!("+{leader}\n"))
    }

    fn release(&mut self, leader: u32) -> io::Result<()> {
        self.send(&format!("-{leader}\n"))
    }

    /// Write `line` at once: the pipe is unbuffered.
    fn send(&mut self, line: &str) -> io::Result<()> {
        self.lifeline.write_all(line.as_bytes())
    }
}

mod spawn;
