use super::try_fast_update_ignored_optional_dependencies;
use pacquet_lockfile::Lockfile;

fn lockfile(source: &str) -> Lockfile {
    serde_saphyr::from_str(source).expect("parse lockfile")
}

#[test]
fn removes_matching_optional_edges_and_prunes_only_unreachable_packages() {
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      carrier:
        specifier: 1.0.0
        version: 1.0.0
      parent:
        specifier: 1.0.0
        version: 1.0.0
    optionalDependencies:
      root-only:
        specifier: 1.0.0
        version: 1.0.0
packages:
  carrier@1.0.0:
    resolution: {integrity: sha512-carrier}
  parent@1.0.0:
    resolution: {integrity: sha512-parent}
  root-only@1.0.0:
    resolution: {integrity: sha512-root}
  shared@1.0.0:
    resolution: {integrity: sha512-shared}
  unique@1.0.0:
    resolution: {integrity: sha512-unique}
snapshots:
  carrier@1.0.0:
    dependencies:
      shared: 1.0.0
  parent@1.0.0:
    optionalDependencies:
      shared: 1.0.0
      unique: 1.0.0
  root-only@1.0.0: {}
  shared@1.0.0: {}
  unique@1.0.0: {}
",
    );

    let updated = try_fast_update_ignored_optional_dependencies(
        &lockfile,
        &["root-only".to_string(), "shared".to_string(), "unique".to_string()],
    )
    .expect("additions should update");

    assert!(updated.importers["."].optional_dependencies.is_none());
    let parent_key = "parent@1.0.0".parse().expect("parent key");
    assert!(
        updated.snapshots.as_ref().expect("snapshots")[&parent_key].optional_dependencies.is_none(),
    );
    let snapshots = updated.snapshots.as_ref().expect("snapshots");
    assert!(snapshots.contains_key(&"shared@1.0.0".parse().expect("shared key")));
    assert!(!snapshots.contains_key(&"root-only@1.0.0".parse().expect("root key")));
    assert!(!snapshots.contains_key(&"unique@1.0.0".parse().expect("unique key")));
    let packages = updated.packages.as_ref().expect("packages");
    assert!(packages.contains_key(&"shared@1.0.0".parse().expect("shared key")));
    assert!(!packages.contains_key(&"root-only@1.0.0".parse().expect("root key")));
    assert!(!packages.contains_key(&"unique@1.0.0".parse().expect("unique key")));
}

#[test]
fn rejects_removing_an_ignored_pattern() {
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
ignoredOptionalDependencies:
  - foo
importers: {}
",
    );

    assert!(
        try_fast_update_ignored_optional_dependencies(&lockfile, &["bar".to_string()]).is_none()
    );
}

#[test]
fn rejects_an_added_exclusion_pattern() {
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
ignoredOptionalDependencies:
  - '*'
importers: {}
",
    );

    assert!(
        try_fast_update_ignored_optional_dependencies(
            &lockfile,
            &["*".to_string(), "!is-positive".to_string()],
        )
        .is_none()
    );
}

#[test]
fn records_an_added_pattern_even_when_it_matches_no_edge() {
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
importers: {}
",
    );

    let updated = try_fast_update_ignored_optional_dependencies(&lockfile, &["unused".to_string()])
        .expect("setting-only addition should update");
    assert_eq!(
        updated.ignored_optional_dependencies.as_deref(),
        Some(["unused".to_string()].as_slice())
    );
}
