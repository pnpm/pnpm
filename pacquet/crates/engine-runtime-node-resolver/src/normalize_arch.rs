//! Pacquet port of
//! [`normalizeArch.ts`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/normalizeArch.ts).

/// Translate the `(platform, arch)` pair pnpm sees at install time into
/// the directory-name shape nodejs.org actually publishes for.
///
/// Three quirks worth calling out — each is exercised by an upstream
/// test:
/// - `darwin` / `arm64` on Node ≤ 15 has no Apple-Silicon build, so
///   pnpm falls back to the `x64` (Rosetta) tarball.
/// - `win32` / `ia32` is published as `win-x86`.
/// - Linux `arm` is published as `armv7l` (Raspberry Pi 4 et al.).
///
/// `node_version` is optional because the macOS Apple-Silicon rule
/// only matters when a concrete version is in hand; callers that
/// haven't picked one yet pass `None` and accept the default mapping.
#[must_use]
pub fn get_normalized_arch(platform: &str, arch: &str, node_version: Option<&str>) -> String {
    if let Some(version) = node_version
        && let Some(major) = node_major(version)
        && platform == "darwin"
        && arch == "arm64"
        && major < 16
    {
        return "x64".to_string();
    }
    if platform == "win32" && arch == "ia32" {
        return "x86".to_string();
    }
    if arch == "arm" {
        return "armv7l".to_string();
    }
    arch.to_string()
}

fn node_major(version: &str) -> Option<u32> {
    version.split('.').next()?.parse().ok()
}

#[cfg(test)]
mod tests;
