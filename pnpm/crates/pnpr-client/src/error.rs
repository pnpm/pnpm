use super::{MAX_ERROR_BODY_SIZE, response_body_bounded};
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

/// Error code of a request naming a field the server does not know.
pub const UNSUPPORTED_REQUEST_FIELD_CODE: &str = "ERR_PNPR_UNSUPPORTED_REQUEST_FIELD";

/// Error code of a request accepting no lockfile format the server renders.
pub const INCOMPATIBLE_LOCKFILE_FORMAT_CODE: &str = "ERR_PNPR_INCOMPATIBLE_LOCKFILE_FORMAT";

impl PnprClientError {
    /// Whether the server refused the request because it cannot honor a
    /// setting or the lockfile format this client sent. Resolving locally
    /// produces the lockfile the server would not.
    #[must_use]
    pub fn is_unsupported_request(&self) -> bool {
        let PnprClientError::Server { code: Some(code), .. } = self else {
            return false;
        };
        [UNSUPPORTED_REQUEST_FIELD_CODE, INCOMPATIBLE_LOCKFILE_FORMAT_CODE].contains(&code.as_str())
    }
}

/// The [`PnprClientError::Server`] for a resolver endpoint's non-success
/// response, carrying the `code` of its JSON error body.
pub(crate) async fn resolver_error_response(
    endpoint: &str,
    response: reqwest::Response,
) -> PnprClientError {
    let status = response.status();
    let body = match response_body_bounded(response, MAX_ERROR_BODY_SIZE).await {
        Ok(body) => body,
        Err(err) => return err,
    };
    let code = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|payload| {
            Some(
                payload
                    .get("code")?
                    .as_str()?
                    .to_string(),
            )
        });
    PnprClientError::Server {
        message: format!("{endpoint} returned {status}: {}", String::from_utf8_lossy(&body)),
        code,
    }
}
