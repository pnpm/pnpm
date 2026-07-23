//! Checks a package's wanted `os` / `cpu` / `libc` against the host.

use derive_more::{Display, Error};
use miette::Diagnostic;
use serde::{Deserialize, Serialize};

/// Caller-supplied override for the `os` / `cpu` / `libc` triples
/// against which a package's wanted platform is evaluated. Each list
/// defaults to `['current']` at the call site (read from the config
/// setting, falling back to `['current']` if absent). The `'current'`
/// sentinel is compared as the concrete host triple.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportedArchitectures {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub libc: Option<Vec<String>>,
}

/// Wanted platform triple as declared by a package's manifest
/// (`os`, `cpu`, `libc`). Each is optional; absent means "any".
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct WantedPlatform {
    pub os: Option<Vec<String>>,
    pub cpu: Option<Vec<String>>,
    pub libc: Option<Vec<String>>,
}

/// Borrow-only view of [`WantedPlatform`] used by [`check_platform`]
/// on the install hot path. Lets the caller pass `manifest.os.as_deref()`
/// (etc.) directly without cloning the manifest's owned `Vec<String>`s
/// into a fresh [`WantedPlatform`] per snapshot. The owned form is
/// only materialised inside [`check_platform`] when an error is
/// produced (for diagnostic display).
///
/// `Copy` so the recursive / per-snapshot call sites don't need an
/// extra reference layer; all three fields are already references.
#[derive(Debug, Default, Clone, Copy)]
pub struct WantedPlatformRef<'a> {
    pub os: Option<&'a [String]>,
    pub cpu: Option<&'a [String]>,
    pub libc: Option<&'a [String]>,
}

/// Current host platform triple, as reported (or overridden by
/// `supported_architectures`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Platform {
    pub os: Vec<String>,
    pub cpu: Vec<String>,
    pub libc: Vec<String>,
}

/// Error returned by [`check_platform`] when no entry in the host
/// triple satisfies a wanted list. Wire-compatible with pnpm's
/// `ERR_PNPM_UNSUPPORTED_PLATFORM` (same code, same message shape).
#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
#[display("Unsupported platform for {package_id}: wanted {wanted_json} (current: {current_json})")]
#[diagnostic(code(ERR_PNPM_UNSUPPORTED_PLATFORM))]
pub struct UnsupportedPlatformError {
    pub package_id: String,
    pub wanted: WantedPlatform,
    pub current: Platform,
    wanted_json: String,
    current_json: String,
}

impl UnsupportedPlatformError {
    fn new(package_id: String, wanted: WantedPlatform, current: Platform) -> Self {
        let wanted_json = wanted_json(&wanted);
        let current_json = current_json(&current);
        Self { package_id, wanted, current, wanted_json, current_json }
    }
}

