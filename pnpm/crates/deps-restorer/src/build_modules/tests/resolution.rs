use crate::VirtualStoreLayout;
use pnpm_config::Config;
use pnpm_lockfile::PackageKey;
use pretty_assertions::assert_eq;
use std::{collections::HashMap, path::PathBuf};
use tempfile::tempdir;

/// With an override map (the hoisted linker), the helper returns
/// the override entry verbatim — no virtual-store layout lookup.
#[test]
fn pkg_root_for_key_hoisted_uses_override() {
    let dir = tempdir().unwrap();
    let mut config = Config::new();
    config.store_dir = dir.path().join("store").into();
    config.modules_dir = dir.path().join("node_modules");
    config.install_state_dir = dir.path().join("node_modules/.pacquet");
    let config = config.leak();
    let layout = VirtualStoreLayout::new(config, None, None, None, None, None);

    let key: PackageKey = "is-positive@1.0.0".parse().expect("parse key");
    let hoisted_dir = PathBuf::from("/repo/node_modules/is-positive");
    let map: HashMap<PackageKey, Vec<PathBuf>> = [(key.clone(), vec![hoisted_dir.clone()])].into();

    let result = super::super::PkgRoots { layout: &layout, by_key: Some(&map) }
        .canonical(&key)
        .expect("override hits");
    assert_eq!(result, hoisted_dir);
}
