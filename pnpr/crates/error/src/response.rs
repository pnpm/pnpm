use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::RegistryError;

impl RegistryError {
    fn storage_status_code(&self) -> StatusCode {
        match self {
            RegistryError::Internal { .. }
            | RegistryError::InvalidHtpasswdFile { .. }
            | RegistryError::Bcrypt(_)
            | RegistryError::Sqlite(_)
            | RegistryError::JoinError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "backend-libsql")]
            RegistryError::Libsql(_) => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]
            RegistryError::Sqlx(_) => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(any(
                feature = "backend-libsql",
                feature = "backend-postgres",
                feature = "backend-mysql"
            ))]
            RegistryError::AuthDatabaseTimeout => StatusCode::GATEWAY_TIMEOUT,
            RegistryError::Io(_) | RegistryError::ObjectStore(_) | RegistryError::Json(_) => {
                StatusCode::BAD_GATEWAY
            }
            _ => unreachable!("non-storage errors are classified by status_code"),
        }
    }

    #[must_use]
    pub fn public_message(&self) -> String {
        let status = self.status_code();
        if status.is_server_error() {
            return status
                .canonical_reason()
                .unwrap_or("Internal Server Error")
                .to_string();
        }
        self.to_string()
    }

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
    #[must_use]
    pub fn status_code(&self) -> StatusCode {
        match self {
            RegistryError::Upstream { source, .. } => upstream_status_code(source),
            RegistryError::UpstreamBody { source, .. } => upstream_body_status_code(source),
            RegistryError::UpstreamStatus { .. }
            | RegistryError::UpstreamResponse { .. }
            | RegistryError::TarballIntegrity { .. } => StatusCode::BAD_GATEWAY,
            RegistryError::UpstreamUnavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            RegistryError::InvalidPackageName { .. }
            | RegistryError::InvalidEcosystemPackageName { .. }
            | RegistryError::InvalidTarballName { .. }
            | RegistryError::InvalidConfig { .. }
            | RegistryError::InvalidAttachment { .. }
            | RegistryError::BadRequest { .. } => StatusCode::BAD_REQUEST,
            RegistryError::VersionAlreadyPublished { .. }
            | RegistryError::ArtifactAlreadyPublished { .. }
            | RegistryError::PublishNotRecorded { .. }
            | RegistryError::BlobUploadConflict { .. }
            | RegistryError::DocumentWriteConflict { .. }
            | RegistryError::StagedApprovalInFlight { .. }
            | RegistryError::RevisionReferenceLimit { .. }
            | RegistryError::RevisionReferenceWriteConflict { .. } => StatusCode::CONFLICT,
            RegistryError::NotFound => StatusCode::NOT_FOUND,
            RegistryError::Unauthenticated { .. } => StatusCode::UNAUTHORIZED,
            RegistryError::Forbidden { .. }
            | RegistryError::TeamsConfigManaged { .. }
            | RegistryError::OsvVulnerability { .. }
            | RegistryError::RegistrationDisabled
            | RegistryError::TooManyUsers { .. } => StatusCode::FORBIDDEN,
            _ => self.storage_status_code(),
        }
    }
}

/// Only server faults need [`RegistryError::log_message`]'s redaction: every variant
/// that can embed a request URL — and so a credential — is one. The 401 / 403 /
/// 404 tier drops to `debug` so a probing or unauthorized client cannot flood
/// the log with warnings.
impl IntoResponse for RegistryError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let error_kind = self.log_kind();
        if status.is_server_error() {
            let err = self.log_message();
            tracing::error!(%err, %error_kind, %status, "request failed");
        } else if matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND,
        ) {
            tracing::debug!(err = %self, %error_kind, %status, "request failed");
        } else {
            tracing::warn!(err = %self, %error_kind, %status, "request failed");
        }
        (status, self.public_message()).into_response()
    }
}

fn upstream_status_code(source: &reqwest::Error) -> StatusCode {
    if source.is_timeout() {
        StatusCode::GATEWAY_TIMEOUT
    } else if source.is_connect() {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::BAD_GATEWAY
    }
}

fn upstream_body_status_code(source: &std::io::Error) -> StatusCode {
    if source.kind() == std::io::ErrorKind::TimedOut
        || source
            .get_ref()
            .and_then(|source| source.downcast_ref::<reqwest::Error>())
            .is_some_and(reqwest::Error::is_timeout)
    {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    }
}
