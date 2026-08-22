use crate::{Lockfile, PackageKey, PkgName, merge_lockfile_changes};
use std::path::Path;

fn parse(yaml: &str) -> Lockfile {
    Lockfile::parse(yaml, Path::new("pnpm-lock.yaml")).unwrap().unwrap()
}

fn merged(ours: &str, theirs: &str) -> Lockfile {
    merge_lockfile_changes(&parse(ours), &parse(theirs))
}

const OURS: &str = "\
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      is-positive:
        specifier: ^3.0.0
        version: 3.0.0
      only-ours:
        specifier: ^1.0.0
        version: 1.0.0
packages:
  is-positive@3.0.0:
    resolution: {integrity: sha512-ours}
  only-ours@1.0.0:
    resolution: {integrity: sha512-only-ours}
snapshots:
  is-positive@3.0.0: {}
  only-ours@1.0.0: {}
";

const THEIRS: &str = "\
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      is-positive:
        specifier: ^3.1.0
        version: 3.1.0
  packages/theirs:
    devDependencies:
      only-theirs:
        specifier: ^2.0.0
        version: 2.0.0
packages:
  is-positive@3.1.0:
    resolution: {integrity: sha512-theirs}
  only-theirs@2.0.0:
    resolution: {integrity: sha512-only-theirs}
snapshots:
  is-positive@3.1.0: {}
  only-theirs@2.0.0: {}
";

#[test]
fn the_higher_version_wins_and_the_specifier_follows_the_change() {
    let merged = merged(OURS, THEIRS);
    let root = merged.importers.get(".").expect("the root importer survives the merge");
    let dependencies = root.dependencies.as_ref().expect("the root has dependencies");
    let is_positive = &dependencies[&PkgName::parse("is-positive").unwrap()];
    assert_eq!(is_positive.version.to_string(), "3.1.0");
    assert_eq!(is_positive.specifier, "^3.1.0");
}

/// pnpm's `mergeVersions` keeps ours only when it is strictly greater, so
/// the incoming lockfile cannot silently downgrade a dependency.
#[test]
fn a_lower_incoming_version_loses() {
    let merged = merged(THEIRS, OURS);
    let dependencies = merged.importers["."].dependencies.as_ref().unwrap();
    let is_positive = &dependencies[&PkgName::parse("is-positive").unwrap()];
    assert_eq!(is_positive.version.to_string(), "3.1.0");
}

#[test]
fn entries_only_one_side_records_all_survive() {
    let merged = merged(OURS, THEIRS);
    let dependencies = merged.importers["."].dependencies.as_ref().unwrap();
    assert!(dependencies.contains_key(&PkgName::parse("only-ours").unwrap()));
    assert!(merged.importers.contains_key("packages/theirs"));

    let packages = merged.packages.as_ref().unwrap();
    let mut names: Vec<String> = packages.keys().map(ToString::to_string).collect();
    names.sort();
    assert_eq!(
        names,
        ["is-positive@3.0.0", "is-positive@3.1.0", "only-ours@1.0.0", "only-theirs@2.0.0"],
    );
    assert_eq!(merged.snapshots.as_ref().unwrap().len(), 4);
}

/// An entry both sides record merges field by field, the way pnpm's
/// object spread does — a field only ours carries is not dropped because
/// theirs omits it.
#[test]
fn a_shared_entry_keeps_the_fields_only_one_side_records() {
    let ours = "\
lockfileVersion: '9.0'
packages:
  is-positive@3.0.0:
    resolution: {integrity: sha512-old}
    hasBin: true
snapshots:
  is-positive@3.0.0:
    optional: true
";
    let theirs = "\
lockfileVersion: '9.0'
packages:
  is-positive@3.0.0:
    resolution: {integrity: sha512-new}
    deprecated: do not use
snapshots:
  is-positive@3.0.0:
    transitivePeerDependencies:
      - react
";
    let merged = merged(ours, theirs);
    let key: PackageKey = "is-positive@3.0.0".parse().unwrap();
    let entry = &merged.packages.as_ref().unwrap()[&key];
    assert_eq!(entry.has_bin, Some(true), "ours survives what theirs omits");
    assert_eq!(entry.deprecated.as_deref(), Some("do not use"));

    let snapshot = &merged.snapshots.as_ref().unwrap()[&key];
    assert!(snapshot.optional, "ours survives what theirs omits");
    assert_eq!(
        snapshot.transitive_peer_dependencies.as_deref(),
        Some(["react".to_string()].as_slice()),
    );
}

/// The merged lockfile is what the install then resolves against, so the
/// recorded-config fields are deliberately dropped rather than picked
/// from one side — the install writes its own back.
#[test]
fn the_recorded_configuration_is_dropped() {
    let ours = "\
lockfileVersion: '9.0'
settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
overrides:
  foo: 1.0.0
ignoredOptionalDependencies:
  - fsevents
pnpmfileChecksum: ours
";
    let theirs = "\
lockfileVersion: '9.0'
ignoredOptionalDependencies:
  - node-gyp
pnpmfileChecksum: theirs
";
    let merged = merged(ours, theirs);
    assert!(merged.settings.is_none());
    assert!(merged.overrides.is_none());
    assert_eq!(merged.pnpmfile_checksum.as_deref(), Some("ours"));
    assert_eq!(
        merged.ignored_optional_dependencies.as_deref(),
        Some(["fsevents".to_string(), "node-gyp".to_string()].as_slice()),
    );
}

/// A tool driving pnpm records its own state in a top-level block beside
/// pnpm's; merging two branches' lockfiles must not delete it. Ours wins a
/// conflict, matching the precedence the other fields use.
#[test]
fn merging_unions_the_foreign_top_level_keys() {
    let mut ours = parse("lockfileVersion: '9.0'\n");
    ours.extra.insert("bit".to_string(), serde_json::json!({ "depsRequiringBuild": ["ours"] }));
    let mut theirs = parse("lockfileVersion: '9.0'\n");
    theirs.extra.insert("bit".to_string(), serde_json::json!({ "depsRequiringBuild": ["theirs"] }));
    theirs.extra.insert("other-tool".to_string(), serde_json::json!(true));

    let merged = merge_lockfile_changes(&ours, &theirs);

    assert_eq!(merged.extra["bit"], serde_json::json!({ "depsRequiringBuild": ["ours"] }));
    assert_eq!(merged.extra["other-tool"], serde_json::json!(true));
}
