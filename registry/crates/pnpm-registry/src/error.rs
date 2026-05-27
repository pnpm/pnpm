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

    #[display("Access rule {value:?} is not recognized (expected $all or $authenticated)")]
    #[from(skip)]
    InvalidAccessRule {
        #[error(not(source))]
        value: String,
    },

    #[display("Package policy pattern {pattern:?} is invalid: {reason}")]
    #[from(skip)]
    InvalidPolicyPattern {
        #[error(not(source))]
        pattern: String,
        reason: String,
    },

    /// Authentication is required for the requested resource but
    /// the caller supplied no credentials (or invalid ones). Maps
    /// to 401 to match npm/verdaccio.
    #[display("Authentication required for {resource}")]
    #[from(skip)]
    Unauthenticated {
        #[error(not(source))]
        resource: String,
    },

    /// Credentials were supplied but the caller isn't allowed to
    /// touch this resource. Maps to 403.
    #[display("{user:?} is not allowed to {action} {resource}")]
    #[from(skip)]
    Forbidden {
        #[error(not(source))]
        user: String,
        action: &'static str,
        resource: String,
    },

    /// Tarball payload from a publish couldn't be decoded — bad
    /// base64, length mismatch, or integrity mismatch.
    #[display("Invalid attachment {filename:?}: {reason}")]
    #[from(skip)]
    InvalidAttachment {
        #[error(not(source))]
        filename: String,
        reason: String,
    },

    /// Generic client-side error during a write operation —
    /// missing/invalid JSON body field, etc. Maps to 400.
    #[display("Bad request: {reason}")]
    #[from(skip)]
    BadRequest {
        #[error(not(source))]
        reason: String,
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
            RegistryError::InvalidPackageName { .. }
            | RegistryError::InvalidTarballName { .. }
            | RegistryError::InvalidAccessRule { .. }
            | RegistryError::InvalidPolicyPattern { .. }
            | RegistryError::InvalidAttachment { .. }
            | RegistryError::BadRequest { .. } => StatusCode::BAD_REQUEST,
            RegistryError::Unauthenticated { .. } => StatusCode::UNAUTHORIZED,
            RegistryError::Forbidden { .. } => StatusCode::FORBIDDEN,
            RegistryError::Io(_) | RegistryError::Json(_) => StatusCode::BAD_GATEWAY,
        }
    }
}

pub type Result<Value, Error = RegistryError> = std::result::Result<Value, Error>;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::http::StatusCode;
    use tokio::net::TcpListener;

    use super::RegistryError;

    /// `reqwest::Error` has no public constructor, so the only way to
    /// get a real `is_timeout()` error in a test is to actually time
    /// out. Spin up a TCP listener that accepts and holds the socket
    /// open, fire a reqwest request with a sub-second timeout against
    /// it, and check the error round-trips through `status_code()`.
    #[tokio::test]
    async fn timeout_error_maps_to_gateway_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // Keep accepted sockets alive for the duration of the test so
        // the client really hangs on read instead of seeing FIN.
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });

        let client =
            reqwest::Client::builder().timeout(Duration::from_millis(100)).build().unwrap();
        let url = format!("http://{addr}/");
        let err = client.get(&url).send().await.unwrap_err();
        assert!(err.is_timeout(), "expected timeout error, got {err:?}");

        let registry_err = RegistryError::Upstream { url, source: err };
        assert_eq!(registry_err.status_code(), StatusCode::GATEWAY_TIMEOUT);
    }
}
