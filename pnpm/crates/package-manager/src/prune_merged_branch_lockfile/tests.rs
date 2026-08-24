use crate::prune_merged_branch_lockfile::prune_merged_branch_lockfile;
use pnpm_lockfile::{Lockfile, ProjectSnapshot};
use pnpm_package_manifest::PackageManifest;
use serde_json::json;
use std::{collections::HashMap, path::PathBuf};

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

/// The importers `main` had before the branch lockfile was folded in:
/// `bar` is the fold's own addition.
fn pre_merge_without_bar() -> HashMap<String, ProjectSnapshot> {
    let base = lockfile(
        r"
lockfileVersion: '9.0'
importers:
  .:
    specifiers:
      foo: ^1.0.0
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
",
    );
    base.importers
}

#[test]
fn drops_the_reinstated_dependency_and_what_only_it_reached() {
    let manifest = manifest(json!({ "dependencies": { "foo": "^1.0.0" } }));
    let pruned = prune_merged_branch_lockfile(
        &lockfile(MERGED),
        &pre_merge_without_bar(),
        &[(".".to_string(), &manifest)],
        true,
    )
    .expect("the fold reinstated a dependency the manifest dropped");
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
    assert!(
        prune_merged_branch_lockfile(
            &merged,
            &pre_merge_without_bar(),
            &[(".".to_string(), &manifest)],
            true,
        )
        .is_none(),
    );
}

/// `mergeGitBranchLockfilesBranchPattern` leaves merge mode on for every
/// install on a matched branch, so an undeclared entry the read lockfile
/// already carried must survive for the freshness check to report.
#[test]
fn leaves_drift_the_fold_did_not_introduce_alone() {
    let merged = lockfile(MERGED);
    let manifest = manifest(json!({ "dependencies": { "foo": "^1.0.0" } }));
    assert!(
        prune_merged_branch_lockfile(
            &merged,
            &merged.importers,
            &[(".".to_string(), &manifest)],
            true,
        )
        .is_none(),
    );
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
    let pruned = prune_merged_branch_lockfile(
        &merged,
        &pre_merge_without_bar(),
        &[(".".to_string(), &manifest)],
        true,
    )
    .expect("the root importer lost a dependency");
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
    let pruned = prune_merged_branch_lockfile(
        &merged,
        &pre_merge_without_bar(),
        &[(".".to_string(), &manifest)],
        true,
    )
    .expect("the root importer lost a dependency");
    let snapshots = pruned.snapshots.as_ref().expect("snapshots");
    assert!(snapshots[&"foo@1.1.0".parse().expect("package key")].optional);
}
