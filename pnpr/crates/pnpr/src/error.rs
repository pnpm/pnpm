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

    /// The uplink's circuit breaker is open: it reached `max_fails`
    /// consecutive failures and is still inside its `fail_timeout`
    /// cooldown, so pnpr short-circuits the request instead of hammering
    /// a known-down upstream. The packument path turns this into a
    /// stale-cache fallback; with nothing cached it surfaces as 503.
    #[display("Upstream {uplink} is temporarily unavailable (circuit open)")]
    #[from(skip)]
    UpstreamUnavailable {
        #[error(not(source))]
        uplink: String,
    },

    #[display("EINTEGRITY: tarball {filename:?} for package {package:?}: {reason}")]
    #[from(skip)]
    TarballIntegrity {
        #[error(not(source))]
        package: String,
        filename: String,
        reason: String,
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

    #[display(
        "Package {package}@{version} is listed in the local OSV database as vulnerable ({advisories})"
    )]
    #[from(skip)]
    OsvVulnerability {
        #[error(not(source))]
        package: String,
        #[error(not(source))]
        version: String,
        #[error(not(source))]
        advisories: String,
    },

    /// New-user registration is off: `auth.htpasswd.max_users` is
    /// unset (the secure default) or set to `-1`. Returned for adduser
    /// on a username that doesn't already exist; existing-user logins
    /// are unaffected.
    #[display(
        "New user registration is disabled. Set auth.htpasswd.max_users to a positive number to allow sign-ups"
    )]
    RegistrationDisabled,

    /// `auth.htpasswd.max_users: N` cap reached. Returned for
    /// adduser on a username that doesn't already exist.
    #[display("Maximum number of users ({max}) reached")]
    #[from(skip)]
    TooManyUsers { max: u64 },

    #[display("Internal error: {reason}")]
    #[from(skip)]
    Internal {
        #[error(not(source))]
        reason: String,
    },

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
    #[cfg(feature = "backend-libsql")]
    #[display("Auth database error: {_0}")]
    Libsql(libsql::Error),

    /// SQL auth backend failure.
    #[cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]
    #[display("Auth database error: {_0}")]
    Sqlx(sqlx::Error),

    /// SQL auth backend operation timed out.
    #[cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]
    #[display("Auth database timeout")]
    AuthDatabaseTimeout,

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
    #[must_use]
    pub fn log_kind(&self) -> &'static str {
        match self {
            RegistryError::Upstream { .. } => "upstream",
            RegistryError::UpstreamStatus { .. } => "upstream_status",
            RegistryError::UpstreamUnavailable { .. } => "upstream_unavailable",
            RegistryError::TarballIntegrity { .. } => "tarball_integrity",
            RegistryError::InvalidPackageName { .. } => "invalid_package_name",
            RegistryError::InvalidTarballName { .. } => "invalid_tarball_name",
            RegistryError::InvalidPolicyPattern { .. } => "invalid_policy_pattern",
            RegistryError::InvalidConfig { .. } => "invalid_config",
            RegistryError::Unauthenticated { .. } => "unauthenticated",
            RegistryError::Forbidden { .. } => "forbidden",
            RegistryError::InvalidAttachment { .. } => "invalid_attachment",
            RegistryError::BadRequest { .. } => "bad_request",
            RegistryError::OsvVulnerability { .. } => "osv_vulnerability",
            RegistryError::RegistrationDisabled => "registration_disabled",
            RegistryError::TooManyUsers { .. } => "too_many_users",
            RegistryError::Internal { .. } => "internal",
            RegistryError::InvalidHtpasswdFile { .. } => "invalid_htpasswd_file",
            RegistryError::Bcrypt(_) => "bcrypt",
            RegistryError::Sqlite(_) => "sqlite",
            #[cfg(feature = "backend-libsql")]
            RegistryError::Libsql(_) => "libsql",
            #[cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]
            RegistryError::Sqlx(_) => "sqlx",
            #[cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]
            RegistryError::AuthDatabaseTimeout => "auth_database_timeout",
            RegistryError::JoinError(_) => "join_error",
            RegistryError::Io(_) => "io",
            RegistryError::ObjectStore(_) => "object_store",
            RegistryError::Json(_) => "json",
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
            return status.canonical_reason().unwrap_or("Internal Server Error").to_string();
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
            RegistryError::Upstream { source, .. } => {
                if source.is_timeout() {
                    StatusCode::GATEWAY_TIMEOUT
                } else if source.is_connect() {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::BAD_GATEWAY
                }
            }
            RegistryError::UpstreamStatus { .. } | RegistryError::TarballIntegrity { .. } => {
                StatusCode::BAD_GATEWAY
            }
            RegistryError::UpstreamUnavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            RegistryError::InvalidPackageName { .. }
            | RegistryError::InvalidTarballName { .. }
            | RegistryError::InvalidPolicyPattern { .. }
            | RegistryError::InvalidConfig { .. }
            | RegistryError::InvalidAttachment { .. }
            | RegistryError::BadRequest { .. } => StatusCode::BAD_REQUEST,
            RegistryError::Unauthenticated { .. } => StatusCode::UNAUTHORIZED,
            RegistryError::Forbidden { .. } => StatusCode::FORBIDDEN,
            RegistryError::OsvVulnerability { .. } => StatusCode::FORBIDDEN,
            RegistryError::RegistrationDisabled | RegistryError::TooManyUsers { .. } => {
                StatusCode::FORBIDDEN
            }
            RegistryError::Internal { .. }
            | RegistryError::InvalidHtpasswdFile { .. }
            | RegistryError::Bcrypt(_)
            | RegistryError::Sqlite(_)
            | RegistryError::JoinError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "backend-libsql")]
            RegistryError::Libsql(_) => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]
            RegistryError::Sqlx(_) => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]
            RegistryError::AuthDatabaseTimeout => StatusCode::GATEWAY_TIMEOUT,
            RegistryError::Io(_) | RegistryError::ObjectStore(_) | RegistryError::Json(_) => {
                StatusCode::BAD_GATEWAY
            }
        }
    }
}

