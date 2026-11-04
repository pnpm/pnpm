use super::{registry_package_ids, tarball_on_disk};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::path::Path;

#[test]
fn a_semver_version_names_one_registry_package() {
    let manifest = json!({ "name": "acme", "version": "1.0.0" });
    assert_eq!(registry_package_ids(Some(&manifest), []), ["acme@1.0.0"]);
}

#[test]
fn a_v_prefixed_version_also_names_the_version_semver_writes() {
    let manifest = json!({ "name": "acme", "version": "v1.0.0" });
    assert_eq!(registry_package_ids(Some(&manifest), []), ["acme@v1.0.0", "acme@1.0.0"]);
}

#[test]
fn a_manifest_without_a_version_names_no_registry_package() {
    let manifest = json!({ "name": "acme" });
    assert_eq!(registry_package_ids(Some(&manifest), []), Vec::<String>::new());
}

#[test]
fn each_declared_prefix_names_a_registry_package() {
    let manifest = json!({ "name": "acme", "version": "v1.0.0" });
    assert_eq!(
        registry_package_ids(Some(&manifest), ["home", "work"]),
        ["acme@v1.0.0", "acme@1.0.0", "acme@home:1.0.0", "acme@work:1.0.0"],
    );
}

#[test]
fn a_version_semver_rejects_names_no_prefixed_registry_package() {
    let manifest = json!({ "name": "acme", "version": "latest" });
    assert_eq!(registry_package_ids(Some(&manifest), ["work"]), ["acme@latest"]);
}

#[test]
fn a_local_tarball_is_read_from_the_lockfile_dir() {
    let lockfile_dir = Path::new("/project");
    assert_eq!(
        tarball_on_disk("local-filesystem", "file:vendor/acme-1.0.0.tgz", lockfile_dir),
        Some(lockfile_dir.join("vendor/acme-1.0.0.tgz")),
    );
}

#[test]
fn a_registry_resolution_to_a_file_url_is_not_a_downloaded_tarball() {
    let lockfile_dir = Path::new("/project");
    assert_eq!(tarball_on_disk("npm-registry", "file:vendor/acme-1.0.0.tgz", lockfile_dir), None);
}
