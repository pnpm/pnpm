use pacquet_diagnostics::miette::{self, Diagnostic};

/// Web-based authentication did not complete before the timeout.
///
/// Ports pnpm's [`WebAuthTimeoutError`][ts-WebAuthTimeoutError]. The
/// `code(...)` is part of the public contract (<https://pnpm.io/errors>).
/// `start_time` / `end_time` are the Unix-epoch-millisecond
/// [`Clock`](crate::Clock) readings that bracketed the poll, and `timeout`
/// is the configured budget in milliseconds — the same three numbers pnpm's
/// error carries.
///
/// [ts-WebAuthTimeoutError]: https://github.com/pnpm/pnpm/blob/a06591e349/pnpm11/network/web-auth/src/WebAuthTimeoutError.ts#L3-L15
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
#[display("Web-based authentication timed out before it could be completed")]
#[diagnostic(
    code(ERR_PNPM_WEBAUTH_TIMEOUT),
    help(
        "Re-run this command and complete the authentication step in your browser before the time \
         limit is reached"
    )
)]
pub struct WebAuthTimeoutError {
    pub end_time: u64,
    pub start_time: u64,
    pub timeout: u64,
}

impl WebAuthTimeoutError {
    #[must_use]
    pub fn new(end_time: u64, start_time: u64, timeout: u64) -> Self {
        WebAuthTimeoutError { end_time, start_time, timeout }
    }
}

#[cfg(test)]
mod tests;