fn redact_url_credentials(message: &str) -> String {
    let mut redacted = String::with_capacity(message.len());
    let mut cursor = 0;
    while let Some(relative_scheme_end) = message[cursor..].find("://") {
        let scheme_end = cursor + relative_scheme_end;
        let scheme_start = find_scheme_start(message, scheme_end);
        if scheme_start == scheme_end || !is_valid_scheme(&message[scheme_start..scheme_end]) {
            redacted.push_str(&message[cursor..scheme_end + 3]);
            cursor = scheme_end + 3;
            continue;
        }

        let url_end = find_url_end(message, scheme_end + 3);
        let (candidate, suffix) = split_trailing_punctuation(&message[scheme_start..url_end]);
        let Some(safe_url) = redact_url_candidate(candidate) else {
            redacted.push_str(&message[cursor..url_end]);
            cursor = url_end;
            continue;
        };

        redacted.push_str(&message[cursor..scheme_start]);
        redacted.push_str(&safe_url);
        redacted.push_str(suffix);
        cursor = url_end;
    }
    redacted.push_str(&message[cursor..]);
    redacted
}

fn find_scheme_start(message: &str, scheme_end: usize) -> usize {
    let bytes = message.as_bytes();
    let mut start = scheme_end;
    while start > 0 {
        let byte = bytes[start - 1];
        if !byte.is_ascii_alphanumeric() && byte != b'+' && byte != b'.' && byte != b'-' {
            break;
        }
        start -= 1;
    }
    start
}

fn is_valid_scheme(scheme: &str) -> bool {
    let mut chars = scheme.bytes();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'.' || byte == b'-'
        })
}

fn find_url_end(message: &str, url_start: usize) -> usize {
    message[url_start..]
        .char_indices()
        .find_map(|(offset, ch)| is_url_delimiter(ch).then_some(url_start + offset))
        .unwrap_or(message.len())
}

fn is_url_delimiter(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | '<' | '>' | '(' | ')' | '{' | '}')
}

fn split_trailing_punctuation(candidate: &str) -> (&str, &str) {
    let mut end = candidate.len();
    while let Some(ch) = candidate[..end].chars().next_back() {
        if !matches!(ch, '.' | ',' | ';' | '!') {
            break;
        }
        end -= ch.len_utf8();
    }
    (&candidate[..end], &candidate[end..])
}

