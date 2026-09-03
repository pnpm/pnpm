/// The composed pipeline restricted to `patchedDependencies` drift:
/// every other input is neutral, so these tests exercise this handler
/// alone.
fn try_fast_update_patched_dependencies(lockfile: &Lockfile, config: &Config) -> Option<Lockfile> {
    {
        // As the caller does: an unreadable patch file declines the whole
        // attempt rather than reading as no patches at all.
        let hashes = config.patched_dependency_hashes().ok()?;
        crate::fast_update_compose::try_compose_fast_updates(
            lockfile,
            &[],
            &[],
            config,
            hashes.as_ref(),
            false,
        )
    }
}
use indexmap::IndexMap;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use std::{collections::BTreeMap, fs, path::Path};
use tempfile::TempDir;

const LOCKFILE: &str = r"
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
";

/// The same graph after a patch for `foo` was applied: the snapshot key
/// carries the `(patch_hash=...)` segment the patch contributed.
const PATCHED_LOCKFILE: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0(patch_hash=deadbeef)
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-deadbeef
snapshots:
  foo@1.1.0(patch_hash=deadbeef): {}
";

const PATCHED_GIT_LOCKFILE: &str = r"
lockfileVersion: '9.0'
importers:
  .: {}
packages:
  foo@git+file:///repo#0123456789012345678901234567890123456789:
    resolution: {type: git, repo: file:///repo, commit: '0123456789012345678901234567890123456789'}
    version: 1.0.0
snapshots:
  foo@git+file:///repo#0123456789012345678901234567890123456789(patch_hash=PATCH_HASH): {}
";

/// `foo` comes from a named registry, so the key's version slot holds
/// `work:1.1.0` rather than a plain semver the patch keys can match.
const REGISTRY_QUALIFIED_LOCKFILE: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: work:1.1.0
packages:
  foo@work:1.1.0:
    resolution:
      integrity: sha512-deadbeef
snapshots:
  foo@work:1.1.0: {}
";

/// A tarball resolution, whose key carries a URL where a version would be.
const TARBALL_LOCKFILE: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      bar:
        specifier: ^2.0.0
        version: 2.0.0
      foo:
        specifier: https://example.test/foo.tgz
        version: https://example.test/foo.tgz
packages:
  bar@2.0.0:
    resolution:
      integrity: sha512-deadbeef
  foo@https://example.test/foo.tgz:
    resolution:
      integrity: sha512-deadbeef
snapshots:
  bar@2.0.0: {}
  foo@https://example.test/foo.tgz: {}
";

/// `bar` depends on `foo` as a plain transitive dependency, so patching
/// `foo` has to move the reference inside `bar`'s snapshot too.
const TRANSITIVE_LOCKFILE: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      bar:
        specifier: ^2.0.0
        version: 2.0.0
packages:
  bar@2.0.0:
    resolution:
      integrity: sha512-deadbeef
  foo@1.1.0:
    resolution:
      integrity: sha512-deadbeef
snapshots:
  bar@2.0.0:
    dependencies:
      foo: 1.1.0
  foo@1.1.0: {}
";

/// `bar` reaches `foo` as a peer, so `foo`'s depPath is embedded in
/// `bar`'s own key.
const PEER_LOCKFILE: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      bar:
        specifier: ^2.0.0
        version: 2.0.0(foo@1.1.0)
packages:
  bar@2.0.0:
    resolution:
      integrity: sha512-deadbeef
  foo@1.1.0:
    resolution:
      integrity: sha512-deadbeef
snapshots:
  bar@2.0.0(foo@1.1.0):
    dependencies:
      foo: 1.1.0
  foo@1.1.0: {}
";

/// The same shape once the joined peer segments exceeded
/// `peersSuffixMaxLength` and pnpm replaced them with a short hash.
const HASHED_PEER_LOCKFILE: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      bar:
        specifier: ^2.0.0
        version: 2.0.0(sha256-abcdef)
packages:
  bar@2.0.0:
    resolution:
      integrity: sha512-deadbeef
  foo@1.1.0:
    resolution:
      integrity: sha512-deadbeef
snapshots:
  bar@2.0.0(sha256-abcdef):
    dependencies:
      foo: 1.1.0
  foo@1.1.0: {}
";

fn lockfile(source: &str) -> Lockfile {
    serde_saphyr::from_str(source).expect("parse lockfile")
}

fn snapshot_keys(lockfile: &Lockfile) -> Vec<String> {
    let mut keys: Vec<_> =
        lockfile.snapshots.as_ref().expect("snapshots").keys().map(ToString::to_string).collect();
    keys.sort();
    keys
}

fn package_keys(lockfile: &Lockfile) -> Vec<String> {
    let mut keys: Vec<_> =
        lockfile.packages.as_ref().expect("packages").keys().map(ToString::to_string).collect();
    keys.sort();
    keys
}

fn snapshot_dependency(lockfile: &Lockfile, key: &str, alias: &str) -> String {
    lockfile.snapshots.as_ref().expect("snapshots")[&key.parse().expect("parse snapshot key")]
        .dependencies
        .as_ref()
        .expect("snapshot dependencies")[&alias.parse().expect("parse alias")]
        .to_string()
}

fn importer_version(lockfile: &Lockfile, alias: &str) -> String {
    lockfile.importers["."].dependencies.as_ref().expect("importer dependencies")
        [&alias.parse().expect("parse alias")]
        .version
        .to_string()
}

/// A workspace whose patch files exist on disk, since both the hash map
/// and the grouped record are read from them.
fn workspace(patches: &[&str]) -> TempDir {
    let dir = tempfile::tempdir().expect("create workspace dir");
    fs::create_dir_all(dir.path().join("patches")).expect("create patches dir");
    for patch in patches {
        write_patch(dir.path(), patch, "--- a\n+++ b\n");
    }
    dir
}

fn write_patch(workspace_dir: &Path, key: &str, contents: &str) {
    let path = workspace_dir.join("patches").join(patch_file_name(key));
    fs::write(path, contents).expect("write patch file");
}

fn patch_file_name(key: &str) -> String {
    format!("{}.patch", key.replace('/', "+"))
}

fn config(workspace_dir: &Path, keys: &[&str], allow_unused_patches: bool) -> Config {
    Config {
        workspace_dir: Some(workspace_dir.to_path_buf()),
        allow_unused_patches,
        patched_dependencies: (!keys.is_empty()).then(|| {
            keys.iter()
                .map(|key| (key.to_string(), format!("patches/{}", patch_file_name(key))))
                .collect::<IndexMap<_, _>>()
        }),
        ..Config::default()
    }
}

fn recorded(lockfile: &Lockfile) -> &BTreeMap<String, String> {
    lockfile.patched_dependencies.as_ref().expect("the candidate records patchedDependencies")
}

#[test]
fn records_a_patch_that_matches_no_locked_package() {
    let dir = workspace(&["bar@2.0.0"]);

    let updated = try_fast_update_patched_dependencies(
        &lockfile(LOCKFILE),
        &config(dir.path(), &["bar@2.0.0"], true),
    )
    .expect("a patch matching nothing in the lockfile cannot change the graph");

    assert_eq!(recorded(&updated).keys().collect::<Vec<_>>(), vec!["bar@2.0.0"]);
}

#[test]
fn rekeys_a_locked_package_the_patch_matches() {
    let dir = workspace(&["foo@1.1.0"]);
    let config = config(dir.path(), &["foo@1.1.0"], true);
    let hash = config
        .patched_dependency_hashes()
        .expect("hash the patch files")
        .expect("a configured patch")["foo@1.1.0"]
        .clone();

    let updated = try_fast_update_patched_dependencies(&lockfile(LOCKFILE), &config)
        .expect("the patch only renames the snapshot, it does not change the graph");

    assert_eq!(snapshot_keys(&updated), vec![format!("foo@1.1.0(patch_hash={hash})")]);
    assert_eq!(
        importer_version(&updated, "foo"),
        format!("1.1.0(patch_hash={hash})"),
        "the importer points at the renamed snapshot",
    );
    assert_eq!(
        package_keys(&updated),
        vec!["foo@1.1.0".to_string()],
        "`packages:` is keyed without the patch hash, so it does not move",
    );
}

#[test]
fn rekeys_a_locked_package_a_bare_name_patch_matches() {
    let dir = workspace(&["foo"]);

    let updated = try_fast_update_patched_dependencies(
        &lockfile(LOCKFILE),
        &config(dir.path(), &["foo"], true),
    )
    .expect("a bare-name key matches every version of the package");

    assert!(snapshot_keys(&updated)[0].contains("(patch_hash="));
}

#[test]
fn unpatches_a_locked_package_when_its_patch_is_removed() {
    let dir = workspace(&[]);
    let mut lockfile = lockfile(PATCHED_LOCKFILE);
    lockfile.patched_dependencies =
        Some(BTreeMap::from([("foo@1.1.0".to_string(), "deadbeef".to_string())]));

    let updated = try_fast_update_patched_dependencies(&lockfile, &config(dir.path(), &[], true))
        .expect("dropping the patch renames the snapshot back");

    assert_eq!(snapshot_keys(&updated), vec!["foo@1.1.0".to_string()]);
    assert_eq!(importer_version(&updated, "foo"), "1.1.0");
    assert!(updated.patched_dependencies.is_none());
}

#[test]
fn moves_a_dependents_reference_to_the_rekeyed_package() {
    let dir = workspace(&["foo@1.1.0"]);
    let config = config(dir.path(), &["foo@1.1.0"], true);
    let hash = config
        .patched_dependency_hashes()
        .expect("hash the patch files")
        .expect("a configured patch")["foo@1.1.0"]
        .clone();

    let updated = try_fast_update_patched_dependencies(&lockfile(TRANSITIVE_LOCKFILE), &config)
        .expect("patching a transitive dependency only renames it");

    assert_eq!(
        snapshot_dependency(&updated, "bar@2.0.0", "foo"),
        format!("1.1.0(patch_hash={hash})"),
        "the dependent points at the renamed snapshot",
    );
    assert_eq!(importer_version(&updated, "bar"), "2.0.0", "the dependent itself does not move");
}

#[test]
fn rejects_a_patch_for_a_registry_qualified_package() {
    let dir = workspace(&["foo@1.1.0"]);

    assert!(
        try_fast_update_patched_dependencies(
            &lockfile(REGISTRY_QUALIFIED_LOCKFILE),
            &config(dir.path(), &["foo@1.1.0"], true),
        )
        .is_none(),
        "the resolver matches this patch on plain semver, so it decides this one",
    );
}

#[test]
fn rejects_a_bare_name_patch_that_would_reach_a_tarball_resolution() {
    let dir = workspace(&["foo"]);

    assert!(
        try_fast_update_patched_dependencies(
            &lockfile(TARBALL_LOCKFILE),
            &config(dir.path(), &["foo"], true),
        )
        .is_none(),
        "a URL in the version slot is not a version the resolver would match on",
    );
}

#[test]
fn rekeys_around_a_tarball_resolution_no_patch_reaches() {
    let dir = workspace(&["bar@2.0.0"]);

    let updated = try_fast_update_patched_dependencies(
        &lockfile(TARBALL_LOCKFILE),
        &config(dir.path(), &["bar@2.0.0"], true),
    )
    .expect("an untouched tarball resolution does not block the rest");

    assert!(snapshot_keys(&updated).iter().any(|key| key.starts_with("bar@2.0.0(patch_hash=")));
    assert!(snapshot_keys(&updated).contains(&"foo@https://example.test/foo.tgz".to_string()));
}

#[test]
fn rejects_rekeying_a_package_another_snapshot_reaches_as_a_peer() {
    let dir = workspace(&["foo@1.1.0"]);

    assert!(
        try_fast_update_patched_dependencies(
            &lockfile(PEER_LOCKFILE),
            &config(dir.path(), &["foo@1.1.0"], true),
        )
        .is_none(),
        "the dependent's peer suffix embeds foo's depPath, so it would rekey too",
    );
}

#[test]
fn rejects_rekeying_when_a_peer_suffix_is_hashed() {
    let dir = workspace(&["foo@1.1.0"]);

    assert!(
        try_fast_update_patched_dependencies(
            &lockfile(HASHED_PEER_LOCKFILE),
            &config(dir.path(), &["foo@1.1.0"], true),
        )
        .is_none(),
        "a shortened peer suffix cannot be checked for the patched package",
    );
}

#[test]
fn rejects_an_unused_patch_when_unused_patches_are_not_allowed() {
    let dir = workspace(&["bar@2.0.0"]);

    assert!(
        try_fast_update_patched_dependencies(
            &lockfile(LOCKFILE),
            &config(dir.path(), &["bar@2.0.0"], false),
        )
        .is_none(),
        "the resolver has to run so it can raise ERR_PNPM_UNUSED_PATCH",
    );
}

#[test]
fn recognizes_a_git_patch_while_absorbing_unrelated_settings_drift() {
    let dir = workspace(&["foo@1.0.0"]);
    let config = Config {
        exclude_links_from_lockfile: true,
        allow_unused_patches: false,
        ..config(dir.path(), &["foo@1.0.0"], false)
    };
    let patch_hashes = config
        .patched_dependency_hashes()
        .expect("hash the patch files")
        .expect("a configured patch");
    let hash = &patch_hashes["foo@1.0.0"];
    let mut subject = lockfile(&PATCHED_GIT_LOCKFILE.replace("PATCH_HASH", hash));
    subject.patched_dependencies = Some(patch_hashes.clone());
    subject.settings =
        Some(crate::fast_update_settings::lockfile_settings_from_config(&Config::default()));

    let updated = try_fast_update_patched_dependencies(&subject, &config)
        .expect("the git patch remains applied while the settings update is absorbed");

    assert!(updated.settings.expect("settings").exclude_links_from_lockfile);
}

#[test]
fn removes_an_unused_patch_without_allowing_unused_patches() {
    let dir = workspace(&[]);
    let mut lockfile = lockfile(LOCKFILE);
    lockfile.patched_dependencies =
        Some(BTreeMap::from([("bar@2.0.0".to_string(), "deadbeef".to_string())]));

    let updated = try_fast_update_patched_dependencies(&lockfile, &config(dir.path(), &[], false))
        .expect("dropping a key that matched nothing leaves no unused patch behind");

    assert!(updated.patched_dependencies.is_none());
}

#[test]
fn rekeys_a_locked_package_whose_patch_file_was_edited() {
    let dir = workspace(&["foo@1.1.0"]);
    let mut lockfile = lockfile(PATCHED_LOCKFILE);
    lockfile.patched_dependencies =
        Some(BTreeMap::from([("foo@1.1.0".to_string(), "deadbeef".to_string())]));

    let updated =
        try_fast_update_patched_dependencies(&lockfile, &config(dir.path(), &["foo@1.1.0"], true))
            .expect("a new hash renames the snapshot");

    assert!(!snapshot_keys(&updated).contains(&"foo@1.1.0(patch_hash=deadbeef)".to_string()));
    assert!(snapshot_keys(&updated)[0].contains("(patch_hash="));
}

#[test]
fn rejects_an_unchanged_configuration() {
    let dir = workspace(&["bar@2.0.0"]);
    let config = config(dir.path(), &["bar@2.0.0"], true);
    let mut lockfile = lockfile(LOCKFILE);
    lockfile.patched_dependencies =
        config.patched_dependency_hashes().expect("hash the patch files");

    assert!(
        try_fast_update_patched_dependencies(&lockfile, &config).is_none(),
        "with nothing to absorb this handler must not claim the install",
    );
}

#[test]
fn rejects_a_missing_patch_file() {
    let dir = workspace(&[]);

    assert!(
        try_fast_update_patched_dependencies(
            &lockfile(LOCKFILE),
            &config(dir.path(), &["bar@2.0.0"], true),
        )
        .is_none(),
        "the resolver reports the unreadable patch file instead",
    );
}
