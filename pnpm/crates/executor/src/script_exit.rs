use std::{fmt, process::ExitStatus};

/// How a script finished.
///
/// A script either runs as a child shell — `sh -c`, `cmd /d /s /c`, or a
/// custom `scriptShell` — or, under `shellEmulator`, inside pacquet's own
/// process, where there is no child and therefore no [`ExitStatus`].
/// Both answer the two questions every caller asks: did it succeed, and
/// with which code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptExit {
    Process(ExitStatus),
    Emulated(i32),
}

impl ScriptExit {
    #[must_use]
    pub fn success(self) -> bool {
        match self {
            Self::Process(status) => status.success(),
            Self::Emulated(code) => code == 0,
        }
    }

    /// The exit code, or `None` for a child killed by a signal before it
    /// could set one.
    #[must_use]
    pub fn code(self) -> Option<i32> {
        match self {
            Self::Process(status) => status.code(),
            Self::Emulated(code) => Some(code),
        }
    }

    /// The name of the signal that killed the child, such as `SIGKILL`.
    /// `None` for a script that exited with a code, and for a signal
    /// without a POSIX name.
    #[must_use]
    pub fn signal_name(self) -> Option<&'static str> {
        #[cfg(unix)]
        if let Self::Process(status) = self {
            use std::os::unix::process::ExitStatusExt;
            return status.signal().and_then(posix_signal_name);
        }
        None
    }
}

#[cfg(unix)]
fn posix_signal_name(signal: i32) -> Option<&'static str> {
    Some(match signal {
        libc::SIGABRT => "SIGABRT",
        libc::SIGALRM => "SIGALRM",
        libc::SIGBUS => "SIGBUS",
        libc::SIGCHLD => "SIGCHLD",
        libc::SIGCONT => "SIGCONT",
        libc::SIGFPE => "SIGFPE",
        libc::SIGHUP => "SIGHUP",
        libc::SIGILL => "SIGILL",
        libc::SIGINT => "SIGINT",
        libc::SIGKILL => "SIGKILL",
        libc::SIGPIPE => "SIGPIPE",
        libc::SIGPROF => "SIGPROF",
        libc::SIGQUIT => "SIGQUIT",
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGSTOP => "SIGSTOP",
        libc::SIGSYS => "SIGSYS",
        libc::SIGTERM => "SIGTERM",
        libc::SIGTRAP => "SIGTRAP",
        libc::SIGTSTP => "SIGTSTP",
        libc::SIGTTIN => "SIGTTIN",
        libc::SIGTTOU => "SIGTTOU",
        libc::SIGURG => "SIGURG",
        libc::SIGUSR1 => "SIGUSR1",
        libc::SIGUSR2 => "SIGUSR2",
        libc::SIGVTALRM => "SIGVTALRM",
        libc::SIGWINCH => "SIGWINCH",
        libc::SIGXCPU => "SIGXCPU",
        libc::SIGXFSZ => "SIGXFSZ",
        _ => return None,
    })
}

impl fmt::Display for ScriptExit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Process(status) => status.fmt(formatter),
            Self::Emulated(code) => write!(formatter, "exit status: {code}"),
        }
    }
}

#[cfg(test)]
mod tests;
