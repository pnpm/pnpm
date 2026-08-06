use super::try_fast_update_patched_dependencies;
use indexmap::IndexMap;
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
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

fn lockfile(source: &str) -> Lockfile {
    serde_saphyr::from_str(source).expect("parse lockfile")
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
fn rejects_a_patch_that_matches_a_locked_package() {
    let dir = workspace(&["foo@1.1.0"]);

    assert!(
        try_fast_update_patched_dependencies(
            &lockfile(LOCKFILE),
            &config(dir.path(), &["foo@1.1.0"], true),
        )
        .is_none(),
        "patching a locked package rekeys its snapshot, so it needs the resolver",
    );
}

#[test]
fn rejects_a_bare_name_patch_that_matches_a_locked_package() {
    let dir = workspace(&["foo"]);

    assert!(
        try_fast_update_patched_dependencies(
            &lockfile(LOCKFILE),
            &config(dir.path(), &["foo"], true),
        )
        .is_none(),
        "a bare-name key matches every version of the package",
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
fn rejects_removing_a_patch_that_was_applied_to_a_locked_package() {
    let dir = workspace(&[]);
    let mut lockfile = lockfile(PATCHED_LOCKFILE);
    lockfile.patched_dependencies =
        Some(BTreeMap::from([("foo@1.1.0".to_string(), "deadbeef".to_string())]));

    assert!(
        try_fast_update_patched_dependencies(&lockfile, &config(dir.path(), &[], true)).is_none(),
        "the locked snapshot still carries the patch hash, so dropping the patch rekeys it",
    );
}

#[test]
fn rejects_an_edited_patch_file_for_a_locked_package() {
    let dir = workspace(&["foo@1.1.0"]);
    let mut lockfile = lockfile(LOCKFILE);
    lockfile.patched_dependencies =
        Some(BTreeMap::from([("foo@1.1.0".to_string(), "stale-hash".to_string())]));

    assert!(
        try_fast_update_patched_dependencies(&lockfile, &config(dir.path(), &["foo@1.1.0"], true),)
            .is_none(),
        "a new hash changes the (patch_hash=...) suffix of a locked package",
    );
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
