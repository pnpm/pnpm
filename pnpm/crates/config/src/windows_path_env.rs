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

pub(crate) fn ensure_windows_dir_envs_on<Sys: EnvVar>(
    os: &str,
) -> Result<(), UnexpandedWindowsEnvVar> {
    if os != "windows" {
        return Ok(());
    }
    expand_if_set::<Sys>("PNPM_HOME")?;
    if Sys::var("PNPM_HOME").is_none() {
        expand_if_set::<Sys>("XDG_DATA_HOME")?;
    }
    for name in DIR_ENV_WITH_FALLBACK {
        expand_if_set::<Sys>(name)?;
    }
    if local_app_data_is_used::<Sys>() {
        expand_if_set::<Sys>("LOCALAPPDATA")?;
    }
    Ok(())
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

fn local_app_data_is_used<Sys: EnvVar>() -> bool {
    let home_needs_local = Sys::var("PNPM_HOME").is_none() && Sys::var("XDG_DATA_HOME").is_none();
    let fallback_needs_local = DIR_ENV_WITH_FALLBACK
        .iter()
        .any(|name| Sys::var(name).is_none());
    home_needs_local || fallback_needs_local
}

const MAX_EXPANSIONS: usize = 32;

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
        let (next, changed) = substitute_once(&current, lookup);
        if !changed {
            break;
        }
        current = next;
    }
    Err(first_percent_var(&current).expect("an unexpanded reference remains"))
}

fn substitute_once(value: &str, lookup: &impl Fn(&str) -> Option<String>) -> (String, bool) {
    let mut next = String::with_capacity(value.len());
    let mut changed = false;
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        next.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            next.push_str(&rest[start..]);
            return (next, changed);
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
        rest = &after[end + 1..];
    }
    next.push_str(rest);
    (next, changed)
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

fn is_windows_env_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '(' | ')'))
}

#[cfg(test)]
mod tests {
    use super::{UnexpandedWindowsEnvVar, ensure_windows_dir_envs_on, expand_dir_env};
    use crate::api::EnvVar;
    use pretty_assertions::assert_eq;

    fn lookup(name: &str) -> Option<String> {
        match name {
            "SOME_ENV" => Some(r"C:\tools".to_owned()),
            "OUTER" => Some(r"%INNER%\apps".to_owned()),
            "INNER" => Some(r"D:\dev".to_owned()),
            "LOCAL_ROOT" => Some(r"C:\Users\me".to_owned()),
            "ProgramFiles(x86)" => Some(r"C:\Program Files (x86)".to_owned()),
            "A" => Some("%B%".to_owned()),
            "B" => Some("%A%".to_owned()),
            _ => None,
        }
    }

    fn expand(value: &str) -> Result<String, UnexpandedWindowsEnvVar> {
        expand_dir_env("windows", "PNPM_HOME", value, lookup)
    }

    #[test]
    fn expands_a_nested_windows_reference() {
        assert_eq!(expand("%SOME_ENV%/pnpm").unwrap(), r"C:\tools/pnpm");
        assert_eq!(expand(r"%OUTER%\pnpm").unwrap(), r"D:\dev\apps\pnpm");
    }

    #[test]
    fn expands_program_files_x86() {
        assert_eq!(expand("%ProgramFiles(x86)%\\pnpm").unwrap(), r"C:\Program Files (x86)\pnpm",);
    }

    #[test]
    fn rejects_a_reference_that_stays_unexpanded() {
        let error = expand("%MISSING%/pnpm").unwrap_err();
        assert_eq!(error.variable, "PNPM_HOME");
        assert_eq!(error.reference, "%MISSING%");
    }

    #[test]
    fn rejects_a_cycle() {
        let error = expand("%A%").unwrap_err();
        assert_eq!(error.reference, "%A%");
    }

    #[test]
    fn leaves_a_literal_percent_in_place() {
        assert_eq!(expand(r"C:\100%\pnpm").unwrap(), r"C:\100%\pnpm");
        assert_eq!(expand("100%%").unwrap(), "100%%");
    }

    #[test]
    fn does_not_expand_off_windows() {
        assert_eq!(
            expand_dir_env("linux", "PNPM_HOME", "%SOME_ENV%/pnpm", lookup).unwrap(),
            "%SOME_ENV%/pnpm",
        );
    }

    struct DirEnv;

    impl EnvVar for DirEnv {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_HOME" => Some("%SOME_ENV%/pnpm".to_owned()),
                "SOME_ENV" => Some(r"C:\tools".to_owned()),
                "XDG_DATA_HOME" => Some("%MISSING%\\data".to_owned()),
                _ => None,
            }
        }
    }

    #[test]
    fn ignores_an_unused_data_home_when_pnpm_home_expands() {
        ensure_windows_dir_envs_on::<DirEnv>("windows").unwrap();
    }

    struct BadCache;

    impl EnvVar for BadCache {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_HOME" => Some(r"C:\pnpm".to_owned()),
                "XDG_CACHE_HOME" => Some("%MISSING%\\cache".to_owned()),
                _ => None,
            }
        }
    }

    #[test]
    fn rejects_an_unexpanded_cache_home() {
        let error = ensure_windows_dir_envs_on::<BadCache>("windows").unwrap_err();
        assert_eq!(error.variable, "XDG_CACHE_HOME");
        assert_eq!(error.reference, "%MISSING%");
    }

    #[test]
    fn skips_the_check_off_windows() {
        ensure_windows_dir_envs_on::<BadCache>("linux").unwrap();
    }
}
