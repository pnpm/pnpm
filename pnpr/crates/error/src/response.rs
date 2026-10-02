use crate::{RegistryError, redact_url_credentials};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

impl RegistryError {
    /// Whether a failed upstream fetch is a transient *availability* failure —
    /// a transport error, an open circuit breaker, or an upstream `5xx`. A `4xx`
    /// is an authoritative response about *this* request — `401`/`403` (auth),
    /// `429` (throttle), `400`/`410`, etc. — and is **not** transient: it must
    /// surface immediately rather than be masked (e.g. by serving a stale cache
    /// entry), which would let a revoked credential or a `410 Gone` keep being
    /// answered from old bytes.
    ///
    /// A `404` never reaches here — it is modeled as a distinct not-found
    /// outcome, not an error.
    #[must_use]
    pub fn is_transient_upstream_error(&self) -> bool {
        match self {
            RegistryError::Upstream { .. }
            | RegistryError::UpstreamBody { .. }
            | RegistryError::UpstreamUnavailable { .. } => true,
            RegistryError::UpstreamStatus { status, .. } => *status >= 500,
            _ => false,
        }
    }

    #[must_use]
    pub fn log_kind(&self) -> &'static str {
        match self {
            RegistryError::Upstream { .. } | RegistryError::UpstreamBody { .. } => "upstream",
            RegistryError::UpstreamStatus { .. } => "upstream_status",
            RegistryError::UpstreamResponse { .. } => "upstream_response",
            RegistryError::UpstreamUnavailable { .. } => "upstream_unavailable",
            RegistryError::TarballIntegrity { .. } => "tarball_integrity",
            RegistryError::InvalidPackageName { .. } => "invalid_package_name",
            RegistryError::InvalidEcosystemPackageName { .. } => "invalid_package_name",
            RegistryError::InvalidTarballName { .. } => "invalid_tarball_name",
            RegistryError::InvalidConfig { .. } => "invalid_config",
            RegistryError::NotFound => "not_found",
            RegistryError::Unauthenticated { .. } => "unauthenticated",
            RegistryError::Forbidden { .. } => "forbidden",
            RegistryError::TeamsConfigManaged { .. } => "teams_config_managed",
            RegistryError::InvalidAttachment { .. } => "invalid_attachment",
            RegistryError::BadRequest { .. } => "bad_request",
            RegistryError::VersionAlreadyPublished { .. } => "version_already_published",
            RegistryError::ArtifactAlreadyPublished { .. } => "artifact_already_published",
            RegistryError::PublishNotRecorded { .. } => "publish_not_recorded",
            RegistryError::BlobUploadConflict { .. } => "blob_upload_conflict",
            RegistryError::DocumentWriteConflict { .. } => "document_write_conflict",
            RegistryError::StagedApprovalInFlight { .. } => "staged_approval_in_flight",
            RegistryError::RevisionReferenceLimit { .. } => "revision_reference_limit",
            RegistryError::RevisionReferenceWriteConflict { .. } => {
                "revision_reference_write_conflict"
            }
            RegistryError::OsvVulnerability { .. } => "osv_vulnerability",
            RegistryError::RegistrationDisabled => "registration_disabled",
            RegistryError::TooManyUsers { .. } => "too_many_users",
            _ => self.storage_log_kind(),
        }
    }

    fn storage_log_kind(&self) -> &'static str {
        match self {
            RegistryError::Internal { .. } => "internal",
            RegistryError::InvalidHtpasswdFile { .. } => "invalid_htpasswd_file",
            RegistryError::Bcrypt(_) => "bcrypt",
            RegistryError::Sqlite(_) => "sqlite",
            #[cfg(feature = "backend-libsql")]
            RegistryError::Libsql(_) => "libsql",
            #[cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]
            RegistryError::Sqlx(_) => "sqlx",
            #[cfg(any(
                feature = "backend-libsql",
                feature = "backend-postgres",
                feature = "backend-mysql"
            ))]
            RegistryError::AuthDatabaseTimeout => "auth_database_timeout",
            RegistryError::JoinError(_) => "join_error",
            RegistryError::Io(_) => "io",
            RegistryError::ObjectStore(_) => "object_store",
            RegistryError::Json(_) => "json",
            _ => unreachable!("non-storage errors are classified by log_kind"),
        }
    }

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
    pub fn log_message(&self) -> String {
        redact_url_credentials(&self.to_string())
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

/// Only server faults need [`Self::log_message`]'s redaction: every variant
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
