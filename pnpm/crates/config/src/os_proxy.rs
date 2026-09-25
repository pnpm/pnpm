//! Operating-system proxy settings, the last source in the proxy cascade.
//!
//! Windows Internet Settings and the macOS system proxy fill a slot only
//! when no config layer and no environment variable named it. Linux has no
//! standard system proxy, so discovery there is empty.

#[cfg(all(not(test), any(windows, target_os = "macos")))]
use std::process::Command;

/// Proxy URLs and bypass list read from the operating system.
///
/// Strings are schemeless `host:port` when the system settings store them
/// that way. The network layer prefixes `http://` at parse time.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct OsProxy {
    pub https: Option<String>,
    pub http: Option<String>,
    pub bypass: Option<String>,
}

/// Read the operating-system proxy, or an empty value when none is set.
///
/// Unit tests always get an empty value so cascade tests do not depend on
/// the machine running them. A missing `reg` or `scutil`, or a command
/// that fails, is also an empty value: proxy discovery must not fail an
/// install.
#[must_use]
pub(crate) fn discover_os_proxy() -> OsProxy {
    #[cfg(test)]
    {
        OsProxy::default()
    }
    #[cfg(not(test))]
    {
        read_os_proxy()
    }
}

#[cfg(not(test))]
fn read_os_proxy() -> OsProxy {
    #[cfg(windows)]
    {
        command_stdout(
            "reg",
            &[
                "query",
                r"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            ],
        )
        .map(|output| parse_windows_internet_settings(&output))
        .unwrap_or_default()
    }
    #[cfg(target_os = "macos")]
    {
        command_stdout("scutil", &["--proxy"])
            .map(|output| parse_macos_scutil_proxy(&output))
            .unwrap_or_default()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        OsProxy::default()
    }
}

#[cfg(all(not(test), any(windows, target_os = "macos")))]
fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parse `reg query` output for the Internet Settings key.
///
/// `ProxyEnable` of `0` or a missing value means no proxy, even when
/// `ProxyServer` is present. A server value without `=` applies to both
/// schemes. `http=` and `https=` entries are read on their own; other
/// schemes are ignored.
#[cfg(any(test, windows))]
#[must_use]
pub(crate) fn parse_windows_internet_settings(output: &str) -> OsProxy {
    if registry_dword(output, "ProxyEnable").unwrap_or(0) == 0 {
        return OsProxy::default();
    }
    let (http, https) = split_proxy_server(
        registry_value(output, "ProxyServer")
            .as_deref()
            .unwrap_or("")
            .trim(),
    );
    let bypass = registry_value(output, "ProxyOverride")
        .map(|raw| join_bypass_entries(raw.split(';')))
        .filter(|value| !value.is_empty());
    OsProxy { https, http, bypass }
}

/// Parse `scutil --proxy` output.
///
/// A scheme is used only when its enable flag is non-zero and a host is
/// present. `ExceptionsList` becomes the bypass list.
#[cfg(any(test, target_os = "macos"))]
#[must_use]
pub(crate) fn parse_macos_scutil_proxy(output: &str) -> OsProxy {
    let mut http_enable = false;
    let mut https_enable = false;
    let mut http_host = None;
    let mut https_host = None;
    let mut http_port = None;
    let mut https_port = None;
    let mut exceptions = Vec::new();
    let mut in_exceptions = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if in_exceptions {
            if trimmed == "}" {
                in_exceptions = false;
                continue;
            }
            if let Some((_, value)) = trimmed.split_once(" : ")
                && let Some(entry) = normalize_bypass_entry(value.trim())
            {
                exceptions.push(entry);
            }
            continue;
        }
        if trimmed.starts_with("ExceptionsList") && trimmed.contains("<array>") {
            in_exceptions = true;
            continue;
        }
        let Some((key, value)) = trimmed.split_once(" : ") else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "HTTPEnable" => http_enable = flag_enabled(value),
            "HTTPSEnable" => https_enable = flag_enabled(value),
            "HTTPProxy" => http_host = non_empty(value),
            "HTTPSProxy" => https_host = non_empty(value),
            "HTTPPort" => http_port = positive_port(value),
            "HTTPSPort" => https_port = positive_port(value),
            _ => {}
        }
    }

    let bypass = (!exceptions.is_empty()).then(|| exceptions.join(","));
    OsProxy {
        http: enabled_endpoint(http_enable, http_host, http_port),
        https: enabled_endpoint(https_enable, https_host, https_port),
        bypass,
    }
}

#[cfg(any(test, windows))]
fn registry_value(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|line| registry_line_value(line, name))
}

/// A `reg query` line is `    <name>    <type>    <data>` with four-space separators.
#[cfg(any(test, windows))]
fn registry_line_value(line: &str, name: &str) -> Option<String> {
    let rest = line.strip_prefix("    ")?;
    if rest.len() < name.len() || !rest[..name.len()].eq_ignore_ascii_case(name) {
        return None;
    }
    let after_name = rest[name.len()..].strip_prefix("    ")?;
    let type_end = after_name.find("    ")?;
    Some(after_name[type_end + 4..].trim().to_string())
}

#[cfg(any(test, windows))]
fn registry_dword(output: &str, name: &str) -> Option<u32> {
    let raw = registry_value(output, name)?;
    let hex = raw.strip_prefix("0x").unwrap_or(&raw);
    u32::from_str_radix(hex, 16).ok()
}

#[cfg(any(test, windows))]
fn split_proxy_server(server: &str) -> (Option<String>, Option<String>) {
    if server.is_empty() {
        return (None, None);
    }
    if !server.contains('=') {
        let url = Some(server.to_string());
        return (url.clone(), url);
    }
    let mut http_proxy = None;
    let mut https_proxy = None;
    for entry in server.split(';') {
        let Some((scheme, value)) = entry.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match scheme
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "http" => http_proxy = Some(value.to_string()),
            "https" => https_proxy = Some(value.to_string()),
            _ => {}
        }
    }
    (http_proxy, https_proxy)
}

#[cfg(any(test, windows))]
fn join_bypass_entries<'a>(entries: impl Iterator<Item = &'a str>) -> String {
    entries
        .filter_map(normalize_bypass_entry)
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(any(test, windows, target_os = "macos"))]
fn normalize_bypass_entry(entry: &str) -> Option<String> {
    let entry = entry.trim();
    if entry.is_empty() || entry.eq_ignore_ascii_case("<local>") {
        return None;
    }
    let stripped = entry.strip_prefix("*.").unwrap_or(entry);
    (!stripped.is_empty()).then(|| stripped.to_string())
}

#[cfg(any(test, target_os = "macos"))]
fn flag_enabled(value: &str) -> bool {
    !value.is_empty() && value != "0"
}

#[cfg(any(test, target_os = "macos"))]
fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(any(test, target_os = "macos"))]
fn positive_port(value: &str) -> Option<u16> {
    let port = value.parse::<u16>().ok()?;
    (port > 0).then_some(port)
}

#[cfg(any(test, target_os = "macos"))]
fn enabled_endpoint(enabled: bool, host: Option<String>, port: Option<u16>) -> Option<String> {
    if !enabled {
        return None;
    }
    let host = host?;
    Some(match port {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

#[cfg(test)]
mod tests;
