use super::super::{
    cas_paths_key,
    cold::{ColdCapture, add_cold_cas_paths},
    warm::warm_cas_paths_by_pkg_id,
};
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use pretty_assertions::assert_eq;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

#[test]
fn warm_cas_paths_share_prefetch_maps_and_deduplicate_peer_variants() {
    let plain: PackageKey = "a@1.0.0".parse().unwrap();
    let peered: PackageKey = "a@1.0.0(peer@1.0.0)".parse().unwrap();
    let patched: PackageKey = "a@1.0.0(patch_hash=abc)".parse().unwrap();
    let snapshot = SnapshotEntry::default();
    let paths = Arc::new(HashMap::from([("index.js".to_string(), PathBuf::from("cas/plain"))]));
    let patched_paths =
        Arc::new(HashMap::from([("index.js".to_string(), PathBuf::from("cas/patched"))]));
    let warm = [
        (&plain, &snapshot, &paths, "plain", false),
        (&peered, &snapshot, &paths, "plain", false),
        (&patched, &snapshot, &patched_paths, "patched", true),
    ];

    let map = warm_cas_paths_by_pkg_id(&warm);

    dbg!(&map);
    assert_eq!(map.len(), 2);
    assert!(Arc::ptr_eq(&map[&cas_paths_key(&plain)], &paths));
    assert!(Arc::ptr_eq(&map[&cas_paths_key(&patched)], &patched_paths));
}

#[test]
fn cold_cas_paths_preserve_warm_entries_and_add_missing_packages() {
    let warm_key: PackageKey = "a@1.0.0".parse().unwrap();
    let duplicate: PackageKey = "a@1.0.0(peer@1.0.0)".parse().unwrap();
    let cold_key: PackageKey = "b@1.0.0".parse().unwrap();
    let snapshot = SnapshotEntry::default();
    let warm_paths = Arc::new(HashMap::from([("index.js".to_string(), PathBuf::from("cas/warm"))]));
    let mut map = warm_cas_paths_by_pkg_id(&[(&warm_key, &snapshot, &warm_paths, "warm", false)]);
    let cold = [&duplicate, &cold_key]
        .into_iter()
        .map(|snapshot_key| ColdCapture {
            snapshot_key,
            snapshot: &snapshot,
            cas_paths: HashMap::from([("index.js".to_string(), PathBuf::from("cas/cold"))]),
            requires_build: false,
            source_is_mutable: false,
            force_import: false,
        })
        .collect();

    add_cold_cas_paths(&mut map, cold);

    dbg!(&map);
    assert_eq!(map.len(), 2);
    assert!(Arc::ptr_eq(&map[&cas_paths_key(&warm_key)], &warm_paths));
    assert_eq!(map[&cas_paths_key(&cold_key)]["index.js"].to_str(), Some("cas/cold"));
}
