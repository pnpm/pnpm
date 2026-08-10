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

#[test]
fn keeps_a_catalog_entry_referenced_with_the_catalog_default_spelling() {
    let mut lockfile = parsed_lockfile(WITH_CATALOG_DEP);
    let mut importer = lockfile.importers["."].clone();
    let alias: PkgName = "foo".parse().expect("alias");
    importer.dependencies.as_mut().expect("dependencies").get_mut(&alias).expect("foo").specifier =
        "catalog:default".to_string();
    lockfile.importers.insert("pkg-a".to_string(), importer);
    let manifest = manifest_from(json!({ "dependencies": { "bar": "^2.0.0" } }));
    let other =
        manifest_from(json!({ "dependencies": { "foo": "catalog:default", "bar": "^2.0.0" } }));

    let updated = try_fast_update_importers(
        &lockfile,
        &[(".".to_string(), &manifest), ("pkg-a".to_string(), &other)],
    )
    .expect("the other importer still references the catalog entry");

    assert!(
        updated.catalogs.as_ref().is_some_and(|catalogs| catalogs["default"].contains_key("foo")),
        "the catalog:default spelling counts as a reference to the default catalog",
    );
}

/// `bar` is a prod dependency reaching `child`; `opt` is an optional
/// dependency reaching the same `child`, which is therefore non-optional.
const WITH_SHARED_OPTIONAL_CHILD: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      bar:
        specifier: ^2.0.0
        version: 2.0.0
    optionalDependencies:
      opt:
        specifier: ^5.0.0
        version: 5.0.0
packages:
  bar@2.0.0:
    resolution:
      integrity: sha512-bar
  opt@5.0.0:
    resolution:
      integrity: sha512-opt
  child@3.0.0:
    resolution:
      integrity: sha512-child
snapshots:
  bar@2.0.0:
    dependencies:
      child: 3.0.0
  opt@5.0.0:
    optional: true
    dependencies:
      child: 3.0.0
  child@3.0.0: {}
";

fn snapshot_optional(lockfile: &Lockfile, key: &str) -> bool {
    lockfile.snapshots.as_ref().expect("snapshots")[&key.parse().expect("snapshot key")].optional
}

#[test]
fn moves_a_dependency_between_prod_and_dev_without_touching_snapshots() {
    let manifest = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0" },
        "devDependencies": { "bar": "^2.0.0" },
    }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_REMOVABLE_DEP),
        &[(".".to_string(), &manifest)],
    )
    .expect("a group move needs no resolution");

    let importer = &updated.importers["."];
    let alias: PkgName = "bar".parse().expect("alias");
    assert!(importer.dependencies.as_ref().is_some_and(|deps| !deps.contains_key(&alias)));
    let moved = &importer.dev_dependencies.as_ref().expect("devDependencies")[&alias];
    assert_eq!((moved.specifier.as_str(), moved.version.to_string().as_str()), ("^2.0.0", "2.0.0"));
    assert_eq!(updated.snapshots, parsed_lockfile(WITH_REMOVABLE_DEP).snapshots);
}

#[test]
fn marks_the_subtree_optional_on_a_move_into_optional_dependencies() {
    let manifest = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0" },
        "optionalDependencies": { "bar": "^2.0.0" },
    }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_REMOVABLE_DEP),
        &[(".".to_string(), &manifest)],
    )
    .expect("a group move needs no resolution");

    let alias: PkgName = "bar".parse().expect("alias");
    assert!(
        updated.importers["."]
            .optional_dependencies
            .as_ref()
            .is_some_and(|deps| deps.contains_key(&alias)),
    );
    assert!(snapshot_optional(&updated, "bar@2.0.0"));
    assert!(snapshot_optional(&updated, "child@3.0.0"));
    assert!(!snapshot_optional(&updated, "foo@1.1.0"));
}

