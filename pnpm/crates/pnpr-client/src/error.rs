use derive_more::{Display, Error, From};
use pnpm_lockfile_verification::VerifyError;

#[derive(Debug, Display, Error, From)]
pub enum PnprClientError {
    #[display("pnpr request failed: {_0}")]
    Http(reqwest::Error),

    #[display("pnpr server error: {}{message}", code.as_ref().map(|code| format!("{code}: ")).unwrap_or_default())]
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

pub(super) async fn check_status(
    response: reqwest::Response,
    request: &str,
) -> Result<reqwest::Response, PnprClientError> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = read_error_body(response).await
        .map_err(|error| {
            PnprClientError::server(format!(
                "{request} returned {status}: failed to read the error response: {error}",
            ))
        })?;
    let payload = serde_json::from_slice::<serde_json::Value>(&body).ok();
    let code = payload
        .as_ref()
        .and_then(|value| value.get("code"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let reason = payload
        .as_ref()
        .and_then(|value| value.get("reason"))
        .and_then(serde_json::Value::as_str);
    let message = format!(
        "{request} returned {status}: {}",
        reason
            .map(str::to_owned)
            .unwrap_or_else(|| String::from_utf8_lossy(&body).into_owned()),
    );
    Err(PnprClientError::server_with_code(message, code))
}

async fn read_error_body(response: reqwest::Response) -> Result<Vec<u8>, PnprClientError> {
    tokio::time::timeout(
        ERROR_BODY_READ_TIMEOUT,
        super::response_body_bounded(response, MAX_ERROR_BODY_SIZE),
    )
    .await
    .map_err(|error| PnprClientError::Protocol(format!("error response body timed out: {error}")))?
}

const ERROR_BODY_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Cap on the body read back from a failed request, which is quoted into
/// the error message.
const MAX_ERROR_BODY_SIZE: usize = 64 * 1024;
