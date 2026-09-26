use crate::VirtualStoreLayout;
use pnpm_config::Config;
use pnpm_lockfile::PackageKey;
use pretty_assertions::assert_eq;
use std::{collections::HashMap, path::PathBuf};
use tempfile::tempdir;

// --- bin_dirs_in_all_parent_dirs ----------------------------------------

/// Top-level hoisted package: the helper must produce one entry per
/// ancestor `node_modules/.bin` from `pkg_root` up to (and including)
/// `lockfile_dir`. A `pkgRoot` of `<lockfile>/node_modules/foo`
/// produces three paths — the package's own `node_modules/.bin`, the
/// synthetic `<modules>/node_modules/.bin` from the dirname loop step,
/// and the final `<lockfile>/node_modules/.bin` push after the loop
/// exits. Synthetic entries don't exist on disk and are harmless to
/// PATH lookup; pinning the literal list pins the exact output shape.
#[test]
fn bin_dirs_top_level_hoisted_pkg() {
    let lockfile_dir = PathBuf::from("/repo");
    let pkg_root = PathBuf::from("/repo/node_modules/foo");
    let dirs = super::super::bin_dirs_in_all_parent_dirs(&pkg_root, &lockfile_dir);
    assert_eq!(
        dirs,
        vec![
            PathBuf::from("/repo/node_modules/foo/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/.bin"),
        ],
    );
}
/// Conflict-nested package: a package living under
/// `<lockfile>/node_modules/parent/node_modules/child` produces five
/// entries — its own bin, parent's, the synthetic `<modules>/node_modules/.bin`,
/// the synthetic intermediate (`<parent>/node_modules/node_modules/.bin`),
/// and the final lockfile-root push. The two synthetic intermediates
/// fall out of the dirname-loop's per-step push but never resolve to
/// real directories.
#[test]
fn bin_dirs_nested_hoisted_pkg() {
    let lockfile_dir = PathBuf::from("/repo");
    let pkg_root = PathBuf::from("/repo/node_modules/parent/node_modules/child");
    let dirs = super::super::bin_dirs_in_all_parent_dirs(&pkg_root, &lockfile_dir);
    assert_eq!(
        dirs,
        vec![
            PathBuf::from("/repo/node_modules/parent/node_modules/child/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/parent/node_modules/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/parent/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/.bin"),
        ],
    );
}
/// A snapshot key absent from the override map (the walker decided
/// not to record it — pre-skipped, optional skip) returns `None` so
/// `BuildModules`'s per-snapshot loop can short-circuit instead of
/// attempting to `cd` into a path the caller never produced.
#[test]
fn pkg_root_for_key_hoisted_missing_returns_none() {
    let dir = tempdir().unwrap();
    let mut config = Config::new();
    config.store_dir = dir.path().join("store").into();
    config.modules_dir = dir.path().join("node_modules");
    config.virtual_store_dir = dir.path().join("node_modules/.pacquet");
    let config = config.leak();
    let layout = VirtualStoreLayout::new(config, None, None, None, None, None);

    let key: PackageKey = "absent@1.0.0".parse().expect("parse key");
    let map: HashMap<PackageKey, Vec<PathBuf>> = HashMap::new();

    let result = super::super::PkgRoots { layout: &layout, by_key: Some(&map) }.canonical(&key);
    assert!(result.is_none(), "absent key surfaces None, got {result:?}");
}
