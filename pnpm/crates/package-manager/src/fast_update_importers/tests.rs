use super::try_fast_update_importers;
use pacquet_lockfile::{Lockfile, PkgName};
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

fn parsed_lockfile(source: &str) -> Lockfile {
    serde_saphyr::from_str(source).expect("parse lockfile")
}

fn manifest_from(value: serde_json::Value) -> PackageManifest {
    PackageManifest::from_value(PathBuf::from("/project/package.json"), value)
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

const WITH_REMOVABLE_DEP: &str = r"
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

/// The same graph where `baz` resolves `foo` as a peer, so `foo`'s id is
/// embedded in `baz`'s key.
const WITH_PEER_ON_REMOVABLE_DEP: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    specifiers:
      foo: ^1.0.0
      baz: ^4.0.0
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
      baz:
        specifier: ^4.0.0
        version: 4.0.0(foo@1.1.0)
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-foo
  baz@4.0.0:
    resolution:
      integrity: sha512-baz
snapshots:
  foo@1.1.0: {}
  baz@4.0.0(foo@1.1.0):
    dependencies:
      foo: 1.1.0
";

#[test]
fn drops_a_dependency_the_manifest_no_longer_declares() {
    let manifest = manifest_from(json!({ "dependencies": { "bar": "^2.0.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_REMOVABLE_DEP),
        &[(".".to_string(), &manifest)],
    )
    .expect("dropping an importer edge needs no resolution");

    let importer = &updated.importers["."];
    let dependencies = importer.dependencies.as_ref().expect("dependencies");
    assert!(!dependencies.contains_key(&"foo".parse::<PkgName>().expect("alias")));
    assert!(dependencies.contains_key(&"bar".parse::<PkgName>().expect("alias")));
    assert_eq!(
        importer.specifiers.as_ref().expect("specifiers").keys().collect::<Vec<_>>(),
        vec!["bar"],
    );
    let mut packages: Vec<_> =
        updated.packages.as_ref().expect("packages").keys().map(ToString::to_string).collect();
    packages.sort();
    assert_eq!(
        packages,
        vec!["bar@2.0.0".to_string(), "child@3.0.0".to_string()],
        "the dropped package goes with its subtree, and shared entries stay",
    );
}

#[test]
fn rejects_dropping_a_dependency_another_package_resolves_as_a_peer() {
    let manifest = manifest_from(json!({ "dependencies": { "baz": "^4.0.0" } }));

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_PEER_ON_REMOVABLE_DEP),
            &[(".".to_string(), &manifest)],
        )
        .is_none(),
        "baz's key embeds foo, so dropping foo rekeys baz rather than only pruning",
    );
}

/// `foo` is referenced through the default catalog by the sole importer;
/// `bar` is a plain dependency.
const WITH_CATALOG_DEP: &str = r"
lockfileVersion: '9.0'
catalogs:
  default:
    foo:
      specifier: ^1.0.0
      version: 1.1.0
importers:
  .:
    specifiers:
      foo: 'catalog:'
      bar: ^2.0.0
    dependencies:
      foo:
        specifier: 'catalog:'
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
snapshots:
  foo@1.1.0: {}
  bar@2.0.0: {}
";

/// `baz` resolves `foo` as a peer, and nothing else depends on `baz`,
/// so dropping both from the manifest leaves no snapshot that embeds
/// `foo`.
const WITH_REMOVABLE_PEER_PAIR: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    specifiers:
      foo: ^1.0.0
      baz: ^4.0.0
      bar: ^2.0.0
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
      baz:
        specifier: ^4.0.0
        version: 4.0.0(foo@1.1.0)
      bar:
        specifier: ^2.0.0
        version: 2.0.0
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-foo
  baz@4.0.0:
    resolution:
      integrity: sha512-baz
  bar@2.0.0:
    resolution:
      integrity: sha512-bar
snapshots:
  foo@1.1.0: {}
  baz@4.0.0(foo@1.1.0):
    dependencies:
      foo: 1.1.0
  bar@2.0.0: {}
";

/// A surviving snapshot whose peer suffix pnpm shortened into a hash.
const WITH_HASHED_PEER_SUFFIX: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    specifiers:
      foo: ^1.0.0
      baz: ^4.0.0
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
      baz:
        specifier: ^4.0.0
        version: 4.0.0(sha256-abcdef)
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-foo
  baz@4.0.0:
    resolution:
      integrity: sha512-baz
snapshots:
  foo@1.1.0: {}
  baz@4.0.0(sha256-abcdef): {}
";

#[test]
fn prunes_a_catalog_entry_its_last_referent_dropped() {
    let manifest = manifest_from(json!({ "dependencies": { "bar": "^2.0.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_CATALOG_DEP),
        &[(".".to_string(), &manifest)],
    )
    .expect("dropping the catalog referent needs no resolution");

    assert!(updated.catalogs.is_none(), "the orphaned catalog entry goes with its referent");
}

#[test]
fn keeps_a_catalog_entry_another_importer_references() {
    let mut lockfile = parsed_lockfile(WITH_CATALOG_DEP);
    let importer = lockfile.importers["."].clone();
    lockfile.importers.insert("pkg-a".to_string(), importer);
    let manifest = manifest_from(json!({ "dependencies": { "bar": "^2.0.0" } }));
    let other = manifest_from(json!({ "dependencies": { "foo": "catalog:", "bar": "^2.0.0" } }));

    let updated = try_fast_update_importers(
        &lockfile,
        &[(".".to_string(), &manifest), ("pkg-a".to_string(), &other)],
    )
    .expect("the other importer still references the catalog entry");

    assert!(
        updated.catalogs.as_ref().is_some_and(|catalogs| catalogs["default"].contains_key("foo")),
        "the still-referenced catalog entry stays",
    );
}

#[test]
fn drops_a_peer_pair_removed_together() {
    let manifest = manifest_from(json!({ "dependencies": { "bar": "^2.0.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_REMOVABLE_PEER_PAIR),
        &[(".".to_string(), &manifest)],
    )
    .expect("the peer-dependent snapshot is unreachable after the removal, so nothing rekeys");

    let mut packages: Vec<_> =
        updated.packages.as_ref().expect("packages").keys().map(ToString::to_string).collect();
    packages.sort();
    assert_eq!(packages, vec!["bar@2.0.0".to_string()]);
}

#[test]
fn rejects_dropping_a_dependency_when_a_surviving_suffix_is_hashed() {
    let manifest = manifest_from(json!({ "dependencies": { "baz": "^4.0.0" } }));

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_HASHED_PEER_SUFFIX),
            &[(".".to_string(), &manifest)],
        )
        .is_none(),
        "a shortened suffix cannot be checked for the dropped package",
    );
}
