//! Windows `%VAR%` expansion for directory environment variables.
//!
//! Windows expands these references for `REG_EXPAND_SZ` values before a
//! process starts. A `REG_SZ` value, or a `set` that ran while the referenced
//! variable was unset, still contains the reference. pnpm would otherwise
//! create a directory whose name is that unexpanded text.

use crate::api::EnvVar;
use derive_more::{Display, Error};
use miette::Diagnostic;

/// A directory environment variable still contains a `%VAR%` reference
/// after expansion.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("{variable} contains an unexpanded environment variable: {reference}")]
#[diagnostic(
    code(ERR_PNPM_UNEXPANDED_ENV_IN_PATH),
    help("Set the referenced variable, or remove the %VAR% reference from this path.")
)]
pub struct UnexpandedWindowsEnvVar {
    pub(crate) variable: String,
    pub(crate) reference: String,
}

const DIR_ENV_WITH_FALLBACK: &[&str] = &["XDG_CACHE_HOME", "XDG_CONFIG_HOME", "XDG_STATE_HOME"];

/// Reject directory environment variables that still contain `%VAR%` on Windows.
///
/// Precedence matches the directory resolvers: `XDG_DATA_HOME` is ignored
/// when `PNPM_HOME` is set, and `LOCALAPPDATA` is read only as a Windows
/// fallback.
pub fn ensure_windows_dir_envs<Sys: EnvVar>() -> Result<(), UnexpandedWindowsEnvVar> {
    ensure_windows_dir_envs_on::<Sys>(std::env::consts::OS)
}

/// Like [`ensure_windows_dir_envs`], limited to the variables that locate
/// the pnpm home directory, for commands that resolve nothing else.
pub fn ensure_windows_home_dir_env<Sys: EnvVar>() -> Result<(), UnexpandedWindowsEnvVar> {
    ensure_windows_home_dir_env_on::<Sys>(std::env::consts::OS)
}

pub(crate) fn ensure_windows_dir_envs_on<Sys: EnvVar>(
    os: &str,
) -> Result<(), UnexpandedWindowsEnvVar> {
    ensure_windows_home_dir_env_on::<Sys>(os)?;
    if os != "windows" {
        return Ok(());
    }
    for name in DIR_ENV_WITH_FALLBACK {
        expand_if_set::<Sys>(name)?;
    }
    if DIR_ENV_WITH_FALLBACK
        .iter()
        .any(|name| Sys::var(name).is_none())
    {
        expand_if_set::<Sys>("LOCALAPPDATA")?;
    }
    Ok(())
}

pub(crate) fn ensure_windows_home_dir_env_on<Sys: EnvVar>(
    os: &str,
) -> Result<(), UnexpandedWindowsEnvVar> {
    if os != "windows" {
        return Ok(());
    }
    let name = ["PNPM_HOME", "XDG_DATA_HOME"]
        .into_iter()
        .find(|name| Sys::var(name).is_some())
        .unwrap_or("LOCALAPPDATA");
    expand_if_set::<Sys>(name)
}

/// The value of `name`, with Windows `%VAR%` references expanded.
///
/// On failure the original value is returned. [`ensure_windows_dir_envs`]
/// reports that failure before the path is used to create a directory.
pub(crate) fn read_dir_env<Sys: EnvVar>(name: &str) -> Option<String> {
    let value = Sys::var(name)?;
    // `ensure_windows_dir_envs` rejects an unexpanded reference before this path is used.
    Some(expand_dir_env(std::env::consts::OS, name, &value, Sys::var).unwrap_or(value))
}

pub(crate) fn expand_dir_env(
    os: &str,
    variable: &str,
    value: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<String, UnexpandedWindowsEnvVar> {
    if os != "windows" {
        return Ok(value.to_owned());
    }
    expand_windows_percent_vars(value, &lookup)
        .map_err(|reference| UnexpandedWindowsEnvVar { variable: variable.to_owned(), reference })
}

fn expand_if_set<Sys: EnvVar>(name: &str) -> Result<(), UnexpandedWindowsEnvVar> {
    let Some(value) = Sys::var(name) else {
        return Ok(());
    };
    expand_dir_env("windows", name, &value, Sys::var).map(|_| ())
}

const MAX_EXPANSIONS: usize = 32;
/// The longest value Windows allows in an environment variable, in UTF-16
/// code units. A self-referencing value such as `X=%X%%X%` never repeats, so
/// expansion stops once the result would exceed this length.
const MAX_EXPANDED_LEN: usize = 32_767;

fn expand_windows_percent_vars(
    value: &str,
    lookup: &impl Fn(&str) -> Option<String>,
) -> Result<String, String> {
    let mut current = value.to_owned();
    let mut seen = Vec::new();
    loop {
        if first_percent_var(&current).is_none() {
            return Ok(current);
        }
        if seen
            .iter()
            .any(|previous: &String| previous == &current)
            || seen.len() == MAX_EXPANSIONS
        {
            break;
        }
        seen.push(current.clone());
        match substitute_once(&current, lookup) {
            Some(next) => current = next,
            None => break,
        }
    }
    Err(first_percent_var(&current).expect("an unexpanded reference remains"))
}

/// Replaces every `%NAME%` whose variable is set. Returns `None` when nothing
/// was replaced or when the result would exceed [`MAX_EXPANDED_LEN`].
fn substitute_once(value: &str, lookup: &impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut next = String::with_capacity(value.len());
    let mut changed = false;
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        next.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            next.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let name = &after[..end];
        if !is_windows_env_name(name) {
            next.push('%');
            rest = after;
            continue;
        }
        if let Some(replacement) = lookup(name) {
            next.push_str(&replacement);
            changed = true;
        } else {
            next.push('%');
            next.push_str(name);
            next.push('%');
        }
        if exceeds_env_value_limit(&next) {
            return None;
        }
        rest = &after[end + 1..];
    }
    next.push_str(rest);
    (changed && !exceeds_env_value_limit(&next)).then_some(next)
}

/// Windows measures the limit in UTF-16 code units. A UTF-8 byte count is
/// never smaller, so it rules out most values without re-encoding them.
fn exceeds_env_value_limit(value: &str) -> bool {
    value.len() > MAX_EXPANDED_LEN && value.encode_utf16().count() > MAX_EXPANDED_LEN
}

fn first_percent_var(value: &str) -> Option<String> {
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        let after = &rest[start + 1..];
        let end = after.find('%')?;
        let name = &after[..end];
        if is_windows_env_name(name) {
            return Some(format!("%{name}%"));
        }
        rest = after;
    }
    None
}

/// Windows allows spaces in a name. A leading or trailing space is rejected so
/// that literal text such as `100% off 50%` is not read as a reference.
fn is_windows_env_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(' ')
        && !name.ends_with(' ')
        && name
            .chars()
            .all(|char| {
                char.is_ascii_alphanumeric() || matches!(char, '_' | '-' | '.' | '(' | ')' | ' ')
            })
}

#[cfg(test)]
mod tests;
