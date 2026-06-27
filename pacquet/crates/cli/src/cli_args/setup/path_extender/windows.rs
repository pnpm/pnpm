//! Extend the user `Path` (and a proxy variable like `PNPM_HOME`) in the
//! Windows registry under `HKEY_CURRENT_USER\Environment`.
//!
//! Ports pnpm's
//! [`@pnpm/os.env.path-extender-windows`](https://github.com/pnpm/pnpm/blob/1819226b51/packages/path-extender-windows/src/path-extender-windows.ts):
//! the registry is read with `reg query`, the proxy variable and `Path` are
//! written with `reg add`, and a dummy `setx` forces the new values to be
//! picked up by future processes. `chcp 65001` makes `reg` emit UTF-8 so
//! non-ASCII values survive the round-trip.

use super::{AddDirToEnvPathOpts, AddingPosition, PathExtenderError};
use std::{path::Path, process::Command};

/// The change made to one environment variable, used to render the
/// before/after report. Mirrors pnpm's `EnvVariableChange`.
#[derive(Debug)]
pub(super) struct EnvVariableChange {
    pub variable: String,
    pub old_value: Option<String>,
    pub new_value: String,
}

const REG_KEY: &str = r"HKEY_CURRENT_USER\Environment";

pub(super) fn add_dir_to_windows_env_path(
    dir: &Path,
    opts: &AddDirToEnvPathOpts,
) -> Result<Vec<EnvVariableChange>, PathExtenderError> {
    // `chcp` makes `reg` use UTF-8 for output. Otherwise non-ASCII
    // characters in environment variables become garbled.
    let chcp_output = run_capture("chcp", &[])
        .map_err(|err| PathExtenderError::Chcp { message: err.to_string() })?;
    let cp_bak = first_number(&chcp_output)
        .ok_or_else(|| PathExtenderError::Chcp { message: chcp_output.clone() })?;
    run_capture("chcp", &["65001"])?;

    let result = (|| {
        let report = add_dir_to_windows_env_path_inner(dir, opts)?;
        refresh_env_vars()?;
        Ok(report)
    })();

    // Restore the original code page even when the body failed.
    let _ = run_capture("chcp", &[&cp_bak.to_string()]);
    result
}

fn add_dir_to_windows_env_path_inner(
    dir: &Path,
    opts: &AddDirToEnvPathOpts,
) -> Result<Vec<EnvVariableChange>, PathExtenderError> {
    let added_dir = dir.to_string_lossy().replace('/', r"\");
    // Reject characters that would split the persisted `Path` into extra
    // entries (`;`), break the `%PNPM_HOME%` indirection (`%`), or corrupt
    // the value (`\n` / `\r`). See `PathExtenderError::UnsafePnpmHomeForWindows`.
    if let Some(character) =
        added_dir.chars().find(|character| matches!(character, ';' | '%' | '\n' | '\r'))
    {
        return Err(PathExtenderError::UnsafePnpmHome { dir: added_dir, character });
    }
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
            Some(sub_dir) => format!("%{proxy}%\\{sub_dir}"),
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
    let current_value = get_env_value_from_registry(registry_output, name);
    match &current_value {
        Some(current) if !overwrite => {
            if current != value {
                return Err(PathExtenderError::BadEnvFound {
                    env_name: name.to_string(),
                    wanted_value: value.to_string(),
                });
            }
            Ok(EnvVariableChange {
                variable: name.to_string(),
                old_value: current_value,
                new_value: value.to_string(),
            })
        }
        _ => {
            set_env_var_in_registry(name, value, expandable_string)?;
            Ok(EnvVariableChange {
                variable: name.to_string(),
                old_value: current_value,
                new_value: value.to_string(),
            })
        }
    }
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
    if path_data.split(';').any(|entry| entry == added_dir) {
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

/// Read every value under `REG_KEY` and pick the one we need, rather than
/// querying a single value (which fails when the value is absent and hides
/// the real cause). Mirrors pnpm's `getRegistryOutput`.
fn get_registry_output() -> Result<String, PathExtenderError> {
    run_capture("reg", &["query", REG_KEY]).map_err(|_| PathExtenderError::RegRead)
}

/// Run a command and capture stdout, returning an error if it cannot be
/// spawned or exits non-zero. Mirrors pnpm's `safe-execa`, which rejects on
/// a non-zero exit rather than silently continuing with empty output.
fn run_capture(program: &str, args: &[&str]) -> Result<String, PathExtenderError> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(PathExtenderError::CommandFailed {
            command: program.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parse a `reg query` line of the form `    <name>    <type>    <data>`
/// (four-space separators), matching `name` case-insensitively. Mirrors
/// pnpm's `getEnvValueFromRegistry` regex.
fn get_env_value_from_registry(registry_output: &str, env_var_name: &str) -> Option<String> {
    for line in registry_output.lines() {
        let Some(rest) = line.strip_prefix("    ") else {
            continue;
        };
        if rest.len() < env_var_name.len()
            || !rest[..env_var_name.len()].eq_ignore_ascii_case(env_var_name)
        {
            continue;
        }
        let Some(after_name) = rest[env_var_name.len()..].strip_prefix("    ") else {
            continue;
        };
        let Some(type_end) = after_name.find("    ") else {
            continue;
        };
        let value_type = &after_name[..type_end];
        if value_type.is_empty() || !value_type.chars().all(|ch| ch.is_alphanumeric() || ch == '_')
        {
            continue;
        }
        return Some(after_name[type_end + 4..].to_string());
    }
    None
}

fn set_env_var_in_registry(
    env_var_name: &str,
    env_var_value: &str,
    expandable_string: bool,
) -> Result<(), PathExtenderError> {
    let reg_type = if expandable_string { "REG_EXPAND_SZ" } else { "REG_SZ" };
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
/// variable to trigger the broadcast. Mirrors pnpm's `refreshEnvVars`.
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
