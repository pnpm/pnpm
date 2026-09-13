use super::{RegistryError, StatusCode, upstream_body_status_code, upstream_status_code};
impl RegistryError {
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
            _ => self.request_log_kind(),
        }
    }

    fn request_log_kind(&self) -> &'static str {
        match self {
            RegistryError::NotFound => "not_found",
            RegistryError::Unauthenticated { .. } => "unauthenticated",
            RegistryError::Forbidden { .. } => "forbidden",
            RegistryError::TeamsConfigManaged { .. } => "teams_config_managed",
            RegistryError::InvalidAttachment { .. } => "invalid_attachment",
            RegistryError::BadRequest { .. } => "bad_request",
            _ => self.publication_log_kind(),
        }
    }

    fn publication_log_kind(&self) -> &'static str {
        match self {
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
            _ => self.request_status_code(),
        }
    }

    fn request_status_code(&self) -> StatusCode {
        match self {
            RegistryError::InvalidPackageName { .. }
            | RegistryError::InvalidEcosystemPackageName { .. }
            | RegistryError::InvalidTarballName { .. }
            | RegistryError::InvalidConfig { .. }
            | RegistryError::InvalidAttachment { .. }
            | RegistryError::BadRequest { .. } => StatusCode::BAD_REQUEST,
            _ => self.publication_status_code(),
        }
    }

    fn publication_status_code(&self) -> StatusCode {
        match self {
            RegistryError::VersionAlreadyPublished { .. }
            | RegistryError::ArtifactAlreadyPublished { .. }
            | RegistryError::PublishNotRecorded { .. }
            | RegistryError::BlobUploadConflict { .. }
            | RegistryError::DocumentWriteConflict { .. }
            | RegistryError::StagedApprovalInFlight { .. }
            | RegistryError::RevisionReferenceLimit { .. }
            | RegistryError::RevisionReferenceWriteConflict { .. } => StatusCode::CONFLICT,
            _ => self.access_status_code(),
        }
    }

    fn access_status_code(&self) -> StatusCode {
        match self {
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
