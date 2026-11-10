//! Extend the user `Path` (and a proxy variable like `PNPM_HOME`) in the
//! Windows registry under `HKEY_CURRENT_USER\Environment`.
//!
//! The registry is read with `reg query`, the proxy variable and `Path`
//! are written with `reg add`, and a dummy `setx` forces the new values to
//! be picked up by future processes. Switching the console to code page
//! 65001 makes `reg` emit UTF-8 so non-ASCII values survive the round-trip.

use super::{AddDirToEnvPathOpts, AddingPosition, PathExtenderError};
use crate::process::Command;
use std::path::Path;

/// The change made to one environment variable, used to render the
/// before/after report.
#[derive(Debug)]
pub(super) struct EnvVariableChange {
    pub variable: String,
    pub old_value: Option<String>,
    pub new_value: String,
}

const REG_KEY: &str = r"HKEY_CURRENT_USER\Environment";

const UTF8_CODEPAGE: u32 = 65001;

pub(super) fn add_dir_to_windows_env_path(
    dir: &Path,
    opts: &AddDirToEnvPathOpts,
) -> Result<Vec<EnvVariableChange>, PathExtenderError> {
    // The UTF-8 code page makes `reg` use UTF-8 for output. Otherwise
    // non-ASCII characters in environment variables become garbled. The code
    // page is switched with `mode con cp select=` rather than `chcp`: both
    // set the same console code page, but `chcp` clears the visible terminal
    // whenever it changes it.
    let codepage_output =
        query_codepage().map_err(|err| PathExtenderError::CodePage { message: err.to_string() })?;
    let cp_bak = first_number(&codepage_output)
        .ok_or_else(|| PathExtenderError::CodePage { message: codepage_output.clone() })?;
    select_codepage(UTF8_CODEPAGE)?;

    let result = (|| {
        let report = add_dir_to_windows_env_path_inner(dir, opts)?;
        refresh_env_vars()?;
        Ok(report)
    })();

    // Restore the saved code page even when the body failed.
    let _ = select_codepage(cp_bak);
    result
}

/// Print the console's code page. The output is a device status report whose
/// first number is the code page:
///
/// ```text
/// Status for device CON:
/// ----------------------
///     Code page:      437
/// ```
fn query_codepage() -> Result<String, PathExtenderError> {
    run_capture_mode(&["con", "cp"])
}

/// Switch the console's code page. The output is a longer status report that
/// starts with the window size, so it is not parsed for the code page.
fn select_codepage(codepage: u32) -> Result<(), PathExtenderError> {
    run_capture_mode(&["con", "cp", &format!("select={codepage}")]).map(drop)
}

