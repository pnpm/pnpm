//! Where pnpm gets a release of a package manager it provisions.

/// Yarn moved its CLI to `@yarnpkg/cli-dist` in 2.0, and Yarn 6
/// (`yarnpkg/zpm`) is a native binary with no npm package at all.
const YARN_BERRY_MAJOR: u64 = 2;
const YARN_NATIVE_MAJOR: u64 = 6;

/// Where the releases a version specifier selects come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManagerSource {
    /// The npm package of this name.
    Registry(&'static str),
    /// Platform archives the package manager's own project publishes:
    /// Bun, and Yarn 6 and later.
    Archive,
}

/// The source of the releases of the package manager `name` (`bun`, `npm`,
/// `pnpm`, or `yarn`) that `version_spec` selects. `None` for any other
/// name.
///
/// Yarn's sources are split by major, so the lowest major the specifier
/// admits decides. A specifier that commits to none, such as a dist-tag or
/// a wildcard, means the current line, Berry.
#[must_use]
pub fn package_manager_source(name: &str, version_spec: &str) -> Option<PackageManagerSource> {
    match name {
        "bun" => Some(PackageManagerSource::Archive),
        "npm" => Some(PackageManagerSource::Registry("npm")),
        "pnpm" => Some(PackageManagerSource::Registry("pnpm")),
        "yarn" => Some(yarn_source(version_spec)),
        _ => None,
    }
}

fn yarn_source(version_spec: &str) -> PackageManagerSource {
    match committed_major(version_spec) {
        Some(major) if major >= YARN_NATIVE_MAJOR => PackageManagerSource::Archive,
        Some(major) if major < YARN_BERRY_MAJOR => PackageManagerSource::Registry("yarn"),
        _ => PackageManagerSource::Registry("@yarnpkg/cli-dist"),
    }
}

/// The lowest major a specifier admits, or `None` when it admits any
/// version: a dist-tag, a wildcard, or something unparsable.
fn committed_major(version_spec: &str) -> Option<u64> {
    let version_spec = version_spec.trim();
    if version_spec.is_empty() || version_spec == "*" || version_spec.eq_ignore_ascii_case("x") {
        return None;
    }
    Some(node_semver::Range::parse(version_spec).ok()?.min_version()?.major)
}
