use pnpm_config::ResolutionMode::LowestDirect as LOWEST_DIRECT;
use pnpm_lockfile::{Lockfile, PackageKey, PkgName};
use pnpm_package_manifest::PackageManifest;
use serde_json::json;
use std::path::PathBuf;

/// The composed pipeline restricted to manifest drift: every other
/// input is neutral, so these tests exercise the importers handler and
/// the shared epilogue alone.
fn try_fast_update_importers(
    lockfile: &Lockfile,
    manifests: &[(String, &PackageManifest)],
) -> Option<Lockfile> {
    crate::fast_update_compose::try_compose_fast_updates(
        lockfile,
        manifests,
        &[],
        &pnpm_config::Config::default(),
        None,
        false,
    )
}

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
    let recorded_aliases = |group: &Option<pnpm_lockfile::ResolvedDependencyMap>| {
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

#[test]
fn rejects_a_dependency_the_lockfile_holds_no_version_of() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.0.0", "extra": "^1.0.0" } }));

    assert!(
        try_fast_update_importers(&lockfile(), &[(".".to_string(), &manifest)]).is_none(),
        "only the resolver can fetch a package the lockfile never saw",
    );
}

/// Two importers hold different versions of `foo`; the registry also has
/// a higher 1.3.0 that nothing locks.
const WITH_TWO_LOCKED_VERSIONS: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      foo:
        specifier: 1.0.0
        version: 1.0.0
  pkg-a:
    dependencies:
      foo:
        specifier: 1.2.0
        version: 1.2.0
packages:
  foo@1.0.0:
    resolution:
      integrity: sha512-foo-1
  foo@1.2.0:
    resolution:
      integrity: sha512-foo-2
snapshots:
  foo@1.0.0: {}
  foo@1.2.0: {}
";

