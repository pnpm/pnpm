use derive_more::{Display, Error, From};
use pnpm_lockfile_verification::VerifyError;

#[derive(Debug, Display, Error, From)]
pub enum PnprClientError {
    #[display("pnpr request failed: {_0}")]
    Http(reqwest::Error),

    #[display("pnpr server error: {message}")]
    #[from(ignore)]
    Server {
        message: String,
        code: Option<String>,
    },

    #[display("malformed server response: {_0}")]
    #[from(ignore)]
    Protocol(#[error(not(source))] String),

    /// Any failure talking to a Turborepo Remote Cache server. Callers
    /// already name the remote cache, so the message stands alone.
    #[from(ignore)]
    RemoteCache(#[error(not(source))] String),

    /// The server rejected the input lockfile under the client's
    /// verification policy. Carries the reconstructed [`VerifyError`]
    /// so the CLI aborts with the same diagnostic code (and breakdown)
    /// the local verification gate would have produced.
    Verification(VerifyError),

    Io(std::io::Error),
}

impl PnprClientError {
    pub fn server(message: impl Into<String>) -> Self {
        Self::Server { message: message.into(), code: None }
    }

    pub fn server_with_code(message: impl Into<String>, code: Option<String>) -> Self {
        Self::Server { message: message.into(), code }
    }
}
