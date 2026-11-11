#[cfg(test)]
pub(crate) mod tests;

use icu_normalizer::{
    DecomposingNormalizerBorrowed, properties::CanonicalCombiningClassMapBorrowed,
};
use pnpr_error::{RegistryError, Result};
use unicode_security::{GeneralSecurityProfile as _, MixedScript as _};

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
    let combining_classes = CanonicalCombiningClassMapBorrowed::new();
    if username
        .chars()
        .next()
        .is_some_and(|first| combining_classes.get_u8(first) != 0)
    {
        return Some("username must not start with a combining mark");
    }
    if username
        .chars()
        .any(|character| !character.is_ascii() && !character.identifier_allowed())
    {
        return Some("username contains restricted Unicode characters");
    }
    if !username.is_single_script() {
        return Some("username must not mix scripts");
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

fn fold_case(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            'ß' | 'ẞ' => folded.push_str("ss"),
            'ς' => folded.push('σ'),
            other => folded.extend(other.to_lowercase()),
        }
    }
    folded
}

/// Hash a username's normalized confusable key.
///
/// Folding case before UTS 39 `skeleton` keeps `'I'` aligned with `'i'` instead
/// of `'l'`, folding again after `skeleton` lowers uppercase prototypes such as
/// `'0' -> 'O'`, and dropping non-starter combining marks equates accented and
/// stroked lookalikes with their base letters.
pub(crate) fn username_skeleton_hash(username: &str) -> String {
    let decomposed = DecomposingNormalizerBorrowed::new_nfkd().normalize(username);
    let pre_folded = fold_case(&decomposed);
    let raw_skeleton: String =
        unicode_security::confusable_detection::skeleton(&pre_folded).collect();
    let combining_classes = CanonicalCombiningClassMapBorrowed::new();
    let skeleton: String = fold_case(&raw_skeleton)
        .chars()
        .filter(|character| combining_classes.get_u8(*character) == 0)
        .collect();
    let key = if skeleton.is_empty() { pre_folded.as_str() } else { skeleton.as_str() };
    pnpm_crypto_hash::create_hex_hash(key)
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