#[test]
fn clears_the_subtree_flags_on_a_move_out_of_optional_dependencies() {
    let manifest = manifest_from(json!({
        "dependencies": { "bar": "^2.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));
    let mut subject = parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD);
    let importer = subject.importers.get_mut(".").expect("importer");
    let alias: PkgName = "bar".parse().expect("alias");
    let moved = importer.dependencies.as_mut().expect("dependencies").remove(&alias).expect("bar");
    importer.dependencies = None;
    importer
        .optional_dependencies
        .as_mut()
        .expect("optionalDependencies")
        .insert(alias.clone(), moved);
    let snapshots = subject.snapshots.as_mut().expect("snapshots");
    for key in ["bar@2.0.0", "child@3.0.0"] {
        snapshots.get_mut(&key.parse().expect("snapshot key")).expect("snapshot").optional = true;
    }

    let updated = try_fast_update_importers(&subject, &[(".".to_string(), &manifest)])
        .expect("a group move needs no resolution");

    assert!(
        updated.importers["."].dependencies.as_ref().is_some_and(|deps| deps.contains_key(&alias)),
    );
    assert!(!snapshot_optional(&updated, "bar@2.0.0"));
    assert!(!snapshot_optional(&updated, "child@3.0.0"), "bar reaches child non-optionally again");
    assert!(snapshot_optional(&updated, "opt@5.0.0"));
}

#[test]
fn stands_aside_when_every_dependency_is_in_its_recorded_group() {
    let manifest = manifest_from(json!({
        "dependencies": { "bar": "^2.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD),
        &[(".".to_string(), &manifest)],
    );

    assert!(updated.is_none(), "nothing changed, so the handler stands aside");
}

#[test]
fn keeps_a_child_another_prod_dependency_reaches_non_optional() {
    let mut subject = parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD);
    let importer = subject.importers.get_mut(".").expect("importer");
    importer.dependencies.as_mut().expect("dependencies").insert(
        "keeper".parse().expect("alias"),
        serde_saphyr::from_str("{specifier: ^6.0.0, version: 6.0.0}").expect("dependency"),
    );
    let snapshots = subject.snapshots.as_mut().expect("snapshots");
    snapshots.insert(
        "keeper@6.0.0".parse().expect("snapshot key"),
        serde_saphyr::from_str("dependencies:\n  child: 3.0.0").expect("snapshot"),
    );
    let manifest = manifest_from(json!({
        "dependencies": { "keeper": "^6.0.0" },
        "optionalDependencies": { "bar": "^2.0.0", "opt": "^5.0.0" },
    }));

    let updated = try_fast_update_importers(&subject, &[(".".to_string(), &manifest)])
        .expect("a group move needs no resolution");

    assert!(snapshot_optional(&updated, "bar@2.0.0"));
    assert!(
        !snapshot_optional(&updated, "child@3.0.0"),
        "keeper still reaches child through prod edges",
    );
}

#[test]
fn flips_a_shared_child_optional_when_a_removal_severs_the_prod_path() {
    let manifest = manifest_from(json!({ "optionalDependencies": { "opt": "^5.0.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD),
        &[(".".to_string(), &manifest)],
    )
    .expect("dropping an importer edge needs no resolution");

    assert!(
        snapshot_optional(&updated, "child@3.0.0"),
        "only the optional path reaches child once bar is gone",
    );
}

#[test]
fn records_an_alias_declared_in_both_prod_and_optional_as_optional() {
    let manifest = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0", "bar": "^2.0.0" },
        "optionalDependencies": { "bar": "^2.0.0" },
    }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_REMOVABLE_DEP),
        &[(".".to_string(), &manifest)],
    )
    .expect("a group move needs no resolution");

    let alias: PkgName = "bar".parse().expect("alias");
    assert!(
        updated.importers["."]
            .optional_dependencies
            .as_ref()
            .is_some_and(|deps| deps.contains_key(&alias)),
        "optional wins when the manifest declares both",
    );
}

#[test]
fn moves_a_group_alongside_a_satisfied_range_change() {
    let manifest = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0" },
        "devDependencies": { "bar": ">=2 <3" },
    }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_REMOVABLE_DEP),
        &[(".".to_string(), &manifest)],
    )
    .expect("both edits stay within the importer");

    let moved = &updated.importers["."].dev_dependencies.as_ref().expect("devDependencies")
        [&"bar".parse::<PkgName>().expect("alias")];
    assert_eq!(moved.specifier, ">=2 <3");
}

/// `foo` and `bar` are prod dependencies (`bar` reaching `child`), `qux`
/// is a dev dependency.
const WITH_THREE_GROUPS: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
      bar:
        specifier: ^2.0.0
        version: 2.0.0
    devDependencies:
      qux:
        specifier: ^5.0.0
        version: 5.0.0
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
  qux@5.0.0:
    resolution:
      integrity: sha512-qux
snapshots:
  foo@1.1.0: {}
  bar@2.0.0:
    dependencies:
      child: 3.0.0
  child@3.0.0: {}
  qux@5.0.0: {}
";

#[test]
fn moves_several_dependencies_between_groups_in_one_pass() {
    let manifest = manifest_from(json!({
        "dependencies": { "qux": "^5.0.0" },
        "devDependencies": { "foo": "^1.0.0" },
        "optionalDependencies": { "bar": "^2.0.0" },
    }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_THREE_GROUPS),
        &[(".".to_string(), &manifest)],
    )
    .expect("group moves need no resolution");

    let importer = &updated.importers["."];
    let recorded_aliases = |group: &Option<pacquet_lockfile::ResolvedDependencyMap>| {
        group
            .as_ref()
            .map(|dependencies| {
                let mut aliases: Vec<_> = dependencies.keys().map(ToString::to_string).collect();
                aliases.sort();
                aliases
            })
            .unwrap_or_default()
    };
    assert_eq!(recorded_aliases(&importer.dependencies), vec!["qux".to_string()]);
    assert_eq!(recorded_aliases(&importer.dev_dependencies), vec!["foo".to_string()]);
    assert_eq!(recorded_aliases(&importer.optional_dependencies), vec!["bar".to_string()]);
    assert!(snapshot_optional(&updated, "bar@2.0.0"));
    assert!(snapshot_optional(&updated, "child@3.0.0"));
    assert!(!snapshot_optional(&updated, "foo@1.1.0"));
    assert!(!snapshot_optional(&updated, "qux@5.0.0"));
}
