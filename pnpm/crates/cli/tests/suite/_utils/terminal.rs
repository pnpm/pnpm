//! A terminal for the tests that need one, and none for the tests that
//! must not have one: what pacquet does with a signal depends on whether
//! it holds a controlling terminal.
#![cfg(unix)]

use std::{
    fs::File,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    process::{Child, Command, Stdio},
    ptr, thread,
};

/// A pseudo-terminal the test types into, with pacquet as its foreground
/// job.
pub struct Terminal {
    master: OwnedFd,
    slave: OwnedFd,
}

impl Terminal {
    pub fn open() -> Self {
        let mut master = 0;
        let mut slave = 0;
        // SAFETY: `openpty` writes the two descriptors into the locals and
        // takes no name, attributes or window size.
        let opened = unsafe {
            libc::openpty(
                &raw mut master,
                &raw mut slave,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        assert_eq!(opened, 0, "open a pseudo-terminal");
        // SAFETY: both descriptors were just opened and nothing else owns them.
        let terminal = unsafe {
            Self { master: OwnedFd::from_raw_fd(master), slave: OwnedFd::from_raw_fd(slave) }
        };
        terminal.drain();
        terminal
    }

    /// Read whatever pacquet and the script write, so neither blocks on a
    /// full terminal buffer. The reader ends when the terminal closes.
    fn drain(&self) {
        let mut master = File::from(self.master.try_clone().expect("clone the terminal"));
        thread::spawn(move || {
            let mut sink = [0; 4096];
            while master
                .read(&mut sink)
                .is_ok_and(|read| read > 0)
            {}
        });
    }

    /// Start `command` the way an interactive shell starts a foreground
    /// job: in a session of its own, with this terminal as its controlling
    /// terminal and its standard streams.
    pub fn spawn_foreground(&self, mut command: Command) -> Child {
        let slave = self.slave.as_raw_fd();
        let stream = || Stdio::from(self.slave.try_clone().expect("clone the terminal"));
        command
            .stdin(stream())
            .stdout(stream())
            .stderr(stream());
        // SAFETY: `setsid`, `ioctl`, `signal` and `sigprocmask` are
        // async-signal-safe, which is all a `pre_exec` hook may call.
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                // The request's type is the libc's own: `c_ulong` on glibc
                // and `c_uint` on Apple, so it is cast to whatever `ioctl`
                // takes.
                if libc::ioctl(slave, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                receive_terminal_signals()
            });
        }
        command.spawn().expect("spawn pacquet on the terminal")
    }

    /// Type `Ctrl+C`, which the terminal turns into a `SIGINT` for its
    /// whole foreground process group.
    pub fn press_ctrl_c(&self) {
        let mut master = File::from(self.master.try_clone().expect("clone the terminal"));
        master.write_all(b"\x03").expect("type into the terminal");
    }
}

/// Spawn `command` in a session without a terminal, able to receive the
/// terminal signals as `kill` would send them.
///
/// All three parts matter, and all are inherited through `exec`. The
/// test harness may run with `SIGINT` ignored, which pacquet would then
/// keep (as it must under `nohup`) and pass on to the script; it may run
/// with the signal blocked, which no change of disposition undoes; and it
/// may hold a terminal, whose foreground job pacquet would then be.
pub fn spawn_without_terminal(mut command: Command) -> Child {
    // SAFETY: `setsid`, `signal` and `sigprocmask` are async-signal-safe,
    // which is all a `pre_exec` hook between `fork` and `exec` may call.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            receive_terminal_signals()
        });
    }
    command.spawn().expect("spawn pacquet without a terminal")
}

/// Undo an ignored or blocked terminal signal inherited from the harness.
///
/// Only async-signal-safe calls, since this runs between `fork` and
/// `exec`.
fn receive_terminal_signals() -> io::Result<()> {
    // SAFETY: the set is a stack local that outlives the call.
    unsafe {
        let mut unblocked: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&raw mut unblocked);
        libc::sigprocmask(libc::SIG_SETMASK, &raw const unblocked, ptr::null_mut());
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::signal(signal, libc::SIG_DFL);
        }
    }
    Ok(())
}
