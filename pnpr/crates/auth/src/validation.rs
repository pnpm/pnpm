use super::{MAX_USERNAME_CHARS, RegistryError, Result};
pub(crate) fn validate_username(username: &str) -> Result<()> {
    if username.is_empty() {
        return Err(RegistryError::BadRequest {
            reason: "username must not be empty".to_string(),
        });
    }
    if username.chars().count() > MAX_USERNAME_CHARS {
        return Err(RegistryError::BadRequest {
            reason: format!("username must be at most {MAX_USERNAME_CHARS} characters"),
        });
    }
    let reason = rejected_username_reason(username);
    match reason {
        Some(reason) => Err(RegistryError::BadRequest {
            reason: reason.to_string(),
        }),
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