#[test]
fn moves_a_widened_range_to_the_higher_version_another_importer_locks() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.1.0" } }));
    let other = manifest_from(json!({ "dependencies": { "foo": "1.2.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_TWO_LOCKED_VERSIONS),
        &[(".".to_string(), &manifest), ("pkg-a".to_string(), &other)],
    )
    .expect("the version is already in the lockfile, so nothing needs resolving");

    let alias: PkgName = "foo".parse().expect("alias");
    let recorded = &updated.importers["."].dependencies.as_ref().expect("dependencies")[&alias];
    assert_eq!(
        (recorded.specifier.as_str(), recorded.version.to_string().as_str()),
        ("^1.1.0", "1.2.0"),
    );
    let mut packages: Vec<_> =
        updated.packages.as_ref().expect("packages").keys().map(ToString::to_string).collect();
    packages.sort();
    assert_eq!(
        packages,
        vec!["foo@1.2.0".to_string()],
        "the version it left is unreachable and goes",
    );
}

#[test]
fn moves_to_a_higher_locked_version_even_when_the_locked_one_still_satisfies() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": ">=1.0.0" } }));
    let other = manifest_from(json!({ "dependencies": { "foo": "1.2.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_TWO_LOCKED_VERSIONS),
        &[(".".to_string(), &manifest), ("pkg-a".to_string(), &other)],
    )
    .expect("resolution would dedupe onto the higher locked version");

    let alias: PkgName = "foo".parse().expect("alias");
    assert_eq!(
        updated.importers["."].dependencies.as_ref().expect("dependencies")[&alias]
            .version
            .to_string(),
        "1.2.0",
    );
}

/// One project with an optional dependency whose child is optional with
/// it, a second that locks a higher `child` a `^3.0.0` range also
/// admits, and a third the lockfile records nothing for yet.
const WITH_A_NEW_PROJECT: &str = r"
lockfileVersion: '9.0'
importers:
  pkg-a:
    optionalDependencies:
      opt:
        specifier: ^5.0.0
        version: 5.0.0
  pkg-c:
    dependencies:
      child:
        specifier: 3.1.0
        version: 3.1.0
packages:
  opt@5.0.0:
    resolution:
      integrity: sha512-opt
  child@3.0.0:
    resolution:
      integrity: sha512-child
  child@3.1.0:
    resolution:
      integrity: sha512-child-1
snapshots:
  opt@5.0.0:
    optional: true
    dependencies:
      child: 3.0.0
  child@3.0.0:
    optional: true
  child@3.1.0: {}
";

/// The projects [`WITH_A_NEW_PROJECT`] already records, alongside the
/// `pkg-b` the tests add.
fn projects_of_a_new_project_lockfile<'a>(
    existing: &'a PackageManifest,
    locks_the_higher_child: &'a PackageManifest,
    added: &'a PackageManifest,
) -> Vec<(String, &'a PackageManifest)> {
    vec![
        ("pkg-a".to_string(), existing),
        ("pkg-c".to_string(), locks_the_higher_child),
        ("pkg-b".to_string(), added),
    ]
}

fn a_new_project_lockfile_projects(added: &PackageManifest) -> [PackageManifest; 2] {
    let _ = added;
    [
        manifest_from(json!({ "optionalDependencies": { "opt": "^5.0.0" } })),
        manifest_from(json!({ "dependencies": { "child": "3.1.0" } })),
    ]
}

#[test]
fn writes_a_new_project_importer_from_the_highest_locked_versions() {
    let added = manifest_from(json!({ "devDependencies": { "child": "^3.0.0" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_A_NEW_PROJECT),
        &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
    )
    .expect("every version the new project needs is already locked");

    let alias: PkgName = "child".parse().expect("alias");
    let recorded =
        &updated.importers["pkg-b"].dev_dependencies.as_ref().expect("devDependencies")[&alias];
    assert_eq!(
        (recorded.specifier.as_str(), recorded.version.to_string().as_str()),
        ("^3.0.0", "3.1.0"),
        "resolution dedupes onto the highest locked version the range admits",
    );
    assert!(
        snapshot_optional(&updated, "child@3.0.0"),
        "the version the new project did not take keeps the flag its only path gives it",
    );
    assert!(snapshot_optional(&updated, "opt@5.0.0"), "nothing else changed about the old path");
}

#[test]
fn clears_an_optional_flag_a_new_projects_plain_dependency_reaches() {
    let added = manifest_from(json!({ "dependencies": { "child": "3.0.0" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_A_NEW_PROJECT),
        &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
    )
    .expect("the version the new project pins is already locked");

    assert!(
        !snapshot_optional(&updated, "child@3.0.0"),
        "the new project reaches it outside optionalDependencies",
    );
}

#[test]
fn rejects_a_new_project_whose_dependency_is_not_locked() {
    let added = manifest_from(json!({ "dependencies": { "extra": "^1.0.0" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_A_NEW_PROJECT),
            &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
        )
        .is_none(),
        "only the resolver can fetch a version the lockfile does not hold",
    );
}

#[test]
fn rejects_a_new_project_that_declares_a_workspace_protocol_dependency() {
    let added = manifest_from(json!({ "dependencies": { "child": "workspace:^" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_A_NEW_PROJECT),
            &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
        )
        .is_none(),
        "a workspace dependency resolves to a directory, not to a locked version",
    );
}

#[test]
fn rejects_a_new_project_that_declares_a_link_protocol_dependency() {
    let added = manifest_from(json!({ "dependencies": { "child": "link:../child" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_A_NEW_PROJECT),
            &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
        )
        .is_none(),
        "a link resolves to a directory, not to a locked version",
    );
}

#[test]
fn rejects_a_new_project_that_depends_on_a_workspace_sibling() {
    let added = manifest_from(json!({ "dependencies": { "child": "^3.0.0" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);
    let sibling = PackageManifest::from_value(
        PathBuf::from("/workspace/child/package.json"),
        json!({ "name": "child", "version": "3.0.0" }),
    );

    assert!(
        crate::fast_update_compose::try_compose_fast_updates(
            &parsed_lockfile(WITH_A_NEW_PROJECT),
            &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
            &[(PathBuf::from("/workspace/child"), &sibling)],
            &pnpm_config::Config::default(),
            None,
            false,
        )
        .is_none(),
        "a plain range on a workspace project may resolve to a link, which only the resolver decides",
    );
}

#[test]
fn rejects_a_widened_range_when_resolution_would_pick_its_lowest_locked_version() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.0.0" } }));
    let other = manifest_from(json!({ "dependencies": { "foo": "1.2.0" } }));
    let config = pnpm_config::Config { resolution_mode: LOWEST_DIRECT, ..Default::default() };

    assert!(
        crate::fast_update_compose::try_compose_fast_updates(
            &parsed_lockfile(WITH_TWO_LOCKED_VERSIONS),
            &[(".".to_string(), &manifest), ("pkg-a".to_string(), &other)],
            &[],
            &config,
            None,
            false,
        )
        .is_none(),
        "which end of the range applies is not a property of the lockfile",
    );
}

#[test]
fn rejects_a_new_project_when_resolution_would_pick_the_lowest_of_several_locked_versions() {
    let added = manifest_from(json!({ "dependencies": { "child": "^3.0.0" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);
    let config = pnpm_config::Config { resolution_mode: LOWEST_DIRECT, ..Default::default() };

    assert!(
        crate::fast_update_compose::try_compose_fast_updates(
            &parsed_lockfile(WITH_A_NEW_PROJECT),
            &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
            &[],
            &config,
            None,
            false,
        )
        .is_none(),
        "which end of the range applies is not a property of the lockfile",
    );
}

#[test]
fn rejects_a_range_no_locked_version_satisfies() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^2.0.0" } }));
    let other = manifest_from(json!({ "dependencies": { "foo": "1.2.0" } }));

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_TWO_LOCKED_VERSIONS),
            &[(".".to_string(), &manifest), ("pkg-a".to_string(), &other)],
        )
        .is_none(),
        "only the resolver can fetch a version the lockfile does not hold",
    );
}

#[test]
fn rejects_a_higher_version_that_exists_only_under_a_named_registry() {
    let mut subject = parsed_lockfile(WITH_TWO_LOCKED_VERSIONS);
    let packages = subject.packages.as_mut().expect("packages");
    let higher = packages.remove(&"foo@1.2.0".parse().expect("package key")).expect("foo@1.2.0");
    packages.insert("foo@work:1.2.0".parse().expect("package key"), higher);
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.1.0" } }));

    assert!(
        try_fast_update_importers(&subject, &[(".".to_string(), &manifest)]).is_none(),
        "a registry-qualified key's semver only pins a version inside that registry",
    );
}

/// Two workspace members, each with a dependency of its own.
const WITH_TWO_IMPORTERS: &str = r"
lockfileVersion: '9.0'
importers:
  packages/a:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
  packages/b:
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
";

fn try_prune_stale_importers(
    lockfile: &Lockfile,
    manifests: &[(String, &PackageManifest)],
) -> Option<Lockfile> {
    crate::fast_update_compose::try_compose_fast_updates(
        lockfile,
        manifests,
        &[],
        &pnpm_config::Config::default(),
        None,
        true,
    )
}

#[test]
fn drops_the_importer_of_a_workspace_project_that_is_gone() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.0.0" } }));

    let updated = try_prune_stale_importers(
        &parsed_lockfile(WITH_TWO_IMPORTERS),
        &[("packages/a".to_string(), &manifest)],
    )
    .expect("dropping a project's importer needs no resolution");

    assert_eq!(updated.importers.keys().collect::<Vec<_>>(), vec!["packages/a"]);
    let mut packages: Vec<_> =
        updated.packages.as_ref().expect("packages").keys().map(ToString::to_string).collect();
    packages.sort();
    assert_eq!(packages, vec!["foo@1.1.0".to_string()], "what only it needed goes with it");
}

#[test]
fn keeps_the_importer_when_the_run_does_not_see_every_project() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.0.0" } }));

    assert!(
        crate::fast_update_compose::try_compose_fast_updates(
            &parsed_lockfile(WITH_TWO_IMPORTERS),
            &[("packages/a".to_string(), &manifest)],
            &[],
            &pnpm_config::Config::default(),
            None,
            false,
        )
        .is_none(),
        "a filtered run cannot tell a removed project from an unselected one",
    );
}

#[test]
fn rejects_dropping_an_importer_a_survivor_links_to() {
    let mut subject = parsed_lockfile(WITH_TWO_IMPORTERS);
    subject
        .importers
        .get_mut("packages/a")
        .expect("importer")
        .dependencies
        .as_mut()
        .expect("dependencies")
        .insert(
            "b".parse().expect("alias"),
            serde_saphyr::from_str("{specifier: workspace:1.0.0, version: link:../b}")
                .expect("dependency"),
        );
    let manifest =
        manifest_from(json!({ "dependencies": { "foo": "^1.0.0", "b": "workspace:1.0.0" } }));

    assert!(
        try_prune_stale_importers(&subject, &[("packages/a".to_string(), &manifest)]).is_none(),
        "a project that is gone while something links to it is a broken workspace",
    );
}

/// The importer depends on `foo@1.0.0` directly, while `baz` — reached
/// through `qux` — resolves `foo` as a peer at the version `qux`
/// provides, `1.2.0`.
const WITH_PEER_ON_ANOTHER_VERSION: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      foo:
        specifier: 1.0.0
        version: 1.0.0
      qux:
        specifier: ^5.0.0
        version: 5.0.0
packages:
  foo@1.0.0:
    resolution:
      integrity: sha512-foo-1
  foo@1.2.0:
    resolution:
      integrity: sha512-foo-2
  qux@5.0.0:
    resolution:
      integrity: sha512-qux
  baz@4.0.0:
    resolution:
      integrity: sha512-baz
snapshots:
  foo@1.0.0: {}
  foo@1.2.0: {}
  qux@5.0.0:
    dependencies:
      foo: 1.2.0
      baz: 4.0.0(foo@1.2.0)
  baz@4.0.0(foo@1.2.0):
    dependencies:
      foo: 1.2.0
";

/// `baz` resolves `qux` as a peer, which in turn resolved `foo`, so the
/// dropped `foo@1.1.0` is named one level down in `baz`'s key.
const WITH_NESTED_PEER_ON_REMOVABLE_DEP: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
      baz:
        specifier: ^4.0.0
        version: 4.0.0(qux@5.0.0(foo@1.1.0))
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-foo
  baz@4.0.0:
    resolution:
      integrity: sha512-baz
snapshots:
  foo@1.1.0: {}
  baz@4.0.0(qux@5.0.0(foo@1.1.0)): {}
";

/// `foo` is a workspace sibling `baz` resolves as a peer. A link has no
/// `name@version` for a suffix segment to be compared against — the
/// segment carries the filename-safe form of the link path instead.
const WITH_PEER_ON_REMOVABLE_LINK: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      foo:
        specifier: 'workspace:*'
        version: link:packages/foo
      baz:
        specifier: ^4.0.0
        version: 4.0.0(foo@packages+foo)
packages:
  baz@4.0.0:
    resolution:
      integrity: sha512-baz
snapshots:
  baz@4.0.0(foo@packages+foo): {}
";

fn sorted_snapshot_keys(lockfile: &Lockfile) -> Vec<String> {
    let mut keys: Vec<_> =
        lockfile.snapshots.as_ref().expect("snapshots").keys().map(ToString::to_string).collect();
    keys.sort();
    keys
}

#[test]
fn drops_a_dependency_whose_version_no_surviving_peer_suffix_names() {
    let manifest = manifest_from(json!({ "dependencies": { "qux": "^5.0.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_PEER_ON_ANOTHER_VERSION),
        &[(".".to_string(), &manifest)],
    )
    .expect("the surviving suffix names the version qux provides, not the dropped one");

    assert_eq!(
        sorted_snapshot_keys(&updated),
        vec!["baz@4.0.0(foo@1.2.0)".to_string(), "foo@1.2.0".to_string(), "qux@5.0.0".to_string()],
    );
}

#[test]
fn drops_a_dependency_a_surviving_suffix_only_ends_with_the_name_of() {
    let mut subject = parsed_lockfile(WITH_PEER_ON_ANOTHER_VERSION);
    let snapshots = subject.snapshots.as_mut().expect("snapshots");
    snapshots.insert(
        "baz@4.0.0(@scope/foo@1.0.0)".parse().expect("snapshot key"),
        serde_saphyr::from_str("dependencies:\n  '@scope/foo': 1.0.0").expect("snapshot"),
    );
    snapshots.insert(
        "@scope/foo@1.0.0".parse().expect("snapshot key"),
        pnpm_lockfile::SnapshotEntry::default(),
    );
    snapshots
        .get_mut(&"qux@5.0.0".parse().expect("snapshot key"))
        .expect("qux")
        .dependencies
        .as_mut()
        .expect("dependencies")
        .insert(
            "baz".parse().expect("alias"),
            "4.0.0(@scope/foo@1.0.0)".parse().expect("reference"),
        );
    let manifest = manifest_from(json!({ "dependencies": { "qux": "^5.0.0" } }));

    assert!(
        try_fast_update_importers(&subject, &[(".".to_string(), &manifest)]).is_some(),
        "@scope/foo is not foo, however the two names end",
    );
}

#[test]
fn rejects_dropping_a_dependency_a_nested_peer_suffix_segment_names() {
    let manifest = manifest_from(json!({ "dependencies": { "baz": "^4.0.0" } }));

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_NESTED_PEER_ON_REMOVABLE_DEP),
            &[(".".to_string(), &manifest)],
        )
        .is_none(),
        "the peers of a peer are as much a part of baz's key as the top-level ones",
    );
}

/// A peer suffix reaches the guard verbatim from the lockfile, so a
/// segment may start with a character no package name would.
const WITH_NON_ASCII_PEER_SUFFIX: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
      baz:
        specifier: ^4.0.0
        version: 4.0.0(é@1.0.0)
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-foo
  baz@4.0.0:
    resolution:
      integrity: sha512-baz
snapshots:
  foo@1.1.0: {}
  baz@4.0.0(é@1.0.0): {}
";

#[test]
fn reads_a_peer_suffix_segment_that_starts_with_a_multi_byte_character() {
    let manifest = manifest_from(json!({ "dependencies": { "baz": "^4.0.0" } }));

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_NON_ASCII_PEER_SUFFIX),
            &[(".".to_string(), &manifest)],
        )
        .is_some(),
        "the segment names neither foo nor anything else dropped",
    );
}

#[test]
fn rejects_dropping_a_linked_dependency_a_surviving_peer_suffix_names() {
    let manifest = manifest_from(json!({ "dependencies": { "baz": "^4.0.0" } }));

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_PEER_ON_REMOVABLE_LINK),
            &[(".".to_string(), &manifest)],
        )
        .is_none(),
        "nothing pins the link to a version, so every suffix naming it stays suspect",
    );
}

#[test]
fn moves_a_range_past_a_peer_suffix_naming_the_version_it_moves_to() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.1.0", "qux": "^5.0.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_PEER_ON_ANOTHER_VERSION),
        &[(".".to_string(), &manifest)],
    )
    .expect("baz already resolves the peer to the version the importer moves to");

    let alias: PkgName = "foo".parse().expect("alias");
    assert_eq!(
        updated.importers["."].dependencies.as_ref().expect("dependencies")[&alias]
            .version
            .to_string(),
        "1.2.0",
    );
    assert_eq!(
        sorted_snapshot_keys(&updated),
        vec!["baz@4.0.0(foo@1.2.0)".to_string(), "foo@1.2.0".to_string(), "qux@5.0.0".to_string()],
    );
}

