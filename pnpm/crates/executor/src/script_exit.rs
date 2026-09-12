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
