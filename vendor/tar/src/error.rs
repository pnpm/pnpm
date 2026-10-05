use std::borrow::Cow;
use std::error;
use std::fmt;
use std::io::{self, Error};

#[derive(Debug)]
pub struct TarError {
    desc: Cow<'static, str>,
    io: io::Error,
}

impl TarError {
    pub fn new(desc: impl Into<Cow<'static, str>>, err: Error) -> TarError {
        TarError {
            desc: desc.into(),
            io: err,
        }
    }
}

impl error::Error for TarError {
    fn description(&self) -> &str {
        &self.desc
    }

    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.io)
    }
}

impl fmt::Display for TarError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.desc.fmt(f)
    }
}

impl From<TarError> for Error {
    fn from(t: TarError) -> Error {
        Error::new(t.io.kind(), t)
    }
}
