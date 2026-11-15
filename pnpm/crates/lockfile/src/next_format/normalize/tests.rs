use crate::{Lockfile, PackageKey, PkgName, next_format::LockfileFormat};
use pretty_assertions::assert_eq;
use std::path::Path;

fn parse(yaml: &str) -> Lockfile {
    let mut lockfile = Lockfile::parse(yaml, Path::new("pnpm-lock.yaml"))
        .expect("parse fixture")
        .expect("fixture has content");
    lockfile.lockfile_version = LockfileFormat::V13.version();
    lockfile
}

fn assert_round_trip(lockfile: &Lockfile) {
    let yaml = lockfile.to_yaml_string().expect("write the normalized lockfile");
    let read = Lockfile::parse(&yaml, Path::new("pnpm-lock.yaml"))
        .expect("read the normalized lockfile")
        .expect("the lockfile has content");
    assert_eq!(read, *lockfile);
}

fn key(value: &str) -> PackageKey {
    value.parse().unwrap()
}

fn name(value: &str) -> PkgName {
    value.parse().unwrap()
}

#[test]
fn removes_a_pruned_binding_and_preserves_a_required_binding() {
    let mut lockfile = parse(
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      consumer:
        specifier: ^1.0.0
        version: 1.0.0(gone@1.0.0)(kept@2.0.0)
packages:
  consumer@1.0.0:
    resolution: {integrity: sha512-AAAA}
    peerDependencies:
      gone: '*'
      kept: '*'
    peerDependenciesMeta:
      gone: {optional: true}
  kept@2.0.0:
    resolution: {integrity: sha512-BBBB}
snapshots:
  consumer@1.0.0(gone@1.0.0)(kept@2.0.0):
    dependencies:
      kept: 2.0.0
    transitivePeerDependencies: [unbound]
  kept@2.0.0: {}
",
    );
    assert!(lockfile.to_next_sections().is_err());
    let packages = lockfile.packages.clone();

    lockfile.normalize_peer_keys_for_next_format().unwrap();

    let consumer = key("consumer@1.0.0(kept@2.0.0)");
    let snapshots = lockfile.snapshots.as_ref().unwrap();
    assert!(snapshots.contains_key(&consumer));
    assert!(!snapshots.contains_key(&key("gone@1.0.0")));
    assert_eq!(
        snapshots[&consumer].dependencies.as_ref().unwrap()[&name("kept")].to_string(),
        "2.0.0",
    );
    assert_eq!(snapshots[&consumer].transitive_peer_dependencies, Some(vec!["unbound".into()]));
    assert_eq!(lockfile.packages, packages);
    let importer = &lockfile.importers["."];
    let dependency = &importer.dependencies.as_ref().unwrap()[&name("consumer")];
    assert_eq!(dependency.version.resolved_key(&name("consumer")), Some(consumer));
    assert_eq!(dependency.specifier, "^1.0.0");
    assert_round_trip(&lockfile);
}