#[test]
fn rejects_a_range_move_a_peer_suffix_names_the_version_it_moves_off() {
    let mut subject = parsed_lockfile(WITH_PEER_ON_ANOTHER_VERSION);
    subject
        .importers
        .get_mut(".")
        .expect("importer")
        .dependencies
        .as_mut()
        .expect("dependencies")
        .insert(
            "baz".parse().expect("alias"),
            serde_saphyr::from_str("{specifier: ^4.0.0, version: 4.0.0(foo@1.0.0)}")
                .expect("dependency"),
        );
    subject.snapshots.as_mut().expect("snapshots").insert(
        "baz@4.0.0(foo@1.0.0)".parse().expect("snapshot key"),
        serde_saphyr::from_str("dependencies:\n  foo: 1.0.0").expect("snapshot"),
    );
    let manifest = manifest_from(
        json!({ "dependencies": { "foo": "^1.1.0", "qux": "^5.0.0", "baz": "^4.0.0" } }),
    );

    assert!(
        try_fast_update_importers(&subject, &[(".".to_string(), &manifest)]).is_none(),
        "baz resolved the peer to the version the importer moves off, so its key would change",
    );
}

#[test]
fn rejects_a_range_when_the_alias_also_has_a_named_registry_key() {
    let mut subject = parsed_lockfile(WITH_TWO_LOCKED_VERSIONS);
    let packages = subject.packages.as_mut().expect("packages");
    let extra = packages[&"foo@1.2.0".parse::<PackageKey>().expect("package key")].clone();
    packages.insert("foo@work:1.4.0".parse().expect("package key"), extra);
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.1.0" } }));

    assert!(
        try_fast_update_importers(&subject, &[(".".to_string(), &manifest)]).is_none(),
        "the alias spans two registries, so a plain reference cannot say which is meant",
    );
}