fn redact_url_candidate(candidate: &str) -> Option<String> {
    redact_parseable_url_candidate(candidate).or_else(|| redact_unparsable_url_candidate(candidate))
}

fn redact_parseable_url_candidate(candidate: &str) -> Option<String> {
    let mut url = url::Url::parse(candidate).ok()?;
    let mut changed = false;
    if !url.username().is_empty() || url.password().is_some() {
        if url.set_username("redacted").is_ok() {
            changed = true;
        }
        if url.set_password(None).is_ok() {
            changed = true;
        }
    }

    if url.query().is_some() {
        let pairs = url
            .query_pairs()
            .map(|(key, value)| {
                if is_sensitive_query_key(&key) {
                    changed = true;
                    (key.into_owned(), "redacted".to_string())
                } else {
                    (key.into_owned(), value.into_owned())
                }
            })
            .collect::<Vec<_>>();
        if changed {
            url.query_pairs_mut()
                .clear()
                .extend_pairs(pairs.iter().map(|(key, value)| (&**key, &**value)));
        }
    }

    if url.fragment().is_some() {
        url.set_fragment(None);
        changed = true;
    }

    changed.then(|| url.to_string())
}

fn redact_unparsable_url_candidate(candidate: &str) -> Option<String> {
    let mut redacted = candidate.to_string();
    let mut changed = false;
    if let Some(safe_url) = redact_unparsable_url_userinfo(&redacted) {
        redacted = safe_url;
        changed = true;
    }
    if let Some(safe_url) = redact_sensitive_query_values(&redacted) {
        redacted = safe_url;
        changed = true;
    }
    if let Some(safe_url) = redact_fragment(&redacted) {
        redacted = safe_url;
        changed = true;
    }
    changed.then_some(redacted)
}

fn redact_unparsable_url_userinfo(candidate: &str) -> Option<String> {
    let authority_start = candidate.find("://")? + 3;
    let scan_end = candidate[authority_start..]
        .find('?')
        .map_or(candidate.len(), |offset| authority_start + offset);
    let userinfo_end = candidate[authority_start..scan_end].rfind('@')? + authority_start;
    let mut redacted = String::with_capacity(candidate.len());
    redacted.push_str(&candidate[..authority_start]);
    redacted.push_str("redacted@");
    redacted.push_str(&candidate[userinfo_end + 1..]);
    Some(redacted)
}

fn redact_sensitive_query_values(candidate: &str) -> Option<String> {
    let query_start = candidate.find('?')?;
    let fragment_start = candidate[query_start + 1..]
        .find('#')
        .map_or(candidate.len(), |offset| query_start + 1 + offset);
    let query = &candidate[query_start + 1..fragment_start];
    let mut redacted = String::with_capacity(candidate.len());
    redacted.push_str(&candidate[..=query_start]);
    let mut changed = false;
    for segment in query.split_inclusive('&') {
        let (pair, separator) = segment.strip_suffix('&').map_or((segment, ""), |pair| (pair, "&"));
        if let Some(value_start) = pair.find('=') {
            let key = &pair[..value_start];
            if is_sensitive_query_key(key) {
                redacted.push_str(key);
                redacted.push_str("=redacted");
                redacted.push_str(separator);
                changed = true;
                continue;
            }
        }
        redacted.push_str(segment);
    }
    redacted.push_str(&candidate[fragment_start..]);
    changed.then_some(redacted)
}

fn redact_fragment(candidate: &str) -> Option<String> {
    let fragment_start = candidate.find('#')?;
    Some(candidate[..fragment_start].to_string())
}

fn is_sensitive_query_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| *ch != '-' && *ch != '_')
        .map(|ch| ch.to_ascii_lowercase())
        .collect::<String>();
    matches!(
        normalized.as_str(),
        "auth"
            | "authtoken"
            | "password"
            | "passwd"
            | "pwd"
            | "secret"
            | "token"
            | "accesstoken"
            | "apikey",
    )
}

pub type Result<Value, Error = RegistryError> = std::result::Result<Value, Error>;

#[cfg(test)]
mod tests;
