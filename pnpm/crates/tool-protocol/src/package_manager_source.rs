const YARN_BERRY_MAJOR: u64 = 2;
const YARN_NATIVE_MAJOR: u64 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManagerSource {
    Registry(&'static str),
    Archive,
}

#[must_use]
pub fn package_manager_source(name: &str, version_spec: &str) -> Option<PackageManagerSource> {
    match name {
        "bun" => Some(PackageManagerSource::Archive),
        "npm" => Some(PackageManagerSource::Registry("npm")),
        "pnpm" => Some(PackageManagerSource::Registry("@pnpm/exe")),
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

fn committed_major(version_spec: &str) -> Option<u64> {
    let version_spec = version_spec.trim();
    if version_spec.is_empty() || version_spec == "*" || version_spec.eq_ignore_ascii_case("x") {
        return None;
    }
    Some(node_semver::Range::parse(version_spec).ok()?.min_version()?.major)
}
