use super::{PackageManagerSource, package_manager_source};
use pretty_assertions::assert_eq;

#[test]
fn package_manager_sources_follow_release_lines() {
    assert_eq!(
        package_manager_source("npm", "latest"),
        Some(PackageManagerSource::Registry("npm"))
    );
    assert_eq!(
        package_manager_source("pnpm", "10"),
        Some(PackageManagerSource::Registry("@pnpm/exe"))
    );
    assert_eq!(
        package_manager_source("yarn", "1.22.22"),
        Some(PackageManagerSource::Registry("yarn"))
    );
    assert_eq!(
        package_manager_source("yarn", "4.9.2"),
        Some(PackageManagerSource::Registry("@yarnpkg/cli-dist"))
    );
    assert_eq!(package_manager_source("yarn", "6.0.0"), Some(PackageManagerSource::Archive));
    assert_eq!(package_manager_source("bun", "latest"), Some(PackageManagerSource::Archive));
    assert_eq!(package_manager_source("unknown", "1.0.0"), None);
}
