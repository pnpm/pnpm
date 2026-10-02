use serde::Deserialize;
use serde_json::Value;
use std::{fmt, io};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitStatus {
    code: Option<i32>,
    signal: Option<i32>,
    signal_name: Option<&'static str>,
}

impl ExitStatus {
    #[must_use]
    pub fn code(&self) -> Option<i32> {
        self.code
    }

    #[must_use]
    pub fn success(&self) -> bool {
        self.code == Some(0) && self.signal.is_none()
    }

    #[must_use]
    pub fn signal(&self) -> Option<i32> {
        self.signal
    }

    #[must_use]
    pub fn signal_name(&self) -> Option<&'static str> {
        self.signal_name
    }

    pub(crate) fn from_host(value: Value) -> io::Result<Self> {
        #[derive(Deserialize)]
        struct Status {
            code: Option<i32>,
            #[serde(rename = "signal")]
            signal_name: Option<String>,
            #[serde(rename = "signalNumber")]
            signal: Option<i32>,
        }
        let status: Status = serde_json::from_value(value).map_err(io::Error::other)?;
        let signal = status.signal;
        let signal_name = status.signal_name.as_deref().and_then(known_signal_name);
        Ok(Self { code: status.code, signal, signal_name })
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.code, self.signal) {
            (Some(code), _) => write!(formatter, "exit status: {code}"),
            (_, Some(signal)) => write!(formatter, "signal: {signal}"),
            _ => formatter.write_str("unknown process exit status"),
        }
    }
}

fn known_signal_name(name: &str) -> Option<&'static str> {
    const NAMES: &[&str] = &[
        "SIGABRT",
        "SIGALRM",
        "SIGBUS",
        "SIGCHLD",
        "SIGCONT",
        "SIGFPE",
        "SIGHUP",
        "SIGILL",
        "SIGINT",
        "SIGKILL",
        "SIGPIPE",
        "SIGPROF",
        "SIGQUIT",
        "SIGSEGV",
        "SIGSTOP",
        "SIGSYS",
        "SIGTERM",
        "SIGTRAP",
        "SIGTSTP",
        "SIGTTIN",
        "SIGTTOU",
        "SIGURG",
        "SIGUSR1",
        "SIGUSR2",
        "SIGVTALRM",
        "SIGWINCH",
        "SIGXCPU",
        "SIGXFSZ",
    ];
    NAMES
        .iter()
        .copied()
        .find(|candidate| *candidate == name)
}
