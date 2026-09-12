mod workspace;

mod lockfile;

mod catalogs;

mod installation;

mod resolution;

use pnpm_lockfile::{Lockfile, PackageKey};
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

/// `foo` is reached only through `qux`, which resolved `bar` as `foo`'s
/// peer, so the one snapshot of `foo` is the peer-suffixed variant while
/// its `packages:` key is the bare `foo@1.1.0`. An importer edge written
/// at that bare version would name a snapshot the lockfile does not hold.
const WITH_ONLY_A_PEER_VARIANT: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      bar:
        specifier: ^2.0.0
        version: 2.0.0
      qux:
        specifier: ^5.0.0
        version: 5.0.0
packages:
  bar@2.0.0:
    resolution:
      integrity: sha512-bar
  foo@1.1.0:
    resolution:
      integrity: sha512-foo
  qux@5.0.0:
    resolution:
      integrity: sha512-qux
snapshots:
  bar@2.0.0: {}
  foo@1.1.0(bar@2.0.0):
    dependencies:
      bar: 2.0.0
  qux@5.0.0:
    dependencies:
      foo: 1.1.0(bar@2.0.0)
";

/// [`WITH_ONLY_A_PEER_VARIANT`] with the importer already depending on a
/// peerless `foo@1.0.0`, so a range that admits `1.1.0` would move onto
/// the version held only as a peer variant.
fn with_a_lower_peerless_foo() -> Lockfile {
    let mut subject = parsed_lockfile(WITH_ONLY_A_PEER_VARIANT);
    let packages = subject.packages.as_mut().expect("packages");
    let metadata = packages[&"foo@1.1.0".parse::<PackageKey>().expect("package key")].clone();
    packages.insert("foo@1.0.0".parse().expect("package key"), metadata);
    subject.snapshots.as_mut().expect("snapshots").insert(
        "foo@1.0.0".parse().expect("snapshot key"),
        pnpm_lockfile::SnapshotEntry::default(),
    );
    let importer = subject.importers.get_mut(".").expect("importer");
    importer.dependencies.as_mut().expect("dependencies").insert(
        "foo".parse().expect("alias"),
        pnpm_lockfile::ResolvedDependencySpec {
            specifier: "1.0.0".to_string(),
            version: pnpm_lockfile::ImporterDepVersion::Regular("1.0.0".parse().expect("version")),
        },
    );
    subject
}

/// [`WITH_ONLY_A_PEER_VARIANT`] with a second snapshot of the same
/// version that resolved none of `foo`'s peers, the shape two parents
/// leave behind when only one of them provides `bar`.
fn with_a_bare_snapshot_beside_the_peer_variant() -> Lockfile {
    let mut subject = parsed_lockfile(WITH_ONLY_A_PEER_VARIANT);
    subject.snapshots.as_mut().expect("snapshots").insert(
        "foo@1.1.0".parse().expect("snapshot key"),
        pnpm_lockfile::SnapshotEntry::default(),
    );
    subject
}

/// [`WITH_ONLY_A_PEER_VARIANT`] with the importer depending on `foo`
/// directly at `recorded`.
fn with_a_direct_foo_at(recorded: &str) -> Lockfile {
    let mut subject = parsed_lockfile(WITH_ONLY_A_PEER_VARIANT);
    subject
        .importers
        .get_mut(".")
        .expect("importer")
        .dependencies
        .as_mut()
        .expect("dependencies")
        .insert(
            "foo".parse().expect("alias"),
            serde_saphyr::from_str(&format!("{{specifier: ^1.0.0, version: {recorded}}}"))
                .expect("dependency"),
        );
    subject
}