#[test]
fn adds_a_dependency_at_the_highest_locked_version_satisfying_it() {
    let subject = with_a_second_locked_child();
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "opt": "^5.0.0", "child": "^3.0.0" } }),
    );
    let updated = try_fast_update_importers(&subject, &[(".".to_string(), &manifest)])
        .expect("the lockfile already holds a version satisfying the new dependency");

    let dependencies = updated.importers["."].dependencies.as_ref().expect("dependencies");
    let added = &dependencies[&"child".parse::<PkgName>().expect("alias")];
    assert_eq!(added.specifier, "^3.0.0");
    assert_eq!(added.version.to_string(), "3.1.0");
}

#[test]
fn adding_a_dependency_clears_the_optional_flag_of_what_it_reaches() {
    let mut subject = parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD);
    let importer = subject.importers.get_mut(".").expect("importer");
    importer.dependencies = None;
    let snapshots = subject.snapshots.as_mut().expect("snapshots");
    for key in ["bar@2.0.0", "child@3.0.0"] {
        snapshots.get_mut(&key.parse().expect("snapshot key")).expect("snapshot").optional = true;
    }
    let manifest = manifest_from(json!({
        "dependencies": { "child": "^3.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));

    let updated = try_fast_update_importers(&subject, &[(".".to_string(), &manifest)])
        .expect("the added dependency is already locked");

    assert!(!snapshot_optional(&updated, "child@3.0.0"), "a prod path now reaches child");
}

#[test]
fn adding_an_optional_dependency_leaves_the_flags_alone() {
    let mut subject = parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD);
    let importer = subject.importers.get_mut(".").expect("importer");
    importer.dependencies = None;
    let snapshots = subject.snapshots.as_mut().expect("snapshots");
    for key in ["bar@2.0.0", "child@3.0.0"] {
        snapshots.get_mut(&key.parse().expect("snapshot key")).expect("snapshot").optional = true;
    }
    let manifest =
        manifest_from(json!({ "optionalDependencies": { "opt": "^5.0.0", "child": "^3.0.0" } }));

    let updated = try_fast_update_importers(&subject, &[(".".to_string(), &manifest)])
        .expect("the added dependency is already locked");

    assert!(snapshot_optional(&updated, "child@3.0.0"));
}

#[test]
fn rejects_adding_a_dependency_no_locked_version_satisfies() {
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "opt": "^5.0.0", "child": "^4.0.0" } }),
    );

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD),
            &[(".".to_string(), &manifest)],
        )
        .is_none(),
        "only the resolver can fetch a version the lockfile does not hold",
    );
}

