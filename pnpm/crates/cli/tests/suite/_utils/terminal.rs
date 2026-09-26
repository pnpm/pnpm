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
    ptr,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

/// How long the output may keep arriving after the command exits. A
/// descendant that outlives the command can hold the terminal open for
/// ever, and the test should fail on its assertions rather than hang.
const OUTPUT_DEADLINE: Duration = Duration::from_secs(10);

/// A pseudo-terminal the test types into, with pacquet as its foreground
/// job.
pub struct Terminal {
    master: OwnedFd,
    slave: Option<OwnedFd>,
    output: Arc<Mutex<Vec<u8>>>,
    reader_done: Option<mpsc::Receiver<()>>,
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
        let output = Arc::new(Mutex::new(Vec::new()));
        // SAFETY: both descriptors were just opened and nothing else owns them.
        let terminal = unsafe {
            Self {
                master: OwnedFd::from_raw_fd(master),
                slave: Some(OwnedFd::from_raw_fd(slave)),
                output: Arc::clone(&output),
                reader_done: None,
            }
        };
        let reader_done = terminal.drain(output);
        Self { reader_done: Some(reader_done), ..terminal }
    }

    /// Read whatever pacquet and the script write, so neither blocks on a
    /// full terminal buffer. The reader ends when the terminal closes.
    fn drain(&self, output: Arc<Mutex<Vec<u8>>>) -> mpsc::Receiver<()> {
        let mut master = File::from(self.master.try_clone().expect("clone the terminal"));
        let (done, reader_done) = mpsc::channel();
        thread::spawn(move || {
            let mut sink = [0; 4096];
            loop {
                match master.read(&mut sink) {
                    Ok(0) => break,
                    Ok(read) => output
                        .lock()
                        .expect("terminal output lock")
                        .extend_from_slice(&sink[..read]),
                    Err(_) => break,
                }
            }
            let _ = done.send(());
        });
        reader_done
    }

    /// What the terminal showed, once the command has exited. Closing the
    /// slave is what lets the reader see the end of the output; a
    /// descendant still holding it open cuts the output at
    /// [`OUTPUT_DEADLINE`].
    pub fn captured_output(&mut self) -> String {
        self.slave.take();
        if let Some(reader_done) = self.reader_done.take() {
            let _ = reader_done.recv_timeout(OUTPUT_DEADLINE);
        }
        let output = self.output.lock().expect("terminal output lock");
        String::from_utf8_lossy(&output).into_owned()
    }

    /// Start `command` the way an interactive shell starts a foreground
    /// job: in a session of its own, with this terminal as its controlling
    /// terminal and its standard streams.
    pub fn spawn_foreground(&self, mut command: Command) -> Child {
        let slave = self.slave
            .as_ref()
            .expect("the terminal is still open")
            .as_raw_fd();
        let stream = || {
            Stdio::from(
                self.slave
                    .as_ref()
                    .expect("the terminal is still open")
                    .try_clone()
                    .expect("clone the terminal"),
            )
        };
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
                // and `c_uint` on Apple, so it is converted to whatever `ioctl`
                // takes.
                #[cfg(target_vendor = "apple")]
                let request = libc::TIOCSCTTY.into();
                #[cfg(not(target_vendor = "apple"))]
                let request = libc::TIOCSCTTY;
                if libc::ioctl(slave, request, 0) < 0 {
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
