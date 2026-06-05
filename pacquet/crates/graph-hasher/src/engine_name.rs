pub use pacquet_detect_libc::{host_arch, host_platform};

/// Compute pnpm's `ENGINE_NAME` string — the same value pnpm uses
/// as the side-effects cache key prefix.
///
/// Ports
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/core/constants/src/index.ts#L7>:
/// ```js
/// `${process.platform};${process.arch};node${process.version.split('.')[0].substring(1)}`
/// ```
///
/// Example outputs:
/// - `"darwin;arm64;node20"`
/// - `"linux;x64;node22"`
/// - `"win32;x64;node24"`
///
/// `node_major` is the Node major version (e.g. `20`, `22`, `24`).
/// Callers pass it as a number because the discovery side (spawning
/// `node --version` or reading `npm_node_execpath`) is policy and
/// doesn't belong in this hasher crate.
///
/// `platform` and `arch` default to the running host via the
/// static `std::env::consts` constants mapped through Node's
/// naming scheme. Production callers can pass `None` to get the
/// host values; tests can pin both for cache-key round-trip.
#[must_use]
pub fn engine_name(node_major: u32, platform: Option<&str>, arch: Option<&str>) -> String {
    let platform = platform.unwrap_or_else(|| host_platform());
    let arch = arch.unwrap_or_else(|| host_arch());
    format!("{platform};{arch};node{node_major}")
}

/// Discover the host Node binary's major version by spawning
/// `node --version` and parsing the leading major-version digits
/// from its output.
///
/// Accepted shapes (in order of how `parse_node_version_output`
/// strips them):
/// - `v22.11.0` — canonical Node output.
/// - `22.11.0` — a leading `v` is optional, for Node-compat runtimes
///   that drop it.
/// - `v25.0.0-nightly` — pre-release tags after the major are fine
///   because parsing stops at the first `.`.
/// - `v22` — no `.` at all is still parseable; the whole post-`v`
///   string is treated as the major.
///
/// Used by [`engine_name`] callers that don't have a Node version
/// pinned by config. Returns `None` when:
/// - `node` isn't on `PATH`,
/// - the binary fails to launch,
/// - or the leading token (after an optional `v` and before the
///   first `.`) isn't a parseable `u32`.
///
/// Callers should fall back to either a sentinel cache key (which
/// won't match any pnpm-written entry — safe) or skip the
/// cache-read entirely when this returns `None`.
#[must_use]
pub fn detect_node_major() -> Option<u32> {
    let raw = detect_node_version_raw()?;
    parse_node_version_output(&raw)
}

/// Discover the host Node binary's full version by spawning
/// `node --version` and stripping the leading `v`. Returns the
/// trimmed semver-shaped string (e.g. `"22.11.0"`) or `None` if
/// detection fails for any of the reasons listed on
/// [`detect_node_major`].
///
/// Used by `pacquet-package-is-installable`'s `check_engine` to
/// evaluate `engines.node` ranges. Pacquet's installability check
/// needs the full version, not just the major, because ranges like
/// `>=14.18.0` would otherwise spuriously reject `14.17.x`.
#[must_use]
pub fn detect_node_version() -> Option<String> {
    let raw = detect_node_version_raw()?;
    Some(raw.strip_prefix('v').unwrap_or(&raw).to_string())
}

fn detect_node_version_raw() -> Option<String> {
    let output = std::process::Command::new("node").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(std::str::from_utf8(&output.stdout).ok()?.trim().to_string())
}

/// Parse `v22.11.0`-style output from `node --version` to the major
/// version integer. Factored out so the parsing rule is unit-testable
/// without spawning `node`.
fn parse_node_version_output(stdout: &str) -> Option<u32> {
    // Tolerate a missing leading `v` for alternative Node-compat
    // runtimes that omit it.
    let after_v = stdout.strip_prefix('v').unwrap_or(stdout);
    let major = after_v.split('.').next()?;
    major.parse().ok()
}

/// Host libc implementation string. Three return values matching
/// pnpm's `'glibc' | 'musl' | null` (with `null` translated to
/// `"unknown"` so the call site stays infallible):
///
/// - `"musl"` — musl-based Linux.
/// - `"glibc"` — glibc-based Linux.
/// - `"unknown"` — non-Linux host (macOS, Windows, BSD, etc.) or
///   detection failure. `check_platform` treats this as "skip libc
///   constraint" — see the upstream call site at
///   <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/checkPlatform.ts#L38>.
///
/// Delegates to [`pacquet_detect_libc::detect()`] for the
/// actual detection; see that function for the fallback chain. The
/// result is cached after the first call via [`std::sync::OnceLock`].
pub fn host_libc() -> &'static str {
    use std::sync::OnceLock;

    static CACHED: OnceLock<&'static str> = OnceLock::new();
    CACHED.get_or_init(|| {
        pacquet_detect_libc::detect().map_or("unknown", pacquet_detect_libc::Implementation::as_str)
    })
}

#[cfg(test)]
mod tests;