/// [`WITH_SHARED_OPTIONAL_CHILD`] with a second version of `child` locked, so
/// which end of a range satisfying both is picked becomes observable.
fn with_a_second_locked_child() -> Lockfile {
    let mut subject = parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD);
    let packages = subject.packages.as_mut().expect("packages");
    let metadata = packages[&"child@3.0.0".parse::<PackageKey>().expect("package key")].clone();
    packages.insert("child@3.1.0".parse().expect("package key"), metadata);
    let snapshots = subject.snapshots.as_mut().expect("snapshots");
    let snapshot = snapshots[&"child@3.0.0".parse().expect("snapshot key")].clone();
    snapshots.insert("child@3.1.0".parse().expect("snapshot key"), snapshot);
    subject
}

#[test]
fn rejects_adding_a_dependency_naming_a_workspace_project() {
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "opt": "^5.0.0", "child": "^3.0.0" } }),
    );
    let sibling = manifest_from(json!({ "name": "child" }));

    assert!(
        crate::fast_update_compose::try_compose_fast_updates(
            &parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD),
            &[(".".to_string(), &manifest)],
            &[(PathBuf::from("/child/package.json"), &sibling)],
            &pnpm_config::Config::default(),
            None,
            false,
        )
        .is_none(),
        "only the resolver decides whether a workspace project is linked",
    );
}