fn wanted_json(wanted: &WantedPlatform) -> String {
    // Match the `JSON.stringify(wanted)` shape: only the fields
    // actually set appear, and each is a JSON array.
    let mut parts = Vec::new();
    if let Some(os) = &wanted.os {
        parts.push(format!(r#""os":{}"#, json_string_array(os)));
    }
    if let Some(cpu) = &wanted.cpu {
        parts.push(format!(r#""cpu":{}"#, json_string_array(cpu)));
    }
    if let Some(libc) = &wanted.libc {
        parts.push(format!(r#""libc":{}"#, json_string_array(libc)));
    }
    format!("{{{}}}", parts.join(","))
}

fn current_json(current: &Platform) -> String {
    // The current platform is `{ os, cpu, libc }` with single strings,
    // not arrays.
    fn single(values: &[String]) -> String {
        values.first().cloned().unwrap_or_default()
    }
    format!(
        r#"{{"os":{:?},"cpu":{:?},"libc":{:?}}}"#,
        single(&current.os),
        single(&current.cpu),
        single(&current.libc),
    )
}

fn json_string_array(values: &[String]) -> String {
    let joined: Vec<String> = values.iter().map(|s| format!("{s:?}")).collect();
    format!("[{}]", joined.join(","))
}

/// Evaluate a package's `os` / `cpu` / `libc` against the host.
///
/// Returns `None` when the package is compatible, or
/// `Some(UnsupportedPlatformError)` when any constraint rejects the
/// host. Negation entries (`!foo`) and the special `any` sentinel are
/// honored.
///
/// The wanted axes are taken as `Option<&[String]>` slices so the
/// hot path doesn't allocate a [`WantedPlatform`] per snapshot —
/// callers can pass `manifest.os.as_deref()` directly. The owned
/// [`WantedPlatform`] form is only built when an error is returned
/// (for diagnostic display via the
/// [`UnsupportedPlatformError`]).
///
/// `supported_architectures` substitutes for `['current']` per axis.
///
/// `current_os`, `current_cpu`, and `current_libc` are passed in
/// rather than read from the environment so this function stays
/// trivially testable.
pub fn check_platform(
    package_id: &str,
    wanted: WantedPlatformRef<'_>,
    supported: Option<&SupportedArchitectures>,
    current_os: &str,
    current_cpu: &str,
    current_libc: &str,
) -> Option<UnsupportedPlatformError> {
    if platform_is_supported(wanted, supported, current_os, current_cpu, current_libc) {
        return None;
    }

    let owned_wanted = WantedPlatform {
        os: wanted.os.map(<[String]>::to_vec),
        cpu: wanted.cpu.map(<[String]>::to_vec),
        libc: wanted.libc.map(<[String]>::to_vec),
    };
    let real_current = Platform {
        os: vec![current_os.to_string()],
        cpu: vec![current_cpu.to_string()],
        libc: vec![current_libc.to_string()],
    };
    Some(UnsupportedPlatformError::new(package_id.to_string(), owned_wanted, real_current))
}

#[must_use]
pub fn platform_is_supported(
    wanted: WantedPlatformRef<'_>,
    supported: Option<&SupportedArchitectures>,
    current_os: &str,
    current_cpu: &str,
    current_libc: &str,
) -> bool {
    wanted.os.is_none_or(|wanted_os| {
        axis_is_supported(
            current_os,
            supported.and_then(|supported| supported.os.as_deref()),
            wanted_os,
        )
    }) && wanted.cpu.is_none_or(|wanted_cpu| {
        axis_is_supported(
            current_cpu,
            supported.and_then(|supported| supported.cpu.as_deref()),
            wanted_cpu,
        )
    }) && wanted.libc.is_none_or(|wanted_libc| {
        current_libc == "unknown"
            || axis_is_supported(
                current_libc,
                supported.and_then(|supported| supported.libc.as_deref()),
                wanted_libc,
            )
    })
}

fn axis_is_supported(current: &str, supported: Option<&[String]>, wanted: &[String]) -> bool {
    if wanted.len() == 1 && wanted[0] == "any" {
        return true;
    }

    let mut matched = false;
    if let Some(supported) = supported {
        for value in supported {
            match platform_value_match(if value == "current" { current } else { value }, wanted) {
                PlatformValueMatch::Rejected => return false,
                PlatformValueMatch::Matched => matched = true,
                PlatformValueMatch::NoMatch => {}
            }
        }
    } else {
        match platform_value_match(current, wanted) {
            PlatformValueMatch::Rejected => return false,
            PlatformValueMatch::Matched => matched = true,
            PlatformValueMatch::NoMatch => {}
        }
    }
    matched || wanted.iter().all(|entry| entry.starts_with('!'))
}

enum PlatformValueMatch {
    Rejected,
    Matched,
    NoMatch,
}

fn platform_value_match(value: &str, wanted: &[String]) -> PlatformValueMatch {
    for entry in wanted {
        if let Some(stripped) = entry.strip_prefix('!') {
            if stripped == value {
                return PlatformValueMatch::Rejected;
            }
        } else if entry == value {
            return PlatformValueMatch::Matched;
        }
    }
    PlatformValueMatch::NoMatch
}
