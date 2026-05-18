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

/// Map `std::env::consts::OS` to Node's `process.platform` naming.
/// Node uses `darwin` / `linux` / `win32` / `freebsd` / `openbsd` /
/// `sunos` / `aix` / `android`. Rust uses `macos` / `linux` /
/// `windows` / `freebsd` / `openbsd` / `solaris` / `aix` /
/// `android`. Only `macos`, `windows`, and `solaris` differ.
pub fn host_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        "solaris" => "sunos",
        other => other,
    }
}

/// Map `std::env::consts::ARCH` to Node's `process.arch` naming.
/// Node uses `x64` / `arm64` / `ia32` / `arm` / `s390x` / `ppc64`
/// / `ppc64` (LE, same string) / `loong64` / `riscv64`. Rust uses
/// `x86_64` / `aarch64` / `x86` / `arm` / `s390x` / `powerpc64` /
/// `powerpc64le` / `loongarch64` / `riscv64`. Mappings below mirror
/// what Node itself emits on each target — anything left as
/// passthrough (e.g. `arm`, `s390x`, `riscv64`) already matches
/// between the two naming schemes.
pub fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "ia32",
        // Node calls big-endian and little-endian POWER both
        // `ppc64`; only big-endian gets `endianness === 'BE'` to
        // distinguish them. Rust's two arch values both map here.
        "powerpc64" | "powerpc64le" => "ppc64",
        "loongarch64" => "loong64",
        other => other,
    }
}

/// Host libc family, mapped to the same string `detect-libc.familySync()`
/// returns upstream. Three return values, matching upstream's
/// `'glibc' | 'musl' | null` (with `null` translated to `"unknown"`
/// so the call site stays infallible):
///
/// - `"musl"` — `/lib/ld-musl-*.so.1` (or `/lib64/ld-musl-*`) is
///   present. Alpine, Void, every musl-based distro.
/// - `"glibc"` — Linux without a musl loader. Glibc is the
///   overwhelming default on every other Linux distro; the binary
///   layout this code runs on always has a libc, so a missing musl
///   loader on Linux means glibc.
/// - `"unknown"` — non-Linux host (macOS, Windows, BSD, etc.).
///   `check_platform` treats this as "skip libc constraint" — see
///   the upstream call site at
///   <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/checkPlatform.ts#L38>.
///
/// Cached after the first call via [`OnceLock`] so a per-install
/// `InstallabilityHost::detect` doesn't pay a `read_dir` on every
/// invocation (the directory listing settles to the same answer
/// for the life of the process, and there's no scenario in which
/// the host swaps libc mid-run).
///
/// Pacquet rolls its own probe instead of pulling in the
/// `detect-libc` crate to avoid a new workspace dep for what is
/// effectively a directory-presence check on Linux. Same accuracy
/// in practice — `detect-libc`'s `familySync` itself just reads
/// `/usr/bin/ldd` / `/lib`. If a future host triggers a
/// false-positive `"glibc"` (e.g. embedded Linux without glibc),
/// the upstream constraint that depended on it will already have
/// been declared `"glibc"` in `package.json#libc` so the false
/// positive is harmless: the package is kept, not skipped.
///
/// [`OnceLock`]: std::sync::OnceLock
pub fn host_libc() -> &'static str {
    static CACHED: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    CACHED.get_or_init(detect_host_libc)
}

#[cfg(target_os = "linux")]
fn detect_host_libc() -> &'static str {
    // Musl ships its dynamic loader as `/lib/ld-musl-<arch>.so.1`
    // (or `/lib64/ld-musl-<arch>.so.1` on some configurations).
    // Glibc lives at the arch-tuple path (`/lib/x86_64-linux-gnu/`
    // etc.) and never installs an `ld-musl-*` artifact. The
    // directory probe is cheaper than spawning `ldd --version` and
    // doesn't require `ldd` to be on PATH (it isn't, in slim
    // containers).
    for dir in ["/lib", "/lib64"] {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.as_encoded_bytes().starts_with(b"ld-musl-") {
                return "musl";
            }
        }
    }
    "glibc"
}

#[cfg(not(target_os = "linux"))]
fn detect_host_libc() -> &'static str {
    "unknown"
}

#[cfg(test)]
mod tests {
    use super::{detect_node_major, detect_node_version, engine_name, parse_node_version_output};
    use pretty_assertions::assert_eq;

    /// Format matches pnpm's `${platform};${arch};node${major}`
    /// — required for the side-effects cache to interop.
    #[test]
    fn engine_name_matches_pnpm_format() {
        assert_eq!(engine_name(20, Some("darwin"), Some("arm64")), "darwin;arm64;node20");
        assert_eq!(engine_name(22, Some("linux"), Some("x64")), "linux;x64;node22");
        assert_eq!(engine_name(24, Some("win32"), Some("x64")), "win32;x64;node24");
    }

    /// Output of `node --version` is the trimmed string
    /// `v<major>.<minor>.<patch>`. Major-extract handles the leading
    /// `v` and falls through cleanly on alternative Node-compat
    /// runtimes that drop it.
    #[test]
    fn parse_node_version_handles_common_shapes() {
        assert_eq!(parse_node_version_output("v22.11.0"), Some(22));
        assert_eq!(parse_node_version_output("v20.18.1"), Some(20));
        // Off-spec (no leading `v`).
        assert_eq!(parse_node_version_output("18.20.4"), Some(18));
        // Pre-release tag in the patch position — still parses the
        // major.
        assert_eq!(parse_node_version_output("v25.0.0-nightly"), Some(25));
        // Garbage returns `None` so the caller can fall through to
        // the no-cache path.
        assert_eq!(parse_node_version_output(""), None);
        assert_eq!(parse_node_version_output("not a version"), None);
        assert_eq!(parse_node_version_output("v.broken"), None);
    }

    /// Defaults route through the host mapping. Just assert the
    /// shape (three semicolon-separated parts ending in
    /// `node<digits>`) — the exact OS/arch depends on where the
    /// test is run.
    #[test]
    fn engine_name_host_default_has_expected_shape() {
        let name = engine_name(20, None, None);
        let parts: Vec<&str> = name.split(';').collect();
        assert_eq!(parts.len(), 3, "expected three parts, got {name:?}");
        assert!(parts[2].starts_with("node"), "third part must start with `node`: {name:?}");
        assert!(parts[2][4..].parse::<u32>().is_ok(), "node version must be numeric: {name:?}");
    }

    /// `detect_node_version` returns the full version string with
    /// the leading `v` stripped. `node` is a hard prerequisite for
    /// the test suite — if it isn't on `PATH` that's a test-env
    /// bug, so we `expect` rather than skip.
    #[test]
    fn detect_node_version_strips_leading_v() {
        let version = detect_node_version().expect("`node` must be on PATH for the test suite");
        assert!(!version.starts_with('v'), "leading `v` must be stripped: {version:?}");
        let major = version.split('.').next().expect("at least one component");
        assert!(major.parse::<u32>().is_ok(), "major must be numeric: {version:?}");
    }

    /// `detect_node_major` round-trips through `detect_node_version` and
    /// the parser. When both are wired correctly the integer major
    /// matches the leading component of the full version string.
    #[test]
    fn detect_node_major_matches_detect_node_version_leading_component() {
        let major = detect_node_major().expect("`node` must be on PATH for the test suite");
        let version = detect_node_version().expect("`node` must be on PATH for the test suite");
        let leading: u32 =
            version.split('.').next().expect("non-empty version").parse().expect("major numeric");
        assert_eq!(major, leading);
    }
}
