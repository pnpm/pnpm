use super::try_fast_update_importers;
use pacquet_lockfile::Lockfile;
use pacquet_package_manifest::PackageManifest;
use serde_json::json;
use std::path::PathBuf;

fn lockfile() -> Lockfile {
    serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-deadbeef
snapshots:
  foo@1.1.0: {}
",
    )
    .expect("parse lockfile")
}

fn manifest(specifier: &str) -> PackageManifest {
    PackageManifest::from_value(
        PathBuf::from("/project/package.json"),
        json!({ "dependencies": { "foo": specifier } }),
    )
}

#[test]
fn updates_a_compatible_dependency_range() {
    let manifest = manifest(">=1 <2");
    let updated = try_fast_update_importers(&lockfile(), &[(".".to_string(), &manifest)])
        .expect("compatible range should update");
    assert_eq!(
        updated.importers["."].dependencies.as_ref().expect("dependencies")
            [&"foo".parse().expect("package name")]
            .specifier,
        ">=1 <2",
    );
}

#[test]
fn rejects_an_incompatible_dependency_range() {
    let manifest = manifest("^2");
    assert!(try_fast_update_importers(&lockfile(), &[(".".to_string(), &manifest)]).is_none());
}

#[test]
fn rejects_a_non_semver_dependency_specifier() {
    let manifest = manifest("latest");
    assert!(try_fast_update_importers(&lockfile(), &[(".".to_string(), &manifest)]).is_none());
}