#[test]
fn rejects_adding_a_dependency_several_locked_versions_satisfy_when_resolution_picks_lowest() {
    let subject = with_a_second_locked_child();
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "opt": "^5.0.0", "child": "^3.0.0" } }),
    );
    let config = pnpm_config::Config { resolution_mode: LOWEST_DIRECT, ..Default::default() };

    assert!(
        crate::fast_update_compose::try_compose_fast_updates(
            &subject,
            &[(".".to_string(), &manifest)],
            &[],
            &config,
            None,
            false,
        )
        .is_none(),
        "which end of the range a direct dependency takes is not a property of the lockfile",
    );
}

#[test]
fn rejects_adding_a_dependency_to_a_lockfile_that_records_publish_dates() {
    let mut subject = parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD);
    subject.time = Some(
        [
            ("bar@2.0.0".to_string(), "2020-01-01T00:00:00.000Z".to_string()),
            ("opt@5.0.0".to_string(), "2020-01-01T00:00:00.000Z".to_string()),
        ]
        .into_iter()
        .collect(),
    );
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "opt": "^5.0.0", "child": "^3.0.0" } }),
    );

    assert!(
        try_fast_update_importers(&subject, &[(".".to_string(), &manifest)]).is_none(),
        "only a resolution can record the publish date of a new direct dependency",
    );
}

