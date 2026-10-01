use crate::{StatusCode, Url};
use std::{error, fmt};

#[derive(Debug)]
pub struct Error {
    message: String,
    source: Option<Box<dyn error::Error + Send + Sync>>,
    url: Option<Url>,
    status: Option<StatusCode>,
    kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Builder,
    Request,
    Connect,
    Body,
    Redirect,
    Timeout,
    Status,
    Decode,
}

impl Error {
    pub(crate) fn new(kind: Kind, message: impl Into<String>) -> Self {
        Self { message: message.into(), source: None, url: None, status: None, kind }
    }
    pub(crate) fn host(error: pnpm_wasm_host::HostError) -> Self {
        let kind = if matches!(
            error.code.as_deref(),
            Some("ETIMEDOUT" | "ESOCKETTIMEDOUT" | "UND_ERR_CONNECT_TIMEOUT")
        ) || error.message.contains("timed out")
        {
            Kind::Timeout
        } else if matches!(error.code.as_deref(), Some("ECONNREFUSED" | "ENOTFOUND" | "EAI_AGAIN"))
        {
            Kind::Connect
        } else {
            Kind::Request
        };
        Self { source: Some(Box::new(error)), ..Self::new(kind, "WASM HTTP host operation failed") }
    }
    pub(crate) fn status_error(status: StatusCode, url: Url) -> Self {
        Self {
            status: Some(status),
            url: Some(url),
            ..Self::new(Kind::Status, format!("HTTP status {status}"))
        }
    }
    pub(crate) fn body(error: pnpm_wasm_host::HostError) -> Self {
        let mut error = Self::host(error);
        if error.kind == Kind::Request {
            error.kind = Kind::Body;
        }
        error
    }
    #[must_use]
    pub fn is_builder(&self) -> bool {
        self.kind == Kind::Builder
    }
    #[must_use]
    pub fn is_timeout(&self) -> bool {
        self.kind == Kind::Timeout
    }
    #[must_use]
    pub fn is_redirect(&self) -> bool {
        self.kind == Kind::Redirect
    }
    #[must_use]
    pub fn is_connect(&self) -> bool {
        self.kind == Kind::Connect
    }
    #[must_use]
    pub fn is_request(&self) -> bool {
        self.kind == Kind::Request
    }
    #[must_use]
    pub fn is_body(&self) -> bool {
        self.kind == Kind::Body
    }
    #[must_use]
    pub fn is_decode(&self) -> bool {
        self.kind == Kind::Decode
    }
    #[must_use]
    pub fn is_status(&self) -> bool {
        self.status.is_some()
    }
    #[must_use]
    pub fn status(&self) -> Option<StatusCode> {
        self.status
    }
    #[must_use]
    pub fn url(&self) -> Option<&Url> {
        self.url.as_ref()
    }
    pub fn url_mut(&mut self) -> Option<&mut Url> {
        self.url.as_mut()
    }
    #[must_use]
    pub fn without_url(mut self) -> Self {
        self.url = None;
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as _)
    }
}