fn add_dir_to_windows_env_path_inner(
    dir: &Path,
    opts: &AddDirToEnvPathOpts,
) -> Result<Vec<EnvVariableChange>, PathExtenderError> {
    // Defense in depth: `handler` validates before any side effect; re-check
    // before writing anything to the registry.
    super::validate_windows_pnpm_home(dir)?;
    let added_dir = dir.to_string_lossy().replace('/', r"\");
    let registry_output = get_registry_output()?;
    let mut changes = Vec::new();
    if let Some(proxy) = opts.proxy_var_name {
        changes.push(update_env_variable(
            &registry_output,
            proxy,
            &added_dir,
            false,
            opts.overwrite,
        )?);
        let path_entry = match opts.proxy_var_sub_dir {
            Some(sub_dir) => format!(r"%{proxy}%\{sub_dir}"),
            None => format!("%{proxy}%"),
        };
        changes.push(add_to_path(&registry_output, &path_entry, opts.position)?);
    } else {
        changes.push(add_to_path(&registry_output, &added_dir, opts.position)?);
    }
    Ok(changes)
}

fn update_env_variable(
    registry_output: &str,
    name: &str,
    value: &str,
    expandable_string: bool,
    overwrite: bool,
) -> Result<EnvVariableChange, PathExtenderError> {
    update_env_variable_with(
        registry_output,
        name,
        value,
        expandable_string,
        overwrite,
        set_env_var_in_registry,
    )
}

fn update_env_variable_with<Setter>(
    registry_output: &str,
    name: &str,
    value: &str,
    expandable_string: bool,
    overwrite: bool,
    mut setter: Setter,
) -> Result<EnvVariableChange, PathExtenderError>
where
    Setter: FnMut(&str, &str, bool) -> Result<(), PathExtenderError>,
{
    let current = get_env_value_with_type_from_registry(registry_output, name);
    let current_value = current.as_ref().map(|entry| entry.data.clone());

    if let Some(current) = &current
        && !overwrite
    {
        if current.data != value {
            return Err(PathExtenderError::BadEnvFound {
                env_name: name.to_string(),
                current_value: current.data.clone(),
            });
        }
        if current.value_type.eq_ignore_ascii_case(registry_value_type(expandable_string)) {
            return Ok(EnvVariableChange {
                variable: name.to_string(),
                old_value: current_value,
                new_value: value.to_string(),
            });
        }
    }

    setter(name, value, expandable_string)?;
    Ok(EnvVariableChange {
        variable: name.to_string(),
        old_value: current_value,
        new_value: value.to_string(),
    })
}

fn add_to_path(
    registry_output: &str,
    added_dir: &str,
    position: AddingPosition,
) -> Result<EnvVariableChange, PathExtenderError> {
    let variable = "Path";
    let path_data = get_env_value_from_registry(registry_output, variable);
    let path_data = match path_data {
        Some(data) if !data.trim().is_empty() => data,
        _ => return Err(PathExtenderError::NoPath),
    };
    if path_data
        .split(';')
        .any(|entry| entry == added_dir)
    {
        return Ok(EnvVariableChange {
            variable: variable.to_string(),
            old_value: Some(path_data.clone()),
            new_value: path_data,
        });
    }
    let new_path_value = match position {
        AddingPosition::Start => format!("{added_dir};{path_data}"),
        AddingPosition::End => format!("{path_data};{added_dir}"),
    };
    set_env_var_in_registry("Path", &new_path_value, true)?;
    Ok(EnvVariableChange {
        variable: variable.to_string(),
        old_value: Some(path_data),
        new_value: new_path_value,
    })
}

/// Read every value under [`REG_KEY`] and pick the one we need, rather than
/// querying a single value (which fails when the value is absent and hides
/// the real cause).
fn get_registry_output() -> Result<String, PathExtenderError> {
    run_capture("reg", &["query", REG_KEY]).map_err(|_| PathExtenderError::RegRead)
}

/// Run a command and capture stdout, returning an error if it cannot be
/// spawned or exits non-zero — rather than silently continuing with empty
/// output. `mode` reports its errors on stdout, so a failure without stderr
/// is reported with stdout instead.
fn run_capture(program: &str, args: &[&str]) -> Result<String, PathExtenderError> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message =
            if stderr.trim().is_empty() { String::from_utf8_lossy(&output.stdout) } else { stderr };
        return Err(PathExtenderError::CommandFailed {
            command: program.to_string(),
            message: message.trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Run `mode`, attempting `mode.com` first (the standard executable name on
/// Windows) with a fallback to `mode` if `mode.com` is not found.
fn run_capture_mode(args: &[&str]) -> Result<String, PathExtenderError> {
    run_capture_mode_with(args, run_capture)
}

fn run_capture_mode_with<Runner>(
    args: &[&str],
    mut runner: Runner,
) -> Result<String, PathExtenderError>
where
    Runner: FnMut(&str, &[&str]) -> Result<String, PathExtenderError>,
{
    match runner("mode.com", args) {
        Err(PathExtenderError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            runner("mode", args)
        }
        res => res,
    }
}

#[derive(Debug)]
struct RegistryEnvValue {
    value_type: String,
    data: String,
}

fn get_env_value_from_registry(registry_output: &str, env_var_name: &str) -> Option<String> {
    get_env_value_with_type_from_registry(registry_output, env_var_name).map(|entry| entry.data)
}

fn get_env_value_with_type_from_registry(
    registry_output: &str,
    env_var_name: &str,
) -> Option<RegistryEnvValue> {
    registry_output
        .lines()
        .find_map(|line| env_value_with_type_from_registry_line(line, env_var_name))
}

/// Parse a `reg query` line of the form `    <name>    <type>    <data>`
/// (four-space separators), matching `name` case-insensitively.
fn env_value_with_type_from_registry_line(
    line: &str,
    env_var_name: &str,
) -> Option<RegistryEnvValue> {
    let rest = line.strip_prefix("    ")?;
    // The length of the name we want is not necessarily a char boundary of
    // this line: an unrelated name can hold a multi-byte character that ends
    // past it. Slicing there panics, so take the candidate as a whole.
    let name = rest.get(..env_var_name.len())?;
    if !name.eq_ignore_ascii_case(env_var_name) {
        return None;
    }
    let after_name = rest[name.len()..].strip_prefix("    ")?;
    let type_end = after_name.find("    ")?;
    let value_type = &after_name[..type_end];
    if value_type.is_empty()
        || !value_type
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_')
    {
        return None;
    }
    Some(RegistryEnvValue {
        value_type: value_type.to_string(),
        data: after_name[type_end + 4..].to_string(),
    })
}

fn registry_value_type(expandable_string: bool) -> &'static str {
    if expandable_string { "REG_EXPAND_SZ" } else { "REG_SZ" }
}

fn set_env_var_in_registry(
    env_var_name: &str,
    env_var_value: &str,
    expandable_string: bool,
) -> Result<(), PathExtenderError> {
    let reg_type = registry_value_type(expandable_string);
    let output = Command::new("reg")
        .args(["add", REG_KEY, "/v", env_var_name, "/t", reg_type, "/d", env_var_value, "/f"])
        .output()?;
    if !output.status.success() {
        return Err(PathExtenderError::FailedSetEnv {
            env_name: env_var_name.to_string(),
            value: env_var_value.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

/// Registry writes are not seen by future processes until at least one
/// variable is set with `setx`. Set and immediately delete a throwaway
/// variable to trigger the broadcast.
fn refresh_env_vars() -> Result<(), PathExtenderError> {
    const TEMP_ENV_VAR: &str = "REFRESH_ENV_VARS";
    run_capture("setx", &[TEMP_ENV_VAR, "1"])?;
    run_capture("reg", &["delete", REG_KEY, "/v", TEMP_ENV_VAR, "/f"])?;
    Ok(())
}

fn first_number(text: &str) -> Option<u32> {
    let digits: String = text
        .chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests;