const TWO_IMPORTERS: &str = r"
lockfileVersion: '9.0'
importers:
  a:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
  b:
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
";

#[test]
fn a_resolve_needing_importer_vetoes_absorbable_siblings_in_either_order() {
    let lockfile = parsed_lockfile(TWO_IMPORTERS);
    let absorbable = manifest(">=1 <2");
    let needs_resolve = manifest("^2");
    assert!(
        try_fast_update_importers(
            &lockfile,
            &[("a".to_string(), &absorbable), ("b".to_string(), &needs_resolve)],
        )
        .is_none(),
        "needs-resolve after absorbable must veto",
    );
    assert!(
        try_fast_update_importers(
            &lockfile,
            &[("a".to_string(), &needs_resolve), ("b".to_string(), &absorbable)],
        )
        .is_none(),
        "needs-resolve before absorbable must veto",
    );

    // Control: with both importers absorbable the compose applies.
    let clean = manifest("^1.0.0");
    let updated = try_fast_update_importers(
        &lockfile,
        &[("a".to_string(), &absorbable), ("b".to_string(), &clean)],
    )
    .expect("absorbable + clean should compose");
    assert_eq!(
        updated.importers["a"].dependencies.as_ref().expect("dependencies")
            [&"foo".parse().expect("package name")]
            .specifier,
        ">=1 <2",
    );
}
