#[cfg(test)]
pub(crate) mod tests;

use pnpr_error::{RegistryError, Result};

/// Maximum password length accepted by pnpr to prevent bcrypt 72-byte truncation issues.
pub const MAX_PASSWORD_BYTES: usize = 71;

/// Minimum number of Unicode characters in a password.
pub const MIN_PASSWORD_CHARS: usize = 8;

pub(crate) fn validate_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(RegistryError::BadRequest {
            reason: format!("password must contain at least {MIN_PASSWORD_CHARS} characters"),
        });
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(RegistryError::BadRequest {
            reason: format!("password cannot exceed {MAX_PASSWORD_BYTES} bytes"),
        });
    }
    Ok(())
}

pub(crate) const MAX_USERNAME_CHARS: usize = 255;

pub(crate) fn validate_username(username: &str) -> Result<()> {
    if username.is_empty() {
        return Err(RegistryError::BadRequest { reason: "username must not be empty".to_string() });
    }
    if username.chars().count() > MAX_USERNAME_CHARS {
        return Err(RegistryError::BadRequest {
            reason: format!("username must be at most {MAX_USERNAME_CHARS} characters"),
        });
    }
    let reason = rejected_username_reason(username);
    match reason {
        Some(reason) => Err(RegistryError::BadRequest { reason: reason.to_string() }),
        None => Ok(()),
    }
}

/// Why a username of an acceptable length is still not one pnpr will store.
fn rejected_username_reason(username: &str) -> Option<&'static str> {
    let trimmed = username.trim_matches(char::is_whitespace);
    if trimmed.len() != username.len() {
        return Some("username must not start or end with whitespace");
    }
    if username.starts_with('#') {
        return Some("username must not start with '#'");
    }
    if username.contains(':') {
        return Some("username must not contain ':'");
    }
    if username.chars().any(char::is_control) {
        return Some("username must not contain control characters");
    }
    None
}

pub(crate) fn token_timestamp_from_sql(timestamp: i64) -> u64 {
    timestamp.max(0) as u64
}

pub(crate) fn token_timestamp_to_sql(timestamp: u64) -> i64 {
    i64::try_from(timestamp).unwrap_or(i64::MAX)
}

pub(crate) fn token_scopes_from_sql(
    scopes_json: Option<&str>,
    token_hash: &str,
) -> Result<Option<Vec<String>>> {
    match scopes_json {
        Some(raw) => {
            let scopes = serde_json::from_str(raw)
                .map_err(|err| RegistryError::Internal {
                    reason: format!("token {token_hash} has an unreadable scopes list: {err}"),
                })?;
            Ok(Some(scopes))
        }
        None => Ok(None),
    }
}

pub(crate) fn token_scopes_to_sql(scopes: Option<&[String]>) -> Option<String> {
    scopes.map(|scopes_list| {
        serde_json::to_string(scopes_list).expect("Vec<String> always serializes to JSON")
    })
}

pub(crate) fn username_skeleton_hash(username: &str) -> String {
    let skeleton: String = unicode_security::confusable_detection::skeleton(username).collect();
    pnpm_crypto_hash::create_hex_hash(&skeleton)
}

pub(crate) fn confusable_username() -> RegistryError {
    RegistryError::BadRequest {
        reason: "username is too similar to an existing username".to_string(),
    }
}

#[cfg(any(feature = "backend-libsql", feature = "backend-postgres", feature = "backend-mysql"))]
pub(crate) fn username_conflict(username: &str, exact_match: bool) -> RegistryError {
    if exact_match {
        RegistryError::AdminConflict { reason: format!("username {username:?} is taken") }
    } else {
        confusable_username()
    }
}
