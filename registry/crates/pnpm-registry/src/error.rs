use axum::http::StatusCode;
use derive_more::{Display, Error, From};

#[derive(Debug, Display, Error, From)]
#[non_exhaustive]
pub enum RegistryError {
    #[display("Upstream request to {url} failed: {source}")]
    Upstream {
        url: String,
        #[error(source)]
        source: reqwest::Error,
    },

    #[display("Upstream returned status {status} for {url}")]
    UpstreamStatus {
        url: String,
        status: u16,
        #[error(not(source))]
        body: String,
    },

    #[display("Package name {name:?} is not a valid npm package name")]
    InvalidPackageName {
        #[error(not(source))]
        name: String,
    },

    #[display("Tarball filename {filename:?} is not valid for package {package:?}")]
    InvalidTarballName {
        #[error(not(source))]
        package: String,
        filename: String,
    },

    #[display("I/O error: {_0}")]
    Io(std::io::Error),

    #[display("JSON error: {_0}")]
    Json(serde_json::Error),
}

impl RegistryError {
    /// Map the error to the HTTP status the proxy should return to the
    /// client. Follows the standard gateway semantics:
    ///
    /// * `502 Bad Gateway` — upstream returned something we can't make
    ///   use of (5xx, malformed JSON, generic transport failure).
    /// * `503 Service Unavailable` — couldn't reach upstream at all
    ///   (DNS, connection refused, network unreachable). Distinct from
    ///   502 so pnpm clients see "service down" rather than "upstream
    ///   misbehaved" — both trigger the client's retry loop, but the
    ///   distinction matters for monitoring and for any future circuit
    ///   breaker.
    /// * `504 Gateway Timeout` — upstream took too long to respond.
    /// * `400 Bad Request` — client-supplied package or tarball name
    ///   wasn't usable. Not retryable.
    pub fn status_code(&self) -> StatusCode {
        match self {
            RegistryError::Upstream { source, .. } => {
                if source.is_timeout() {
                    StatusCode::GATEWAY_TIMEOUT
                } else if source.is_connect() {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::BAD_GATEWAY
                }
            }
            RegistryError::UpstreamStatus { .. } => StatusCode::BAD_GATEWAY,
            RegistryError::InvalidPackageName { .. } | RegistryError::InvalidTarballName { .. } => {
                StatusCode::BAD_REQUEST
            }
            RegistryError::Io(_) | RegistryError::Json(_) => StatusCode::BAD_GATEWAY,
        }
    }
}

pub type Result<T, E = RegistryError> = std::result::Result<T, E>;
