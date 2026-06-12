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

    #[display("Package policy pattern {pattern:?} is invalid: {reason}")]
    #[from(skip)]
    InvalidPolicyPattern {
        #[error(not(source))]
        pattern: String,
        reason: String,
    },

    /// The YAML config could not be parsed. Startup-only — this never
    /// surfaces over HTTP, but `Config` parsing shares this error type.
    #[display("Invalid config: {reason}")]
    #[from(skip)]
    InvalidConfig {
        #[error(not(source))]
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

    /// `auth.htpasswd.max_users: -1` blocks new registrations.
    /// Returned for adduser on a username that doesn't already
    /// exist; existing-user logins are unaffected.
    #[display("New user registration is disabled by auth.htpasswd.max_users: -1")]
    RegistrationDisabled,

    /// `auth.htpasswd.max_users: N` cap reached. Returned for
    /// adduser on a username that doesn't already exist.
    #[display("Maximum number of users ({max}) reached")]
    #[from(skip)]
    TooManyUsers { max: u64 },

    /// The htpasswd file on disk couldn't be parsed at startup.
    /// Surfaced as a startup-time error rather than a silent empty
    /// store so a corrupted file can't quietly lock every existing
    /// user out.
    #[display("Invalid htpasswd file {path}: {reason}")]
    #[from(skip)]
    InvalidHtpasswdFile {
        #[error(not(source))]
        path: String,
        reason: String,
    },

    /// Bcrypt hash/verify failure. Operational error, not user-facing.
    #[display("Bcrypt failure: {_0}")]
    Bcrypt(bcrypt::BcryptError),

    /// SQLite-backed token store failure.
    #[display("Token database error: {_0}")]
    Sqlite(rusqlite::Error),

    /// Networked-SQLite (libsql / Turso) auth backend failure.
    #[display("Auth database error: {_0}")]
    Libsql(libsql::Error),

    /// A blocking task spawned for bcrypt or `SQLite` work panicked
    /// or was cancelled. Treat as an internal server error.
    #[display("Background task failed: {_0}")]
    JoinError(tokio::task::JoinError),

    #[display("I/O error: {_0}")]
    Io(std::io::Error),

    /// Object-store (S3 / R2 / S3-compatible) backend failure on the
    /// hosted store.
    #[display("Object store error: {_0}")]
    ObjectStore(object_store::Error),

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
    #[must_use]
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
            | RegistryError::InvalidPolicyPattern { .. }
            | RegistryError::InvalidConfig { .. }
            | RegistryError::InvalidAttachment { .. }
            | RegistryError::BadRequest { .. } => StatusCode::BAD_REQUEST,
            RegistryError::Unauthenticated { .. } => StatusCode::UNAUTHORIZED,
            RegistryError::Forbidden { .. } => StatusCode::FORBIDDEN,
            RegistryError::RegistrationDisabled | RegistryError::TooManyUsers { .. } => {
                StatusCode::FORBIDDEN
            }
            RegistryError::InvalidHtpasswdFile { .. }
            | RegistryError::Bcrypt(_)
            | RegistryError::Sqlite(_)
            | RegistryError::Libsql(_)
            | RegistryError::JoinError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            RegistryError::Io(_) | RegistryError::ObjectStore(_) | RegistryError::Json(_) => {
                StatusCode::BAD_GATEWAY
            }
        }
    }
}

pub type Result<Value, Error = RegistryError> = std::result::Result<Value, Error>;

#[cfg(test)]
mod tests;
