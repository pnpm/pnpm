//! The [`Secret`] config value, and the environment substitution that lets it
//! refuse an unset variable.

use std::{
    cell::{Cell, RefCell},
    fmt,
    ops::Deref,
};

use pnpm_env_replace::{EnvVar, env_replace_lossy, placeholder_ranges};
use pnpr_error::RegistryError;
use serde::Deserialize;

/// A secret-bearing config string. It deserializes only from a value that
/// references no unset environment variable, so a missing variable fails
/// startup instead of leaving an empty credential, and its `Debug` output is
/// redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl Deref for Secret {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[redacted]")
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        Self::checked(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl Secret {
    /// `value` as a secret, refused while it holds an unset environment
    /// variable.
    pub(crate) fn checked(value: String) -> Result<Self, String> {
        match unset_placeholder_in(&value) {
            Some(placeholder) => Err(format!(
                "a secret-bearing field references the unset environment variable {placeholder}",
            )),
            None => Ok(Self(value)),
        }
    }
}

/// An unresolved `${...}` placeholder and the token standing in for it while
/// the config is checked.
struct UnsetPlaceholder {
    token: String,
    placeholder: String,
}

thread_local! {
    static UNSET_PLACEHOLDERS: RefCell<Vec<UnsetPlaceholder>> = const { RefCell::new(Vec::new()) };
    static REFUSED_SECRET: Cell<bool> = const { Cell::new(false) };
}

fn unset_placeholder_in(value: &str) -> Option<String> {
    let placeholder = UNSET_PLACEHOLDERS.with_borrow(|unset| {
        unset
            .iter()
            .find(|entry| value.contains(&entry.token))
            .map(|entry| entry.placeholder.clone())
    });
    if placeholder.is_some() {
        REFUSED_SECRET.set(true);
    }
    placeholder
}

/// Substitute environment variables into `raw` and deserialize the result.
///
/// A placeholder that resolves to nothing is a [`Secret`] error. Elsewhere it
/// becomes an empty string with a warning, except where an empty string is
/// not a valid value, such as a number, which fails with the placeholder
/// named.
pub(crate) fn parse_with_env<Sys: EnvVar, Value>(raw: &str) -> Result<Value, RegistryError>
where
    Value: for<'de> Deserialize<'de>,
{
    let (substituted, unresolved) = env_replace_lossy::<Sys>(raw);
    if unresolved.is_empty() {
        return parse_yaml(&substituted);
    }
    tracing::warn!(?unresolved, "config references unset environment variables");
    let (marked, unset) = mark_unset_placeholders::<Sys>(raw);
    UNSET_PLACEHOLDERS.with_borrow_mut(|slot| *slot = unset);
    REFUSED_SECRET.set(false);
    let result = parse_yaml::<Value>(&marked);
    let unset = UNSET_PLACEHOLDERS.with_borrow_mut(std::mem::take);
    if let Err(RegistryError::InvalidConfig { reason }) = result {
        // The parser quotes the lines around the error, which hold the rest of
        // the secret, so a refused secret keeps only the located message.
        let reason = if REFUSED_SECRET.take() {
            reason
                .lines()
                .next()
                .unwrap_or_default()
                .to_string()
        } else {
            reason
        };
        let reason = unset
            .iter()
            .fold(reason, |reason, entry| reason.replace(&entry.token, &entry.placeholder));
        return Err(RegistryError::InvalidConfig { reason });
    }
    parse_yaml(&substituted)
}

/// An empty document parses as `{}`.
fn parse_yaml<Value>(text: &str) -> Result<Value, RegistryError>
where
    Value: for<'de> Deserialize<'de>,
{
    match serde_saphyr::from_str::<Option<Value>>(text).map_err(invalid)? {
        Some(value) => Ok(value),
        None => serde_saphyr::from_str("{}").map_err(invalid),
    }
}

fn invalid(err: impl fmt::Display) -> RegistryError {
    RegistryError::InvalidConfig { reason: err.to_string() }
}

/// `raw` with every placeholder the environment leaves unresolved replaced by
/// a token that occurs nowhere else, then substituted.
fn mark_unset_placeholders<Sys: EnvVar>(raw: &str) -> (String, Vec<UnsetPlaceholder>) {
    let nonce = token_nonce();
    let mut marked = raw.to_string();
    let mut unset = Vec::new();
    for range in placeholder_ranges(raw).into_iter().rev() {
        let placeholder = &raw[range.clone()];
        if env_replace_lossy::<Sys>(placeholder).1.is_empty() {
            continue;
        }
        let token = format!("pnprunset{nonce}n{}", unset.len());
        marked.replace_range(range, &token);
        unset.push(UnsetPlaceholder { token, placeholder: placeholder.to_string() });
    }
    (env_replace_lossy::<Sys>(&marked).0, unset)
}

fn token_nonce() -> String {
    use std::fmt::Write as _;
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("OS CSPRNG must be available");
    bytes
        .iter()
        .fold(String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}
