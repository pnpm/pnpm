use crate::prune_merged_branch_lockfile::prune_merged_branch_lockfile;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use serde_json::json;
use std::path::PathBuf;

fn manifest(value: serde_json::Value) -> PackageManifest {
    PackageManifest::from_value(PathBuf::from("/project/package.json"), value)
}

fn lockfile(source: &str) -> Lockfile {
    serde_saphyr::from_str(source).expect("parse lockfile")
}

/// What `mergeLockfileChanges` leaves behind when the main branch
/// dropped `bar` after the branch lockfile was written: `bar` is back in
/// the importer, and `child` is in the graph only because `bar` reaches
/// it.
const MERGED: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    specifiers:
      foo: ^1.0.0
      bar: ^2.0.0
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
      bar:
        specifier: ^2.0.0
        version: 2.0.0
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-foo
  bar@2.0.0:
    resolution:
      integrity: sha512-bar
  child@3.0.0:
    resolution:
      integrity: sha512-child
snapshots:
  foo@1.1.0: {}
  bar@2.0.0:
    dependencies:
      child: 3.0.0
  child@3.0.0: {}
";

#[test]
fn drops_the_reinstated_dependency_and_what_only_it_reached() {
    let manifest = manifest(json!({ "dependencies": { "foo": "^1.0.0" } }));
    let pruned =
        prune_merged_branch_lockfile(&lockfile(MERGED), &[(".".to_string(), &manifest)], true);
    let packages = pruned.packages.as_ref().expect("packages");
    assert_eq!(packages.len(), 1);
    assert!(packages.contains_key(&"foo@1.1.0".parse().expect("package key")));
    let snapshots = pruned.snapshots.as_ref().expect("snapshots");
    assert_eq!(snapshots.len(), 1);
    assert!(snapshots.contains_key(&"foo@1.1.0".parse().expect("package key")));
}

#[test]
fn leaves_a_merge_the_manifests_still_declare_alone() {
    let merged = lockfile(MERGED);
    let manifest = manifest(json!({ "dependencies": { "foo": "^1.0.0", "bar": "^2.0.0" } }));
    let pruned = prune_merged_branch_lockfile(&merged, &[(".".to_string(), &manifest)], true);
    assert_eq!(pruned, merged);
}

/// A filtered install names only the projects it selected, and the
/// packages an unselected project reaches have to survive its absence.
#[test]
fn keeps_what_an_unnamed_importer_reaches() {
    let merged = lockfile(
        r"
lockfileVersion: '9.0'
importers:
  .:
    specifiers:
      foo: ^1.0.0
      bar: ^2.0.0
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
      bar:
        specifier: ^2.0.0
        version: 2.0.0
  packages/other:
    specifiers:
      bar: ^2.0.0
    dependencies:
      bar:
        specifier: ^2.0.0
        version: 2.0.0
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-foo
  bar@2.0.0:
    resolution:
      integrity: sha512-bar
snapshots:
  foo@1.1.0: {}
  bar@2.0.0: {}
",
    );
    let manifest = manifest(json!({ "dependencies": { "foo": "^1.0.0" } }));
    let pruned = prune_merged_branch_lockfile(&merged, &[(".".to_string(), &manifest)], true);
    assert!(pruned.importers["packages/other"].dependencies.is_some());
    assert_eq!(pruned.packages.as_ref().expect("packages").len(), 2);
}

/// Severing the last non-optional path to a package makes it optional,
/// the same recomputation an absorbed importer edit runs.
#[test]
fn recomputes_the_optional_flag_of_a_surviving_package() {
    let merged = lockfile(
        r"
lockfileVersion: '9.0'
importers:
  .:
    specifiers:
      foo: ^1.0.0
      bar: ^2.0.0
    dependencies:
      bar:
        specifier: ^2.0.0
        version: 2.0.0
    optionalDependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-foo
  bar@2.0.0:
    resolution:
      integrity: sha512-bar
snapshots:
  foo@1.1.0: {}
  bar@2.0.0:
    dependencies:
      foo: 1.1.0
",
    );
    let manifest = manifest(json!({ "optionalDependencies": { "foo": "^1.0.0" } }));
    let pruned = prune_merged_branch_lockfile(&merged, &[(".".to_string(), &manifest)], true);
    let snapshots = pruned.snapshots.as_ref().expect("snapshots");
    assert!(snapshots[&"foo@1.1.0".parse().expect("package key")].optional);
}
