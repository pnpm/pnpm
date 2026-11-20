use crate::{
    GitFetcherError,
    fetcher::exec_git_with,
};
use std::{
    collections::HashMap,
    env,
    ffi::OsStr,
    path::Path,
};

/// Git transports permitted for dependency fetching. Helper transports
/// can execute commands selected by repository metadata.
pub const SUPPORTED_GIT_PROTOCOLS: [&str; 5] = ["file", "git", "http", "https", "ssh"];

pub(crate) fn read_protocol_policies(
    bin: &Path,
    cwd: Option<&Path>,
) -> Result<HashMap<String, String>, GitFetcherError> {
    let output =
        match exec_git_with(bin, &["config", "--null", "--get-regexp", r"^protocol\."], cwd) {
            Ok(output) => output,
            Err(GitFetcherError::GitExec { operation: "config", status, stderr })
                if status.code() == Some(1) && stderr.is_empty() =>
            {
                return Ok(HashMap::new());
            }
            Err(error) => return Err(error),
        };
    Ok(output
        .split('\0')
        .filter_map(|entry| entry.split_once('\n'))
        .filter(|(key, _)| key.ends_with(".allow"))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect())
}

/// Read the supported protocols permitted by Git configuration and the
/// caller's environment for a top-level fetch. The returned allowlist can
/// constrain nested Git processes without enabling a configured ban.
/// Without Git on PATH, return an empty allowlist: non-Git resolution can
/// proceed, and Git started through a configured PATH stays blocked.
pub fn read_allowed_git_protocols(cwd: &Path) -> Result<String, GitFetcherError> {
    read_allowed_git_protocols_with(Path::new("git"), cwd)
}

pub(crate) fn read_allowed_git_protocols_with(
    bin: &Path,
    cwd: &Path,
) -> Result<String, GitFetcherError> {
    let policies = match read_protocol_policies(bin, Some(cwd)) {
        Ok(policies) => policies,
        Err(GitFetcherError::GitNotFound) => return Ok(String::new()),
        Err(error) => return Err(error),
    };
    let inherited = env::var_os("GIT_ALLOW_PROTOCOL");
    let from_user = env::var_os("GIT_PROTOCOL_FROM_USER")
        .map(|value| read_git_boolean(bin, &value, cwd))
        .transpose()?
        .unwrap_or(true);
    Ok(filter_supported_protocols(inherited.as_deref(), |protocol| {
        protocol_enabled(protocol, &policies, from_user)
    }))
}

fn read_git_boolean(bin: &Path, value: &OsStr, cwd: &Path) -> Result<bool, GitFetcherError> {
    let setting = format!("pnpm.protocol-from-user={}", value.to_string_lossy());
    exec_git_with(
        bin,
        &["-c", &setting, "config", "--type=bool", "--get", "pnpm.protocol-from-user"],
        Some(cwd),
    )
    .map(|value| value.trim() == "true")
}

pub(crate) fn submodule_protocols(
    inherited: Option<&OsStr>,
    policies: &HashMap<String, String>,
) -> String {
    filter_supported_protocols(inherited, |protocol| protocol_enabled(protocol, policies, false))
}

fn filter_supported_protocols(inherited: Option<&OsStr>, enabled: impl Fn(&str) -> bool) -> String {
    SUPPORTED_GIT_PROTOCOLS
        .into_iter()
        .filter(|protocol| enabled(protocol))
        .filter(|protocol| {
            inherited.is_none_or(|allowed| {
                allowed
                    .as_encoded_bytes()
                    .split(|byte| *byte == b':')
                    .any(|candidate| candidate == protocol.as_bytes())
            })
        })
        .collect::<Vec<_>>()
        .join(":")
}

fn protocol_enabled(protocol: &str, policies: &HashMap<String, String>, from_user: bool) -> bool {
    let default = if protocol == "file" { "user" } else { "always" };
    let policy = policies
        .get(&format!("protocol.{protocol}.allow"))
        .or_else(|| policies.get("protocol.allow"))
        .map_or(default, String::as_str);
    policy.eq_ignore_ascii_case("always") || from_user && policy.eq_ignore_ascii_case("user")
}

#[cfg(test)]
mod tests;
