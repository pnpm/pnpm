//! Port of `checkPlatform.ts` from
//! <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/checkPlatform.ts>.

use derive_more::{Display, Error};
use miette::Diagnostic;
use serde::{Deserialize, Serialize};

/// Caller-supplied override for the `os` / `cpu` / `libc` triples
/// against which a package's wanted platform is evaluated. Each list
/// defaults to `['current']` at the call site (upstream reads the
/// config setting and falls back to `['current']` if absent). The
/// `'current'` sentinel is replaced with the host triple via
/// `dedupe_current` before the `os` / `cpu` / `libc` lists are
/// compared. Mirrors upstream's `SupportedArchitectures` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/core/types/src/package.ts#L232-L236>.
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
/// into a fresh `WantedPlatform` per snapshot. The owned form is
/// only materialised inside `check_platform` when an error is
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
    // Mirror upstream's `JSON.stringify(wanted)` shape: only the
    // fields actually set appear, and each is a JSON array.
    let mut parts = Vec::new();
    if let Some(os) = &wanted.os {
        parts.push(format!("\"os\":{}", json_string_array(os)));
    }
    if let Some(cpu) = &wanted.cpu {
        parts.push(format!("\"cpu\":{}", json_string_array(cpu)));
    }
    if let Some(libc) = &wanted.libc {
        parts.push(format!("\"libc\":{}", json_string_array(libc)));
    }
    format!("{{{}}}", parts.join(","))
}

fn current_json(current: &Platform) -> String {
    // Upstream constructs `{ os: platform, cpu: arch, libc: currentLibc }`
    // (single strings, not arrays). Mirror that shape.
    fn single(values: &[String]) -> String {
        values.first().cloned().unwrap_or_default()
    }
    format!(
        "{{\"os\":{:?},\"cpu\":{:?},\"libc\":{:?}}}",
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
/// honored exactly as upstream's `checkList`.
///
/// The wanted axes are taken as `Option<&[String]>` slices so the
/// hot path doesn't allocate a `WantedPlatform` per snapshot —
/// callers can pass `manifest.os.as_deref()` directly. The owned
/// [`WantedPlatform`] form is only built when an error is returned
/// (for diagnostic display via the
/// [`UnsupportedPlatformError`]).
///
/// `supported_architectures` substitutes for `['current']` per axis;
/// `'current'` entries are replaced with the host value before
/// comparison (see `dedupe_current` in this module), matching pnpm
/// at <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/checkPlatform.ts#L88-L90>.
///
/// `current_os`, `current_cpu`, and `current_libc` are passed in
/// rather than read from the environment so this function stays
/// trivially testable (and the upstream tests that mock `process.platform`
/// translate directly).
pub fn check_platform(
    package_id: &str,
    wanted: WantedPlatformRef<'_>,
    supported: Option<&SupportedArchitectures>,
    current_os: &str,
    current_cpu: &str,
    current_libc: &str,
) -> Option<UnsupportedPlatformError> {
    let default_current = vec!["current".to_string()];
    let os_supp = supported.and_then(|supported| supported.os.as_ref()).unwrap_or(&default_current);
    let cpu_supp =
        supported.and_then(|supported| supported.cpu.as_ref()).unwrap_or(&default_current);
    let libc_supp =
        supported.and_then(|supported| supported.libc.as_ref()).unwrap_or(&default_current);

    let current = Platform {
        os: dedupe_current(current_os, os_supp),
        cpu: dedupe_current(current_cpu, cpu_supp),
        libc: dedupe_current(current_libc, libc_supp),
    };

    let mut os_ok = true;
    let mut cpu_ok = true;
    let mut libc_ok = true;

    if let Some(wanted_os) = wanted.os {
        os_ok = check_list(&current.os, wanted_os);
    }
    if let Some(wanted_cpu) = wanted.cpu {
        cpu_ok = check_list(&current.cpu, wanted_cpu);
    }
    if let Some(wanted_libc) = wanted.libc
        && current_libc != "unknown"
    {
        libc_ok = check_list(&current.libc, wanted_libc);
    }

    if !os_ok || !cpu_ok || !libc_ok {
        // Cold path. Only here do we materialise the owned
        // `WantedPlatform` for the error payload — the rest of the
        // pass borrows.
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
        return Some(UnsupportedPlatformError::new(
            package_id.to_string(),
            owned_wanted,
            real_current,
        ));
    }
    None
}

/// Replace the literal `current` sentinel in `supported` with the
/// concrete host value. Ports upstream's `dedupeCurrent` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/checkPlatform.ts#L88-L90>.
fn dedupe_current(current: &str, supported: &[String]) -> Vec<String> {
    supported
        .iter()
        .map(|item| if item == "current" { current.to_string() } else { item.clone() })
        .collect()
}

/// Decide whether any element of `value` is allowed by `list`.
///
/// Ports `checkList` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/checkPlatform.ts#L56-L86>.
fn check_list(value: &[String], list: &[String]) -> bool {
    if list.len() == 1 && list[0] == "any" {
        return true;
    }
    let mut matched = false;
    for v in value {
        for entry in list {
            if let Some(stripped) = entry.strip_prefix('!') {
                if stripped == v {
                    return false;
                }
            } else if entry == v {
                matched = true;
            }
        }
    }
    matched || list.iter().all(|e| e.starts_with('!'))
}
