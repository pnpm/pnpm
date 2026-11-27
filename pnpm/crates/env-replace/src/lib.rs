//! Environment-variable substitution for pnpm-style `${VAR}` placeholders.
//!
//! Occurrences of `${VAR}` (with optional `${VAR-default}` or `${VAR:-default}` fallback,
//! or npm's `${VAR?}`, which falls back to `""`) are replaced with the value the
//! [`EnvVar`] capability returns for `VAR`.
//! Backslashes immediately preceding the `$` escape the placeholder so
//! it is left as-is.
//!
//! The env lookup is threaded through the [`EnvVar`] capability trait so
//! callers can drive every branch (set, unset, empty) with local fakes
//! instead of mutating the real process environment. Production callers
//! thread [`SystemEnv`] (which delegates to `std::env::var`) through the
//! turbofish slot; `pnpm-config` threads its broader `Host` provider
//! instead, per the DI pattern from
//! [pnpm/pacquet#339](https://github.com/pnpm/pacquet/issues/339).

pub use pnpm_env_placeholders::placeholder_ranges;
pub use sentinel::SentinelPlaceholders;

use pnpm_env_placeholders::{
    Placeholder,
    placeholders,
};

mod sentinel;

/// Capability: read a process environment variable as a UTF-8 string.
pub trait EnvVar {
    /// Return the value of the named environment variable, or `None`
    /// when it is unset. Implementations should treat invalid UTF-8
    /// as `None` to match `std::env::var`'s behaviour, which is what
    /// pnpm itself observes via Node's `process.env`.
    fn var(name: &str) -> Option<String>;

    /// Enumerate every `(name, value)` environment variable pair.
    ///
    /// Used by consumers that must match env vars by prefix rather than
    /// by exact name (e.g. URL-scoped `npm_config_//…` auth settings,
    /// where the host is part of the variable name). Defaults to an
    /// empty set so existing fakes that only implement [`EnvVar::var`]
    /// keep compiling; production providers override it.
    #[must_use]
    fn vars() -> Vec<(String, String)> {
        Vec::new()
    }
}

/// Production [`EnvVar`] provider: reads the real process environment via
/// [`std::env::var`].
pub struct SystemEnv;

impl EnvVar for SystemEnv {
    fn var(name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn vars() -> Vec<(String, String)> {
        // `std::env::vars()` panics if any name/value is not valid UTF-8.
        // Iterate the OsString form and drop non-UTF-8 entries instead,
        // matching `var`'s `std::env::var(..).ok()` (which yields `None`
        // for non-UTF-8).
        std::env::vars_os()
            .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
            .collect()
    }
}

/// Replace `${VAR}`, `${VAR-default}`, and `${VAR:-default}` placeholders with
/// the value [`Sys::var`] returns. Placeholders that have no value and no
/// default become `""` (the literal `${...}` never reaches the caller) and
/// are recorded in the returned `Vec` so the caller can surface each one as
/// a warning. npm's optional `${VAR?}` form defaults to `""`, so it is never
/// recorded.
///
/// Recording each unresolved placeholder matters because leaving an
/// unresolved `${VAR}` in an auth value would later be sent as a literal
/// bearer token, notably under OIDC trusted publishing
/// (<https://github.com/pnpm/pnpm/issues/11513>).
///
/// [`Sys::var`]: EnvVar::var
#[must_use]
pub fn env_replace_lossy<Sys: EnvVar>(text: &str) -> (String, Vec<String>) {
    let mut output = String::with_capacity(text.len());
    let mut unresolved = Vec::new();
    let (placeholders, tail) = placeholders(text);
    for placeholder in placeholders {
        output.push_str(placeholder.literal());
        // The escape backslashes are re-emitted halved: each pair collapses
        // to one literal backslash.
        for _ in 0..(placeholder.backslashes() / 2) {
            output.push('\\');
        }
        if placeholder.backslashes() % 2 == 1 {
            // Odd backslashes: the placeholder is escaped, leave it literal.
            output.push_str(placeholder.raw());
        } else {
            expand_placeholder::<Sys>(placeholder, &mut output, &mut unresolved);
        }
    }
    output.push_str(tail);
    (output, unresolved)
}

/// Substitute one environment placeholder, recording a name that
/// neither the environment nor a default resolves.
fn expand_placeholder<Sys: EnvVar>(
    placeholder: Placeholder<'_>,
    output: &mut String,
    unresolved: &mut Vec<String>,
) {
    let inside = placeholder.body();
    let (var_name, default, default_on_empty) = match inside.split_once('-') {
        Some((name, default)) => match name.strip_suffix(':') {
            Some(name) => (name, Some(default), true),
            None => (name, Some(default), false),
        },
        None => match optional_var_name(inside) {
            Some(var_name) => (var_name, Some(""), false),
            None => (inside, None, false),
        },
    };
    let value = Sys::var(var_name)
        .filter(|value| !value.is_empty() || (default.is_some() && !default_on_empty));
    match (value, default) {
        (Some(value), _) => output.push_str(&value),
        (None, Some(default)) => output.push_str(default),
        (None, None) => unresolved.push(placeholder.raw().to_owned()),
    }
}

/// The `NAME` of an npm-style optional `${NAME?}` placeholder. Suffixes
/// ending in `-` are excluded so `${NAME-?}` is handled as a dash default.
fn optional_var_name(inside: &str) -> Option<&str> {
    inside
        .strip_suffix('?')
        .filter(|name| !name.is_empty())
        .filter(|name| !name.contains('?'))
        .filter(|name| !name.ends_with('-'))
}

#[cfg(test)]
mod tests;
