use super::package_version;
use pnpm_lockfile::PackageKey;

#[test]
fn package_identity_uses_the_manifest_version_for_non_registry_sources() {
    let file: PackageKey = "native-addon@file:../native-addon.tgz".parse().unwrap();
    assert_eq!(package_version(&file, Some("1.0.0")), "1.0.0");

    let registry: PackageKey = "native-addon@2.0.0".parse().unwrap();
    assert_eq!(package_version(&registry, None), "2.0.0");
}
