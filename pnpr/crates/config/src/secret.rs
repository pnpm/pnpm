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
#[derive(Clone, PartialEq, Eq, derive_more::From)]
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

    /// `value` as a secret, refused while it holds an unset environment
    /// variable.
    pub(crate) fn checked(value: String) -> Result<Self, String> {
        match unset_placeholder_in(&value) {
            Some(placeholder) => Err(refuse(format!(
                "a secret-bearing field references the unset environment variable {placeholder}",
            ))),
            None => Ok(Self(value)),
        }
    }
}

impl Deref for Secret {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
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

/// The value as written. A graph document the metadata database stores holds
/// only `${VAR}` references here, which its credential rules require.
impl serde::Serialize for Secret {
    fn serialize<Ser>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error>
    where
        Ser: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
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

/// `reason` as the error of a secret-bearing value, which [`parse_with_env`]
/// reports rather than reading as the place of an empty substitution.
pub(crate) fn refuse(reason: String) -> String {
    REFUSED_SECRET.set(true);
    reason
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
    UNSET_PLACEHOLDERS.with_borrow(|unset| {
        unset
            .iter()
            .find(|entry| value.contains(&entry.token))
            .map(|entry| entry.placeholder.clone())
    })
}

/// Substitute environment variables into `raw` and deserialize the result.
///
/// A placeholder that resolves to nothing is a [`Secret`] error. Elsewhere it
/// becomes an empty string with a warning.
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
    let checked = check_secrets::<Value>(marked);
    let unset = UNSET_PLACEHOLDERS.with_borrow_mut(std::mem::take);
    if let Err((reason, marked)) = checked {
        let reason = name_unset_placeholders(&reason, &marked, &unset);
        return Err(RegistryError::InvalidConfig { reason });
    }
    parse_yaml(&substituted)
}

/// Parse `marked` until no [`Secret`] holds a token. A parse error at a token
/// that no secret refused marks a place an empty substitution must reach, so
/// that token becomes empty, as [`env_replace_lossy`] leaves it, and the parse
/// retries. Returns the error that remains, with the document it came from.
fn check_secrets<Value>(mut marked: String) -> Result<(), (String, String)>
where
    Value: for<'de> Deserialize<'de>,
{
    loop {
        REFUSED_SECRET.set(false);
        let Err(RegistryError::InvalidConfig { reason }) = parse_yaml::<Value>(&marked) else {
            return Ok(());
        };
        if REFUSED_SECRET.take() || !blank_token_at(&mut marked, &reason) {
            return Err((reason, marked));
        }
    }
}

/// Empty the token that starts where `reason` locates its error. Returns
/// whether one did.
fn blank_token_at(marked: &mut String, reason: &str) -> bool {
    let Some(range) = error_location(reason).and_then(|location| token_range_at(marked, location))
    else {
        return false;
    };
    marked.replace_range(range, "");
    true
}

/// The byte range of the token that starts at the 1-based `(line, column)`.
fn token_range_at(marked: &str, (line, column): (usize, usize)) -> Option<std::ops::Range<usize>> {
    let preceding = line.checked_sub(1)?;
    let text = marked.split('\n').nth(preceding)?;
    let line_start: usize = marked
        .split('\n')
        .take(preceding)
        .map(|earlier| earlier.len() + 1)
        .sum();
    UNSET_PLACEHOLDERS.with_borrow(|unset| {
        unset
            .iter()
            .find_map(|entry| {
                let offset = text.find(&entry.token)?;
                let starts_at_column = text[..offset].chars().count() + 1 == column;
                starts_at_column.then(|| {
                    line_start + offset..line_start + offset + entry.token.len()
                })
            })
    })
}

/// The 1-based line and column of a `line <L> column <C>` error.
fn error_location(reason: &str) -> Option<(usize, usize)> {
    let (_, rest) = reason.split_once("line ")?;
    let (line, rest) = rest.split_once(" column ")?;
    let column = rest
        .split(|char: char| !char.is_ascii_digit())
        .next()?;
    Some((line.parse().ok()?, column.parse().ok()?))
}

/// `reason`, a located error in `marked`, with the unset placeholders that
/// stand on its line named, or every unset placeholder when none does.
fn name_unset_placeholders(reason: &str, marked: &str, unset: &[UnsetPlaceholder]) -> String {
    let reason = unset
        .iter()
        .fold(reason.to_string(), |reason, entry| reason.replace(&entry.token, &entry.placeholder));
    if unset
        .iter()
        .any(|entry| reason.contains(&entry.placeholder))
    {
        return reason;
    }
    let error_line = error_location(&reason)
        .and_then(|(line, _)| {
            marked
                .split('\n')
                .nth(line.saturating_sub(1))
        })
        .unwrap_or_default();
    let on_line: Vec<&str> = unset
        .iter()
        .filter(|entry| error_line.contains(&entry.token))
        .map(|entry| entry.placeholder.as_str())
        .collect();
    let named = if on_line.is_empty() {
        unset
            .iter()
            .map(|entry| entry.placeholder.as_str())
            .collect()
    } else {
        on_line
    };
    format!("{reason} (unset environment variables: {})", named.join(", "))
}

/// An empty document parses as `{}`. An error keeps only its located message:
/// the parser also quotes the lines around the error, which can hold a
/// credential.
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
    let reason = err.to_string();
    let located = reason
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    RegistryError::InvalidConfig { reason: located }
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
