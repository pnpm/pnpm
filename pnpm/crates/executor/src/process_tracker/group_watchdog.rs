//! A guard against a child's process group outliving pnpm.
//!
//! A child that leads a process group of its own is out of reach of
//! whatever signals pnpm's group. A `SIGKILL` aimed at that group, which
//! is how Playwright's `webServer` stops the command it started, ends pnpm
//! and leaves the script running, holding the caller's pipes open
//! ([pnpm/pnpm#15555](https://github.com/pnpm/pnpm/issues/15555)). The
//! signal cannot be relayed, so a `sh` in a group of its own stands watch
//! instead: it reads a pipe only pnpm writes to, and end of input before
//! pnpm has released it means pnpm died with the group still running. The
//! watchdog then kills the group, as the signal would have had the child
//! stayed in pnpm's group.
//!
//! Windows needs none of this: the Job Object in [`crate::job_control`]
//! ends the children when pnpm does.

use std::{
    io::{self, Write},
    os::unix::process::CommandExt,
    process::{Child, ChildStdin, Command, Stdio},
};

/// What the watchdog runs, with the process group to kill as `$1`. A line
/// on standard input releases it; end of input without one means pnpm is
/// gone. It ignores the signals pnpm relays, so a signal sent to every
/// process pnpm started does not take it down before the group it watches.
///
/// `kill -9 -<pgid>` is the one spelling dash, bash, zsh and busybox `sh`
/// all take: dash refuses `--` after a signal given by number, and busybox
/// refuses `--` altogether.
const WATCHDOG_SCRIPT: &str = "trap '' INT TERM HUP; read -r _ || kill -9 -$1";

/// A `sh` that kills the process group led by one of pnpm's children if
/// pnpm dies before it has finished with that group.
pub(super) struct GroupWatchdog {
    process: Child,
    /// The pipe the watchdog reads. Dropping it unreleased is what tells
    /// the watchdog that pnpm is gone.
    lifeline: ChildStdin,
}

impl GroupWatchdog {
    /// Stand watch over the process group led by `leader`.
    ///
    /// Without a `sh` to run there is no watchdog, and the group is on its
    /// own. Any other failure to start one is returned.
    pub(super) fn spawn(leader: u32) -> io::Result<Option<Self>> {
        let mut command = Command::new("sh");
        command
            .args(["-c", WATCHDOG_SCRIPT, "sh"])
            .arg(leader.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // A group of its own keeps it out of a kill aimed at pnpm's.
            .process_group(0);
        let mut process = match command.spawn() {
            Ok(process) => process,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tracing::debug!(
                    target: "pacquet::process_tracker",
                    leader,
                    "no `sh` to watch the process group with",
                );
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let lifeline = process.stdin.take().expect("the watchdog's stdin is piped");
        Ok(Some(Self { process, lifeline }))
    }

    /// Tell the watchdog that pnpm is done with the group, and reap it.
    pub(super) fn release(mut self) {
        // A watchdog that died already cannot be told, and needs no telling.
        let _ = self.lifeline.write_all(b"\n");
        drop(self.lifeline);
        let _ = self.process.wait();
    }
}
