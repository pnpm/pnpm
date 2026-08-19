use super::{load, memo_path, persist};
use pnpm_lockfile::Lockfile;
use std::fs;

/// A minimal but well-formed wanted lockfile, as `pnpm-lock.yaml`
/// carries it.
const LOCKFILE_YAML: &str = "lockfileVersion: '9.0'

settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false

importers:

  .:
    dependencies:
      is-odd:
        specifier: ^3.0.1
        version: 3.0.1

packages:

  is-odd@3.0.1:
    resolution: {integrity: sha512-CQpnWPrDwmP1+SMHXZhtLtJv90yiyVfluGsX5iNCVkrhQtU3TQHsUWPG9wkdk9Lgd5yNpAg9jQEo90CBaXgWMA==}

snapshots:

  is-odd@3.0.1: {}
";

#[test]
fn memo_path_is_keyed_by_workspace_root() {
    let cache = std::path::Path::new("/cache");
    let first = memo_path(cache, std::path::Path::new("/proj/a"));
    let second = memo_path(cache, std::path::Path::new("/proj/b"));
    assert_ne!(first, second, "two projects must not share a memo");
    assert!(first.starts_with("/cache/lockfile-memo/v1"), "got {first:?}");
    assert_eq!(
        first,
        memo_path(cache, std::path::Path::new("/proj/a")),
        "the key must be stable",
    );
}

#[test]
fn persist_then_load_roundtrips_the_wanted_lockfile() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join(Lockfile::FILE_NAME), LOCKFILE_YAML).unwrap();

    persist(&cache, &root);
    let memo = load(&cache, &root).expect("memo should load after persist");
    assert!(memo.packages.is_some_and(|packages| packages.len() == 1));
}

#[test]
fn persist_without_a_lockfile_is_a_quiet_no_op() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    // Nothing to copy: must not panic, must not fabricate a memo.
    persist(&cache, &root);
    assert!(load(&cache, &root).is_none());
}

#[test]
fn an_unparsable_memo_reads_as_no_memo() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let root = temp.path().join("project");
    let path = memo_path(&cache, &root);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "lockfileVersion: '9.0'\nimporters: [not, a, map]\n").unwrap();

    assert!(load(&cache, &root).is_none(), "garbage must fall back to a fresh resolve");
}