#[test]
fn rewrites_nested_provider_contexts_aliases_and_every_dependency_group() {
    let mut lockfile = parse(
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      application:
        specifier: npm:consumer@1.0.0
        version: consumer@1.0.0(provider@1.0.0(gone@1.0.0))
      wrapper:
        specifier: 1.0.0
        version: 1.0.0
    devDependencies:
      dev-provider:
        specifier: npm:provider@1.0.0
        version: provider@1.0.0(gone@1.0.0)
    optionalDependencies:
      optional-consumer:
        specifier: npm:consumer@1.0.0
        version: consumer@1.0.0(provider@1.0.0(gone@1.0.0))
packages:
  consumer@1.0.0:
    resolution: {integrity: sha512-AAAA}
    peerDependencies:
      renamed: '*'
  provider@1.0.0:
    resolution: {integrity: sha512-BBBB}
    peerDependencies:
      gone: '*'
    peerDependenciesMeta:
      gone: {optional: true}
  wrapper@1.0.0:
    resolution: {integrity: sha512-CCCC}
snapshots:
  consumer@1.0.0(provider@1.0.0(gone@1.0.0)):
    dependencies:
      renamed: provider@1.0.0(gone@1.0.0)
  provider@1.0.0(gone@1.0.0): {}
  wrapper@1.0.0:
    dependencies:
      consumer-alias: consumer@1.0.0(provider@1.0.0(gone@1.0.0))
    optionalDependencies:
      provider-alias: provider@1.0.0(gone@1.0.0)
",
    );

    lockfile.normalize_peer_keys_for_next_format().unwrap();

    let consumer = key("consumer@1.0.0(provider@1.0.0)");
    let provider = key("provider@1.0.0");
    let importer = &lockfile.importers["."];
    for (entries, alias, expected) in [
        (&importer.dependencies, "application", &consumer),
        (&importer.dev_dependencies, "dev-provider", &provider),
        (&importer.optional_dependencies, "optional-consumer", &consumer),
    ] {
        assert_eq!(
            entries.as_ref().unwrap()[&name(alias)].version.resolved_key(&name(alias)),
            Some(expected.clone()),
        );
    }
    let snapshots = lockfile.snapshots.as_ref().unwrap();
    let wrapper = &snapshots[&key("wrapper@1.0.0")];
    assert_eq!(
        wrapper.dependencies.as_ref().unwrap()[&name("consumer-alias")].resolve(&name(
            "consumer-alias"
        )),
        Some(consumer.clone()),
    );
    assert_eq!(
        wrapper.optional_dependencies.as_ref().unwrap()[&name("provider-alias")].resolve(&name(
            "provider-alias"
        )),
        Some(provider.clone()),
    );
    assert_eq!(
        snapshots[&consumer].dependencies.as_ref().unwrap()[&name("renamed")].resolve(&name(
            "renamed"
        )),
        Some(provider),
    );
    assert_round_trip(&lockfile);
}

#[test]
fn coalesces_equivalent_snapshots_after_rebuilding_their_keys() {
    let mut lockfile = parse(
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      first:
        specifier: npm:consumer@1.0.0
        version: consumer@1.0.0(gone@1.0.0)
      second:
        specifier: npm:consumer@1.0.0
        version: consumer@1.0.0(gone@2.0.0)
packages:
  consumer@1.0.0:
    resolution: {integrity: sha512-AAAA}
snapshots:
  consumer@1.0.0(gone@1.0.0): {}
  consumer@1.0.0(gone@2.0.0): {}
",
    );

    lockfile.normalize_peer_keys_for_next_format().unwrap();

    let snapshots = lockfile.snapshots.as_ref().unwrap();
    assert_eq!(snapshots.len(), 1);
    assert!(snapshots.contains_key(&key("consumer@1.0.0")));
    let entries = lockfile.importers["."].dependencies.as_ref().unwrap();
    for alias in ["first", "second"] {
        assert_eq!(
            entries[&name(alias)].version.resolved_key(&name(alias)),
            Some(key("consumer@1.0.0")),
        );
    }
    assert_round_trip(&lockfile);
}

#[test]
fn rejects_unequal_snapshot_collisions_without_mutating_the_lockfile() {
    let mut lockfile = parse(
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      first:
        specifier: npm:consumer@1.0.0
        version: consumer@1.0.0(gone@1.0.0)
      second:
        specifier: npm:consumer@1.0.0
        version: consumer@1.0.0(gone@2.0.0)
packages:
  consumer@1.0.0:
    resolution: {integrity: sha512-AAAA}
  child@1.0.0:
    resolution: {integrity: sha512-BBBB}
snapshots:
  consumer@1.0.0(gone@1.0.0):
    dependencies:
      child: 1.0.0
  consumer@1.0.0(gone@2.0.0): {}
  child@1.0.0: {}
",
    );
    let before = lockfile.clone();

    let error = lockfile.normalize_peer_keys_for_next_format().unwrap_err();

    assert!(
        error.to_string().contains("unequal snapshots would share the rebuilt key consumer@1.0.0"),
        "{error}",
    );
    assert_eq!(lockfile, before);
}
