use super::{
    HashMap, HashSet, SnapshotEntry, assert_eq, importer_locked_peer_versions,
    peer_context_lockfile, peer_declaring_metadata, plain_dependency, snapshot_with_dependencies,
    snapshot_with_dependency,
};

#[test]
fn truncated_provider_keys_retain_nested_pins_without_overriding_encoded_pins() {
    for max_length in [1000, 20] {
        let suffix = pnpm_lockfile::PeerBindings::from_suffix("(provider@1.0.0)(peer@1.0.0)")
            .unwrap()
            .to_suffix(max_length)
            .unwrap();
        let version = format!("1.0.0{suffix}");
        let key = format!("consumer@{version}");
        let mut lockfile = peer_context_lockfile(
            Some(("consumer@1.0.0", peer_declaring_metadata(["provider", "peer"]))),
            [
                (
                    key.as_str(),
                    snapshot_with_dependencies([
                        (
                            "provider",
                            plain_dependency("1.0.0(peer@2.0.0(deep@5.0.0))(nested@3.0.0)"),
                        ),
                        ("peer", plain_dependency("1.0.0")),
                    ]),
                ),
                (
                    "provider@1.0.0(peer@2.0.0(deep@5.0.0))(nested@3.0.0)",
                    snapshot_with_dependencies([
                        ("peer", plain_dependency("2.0.0(deep@5.0.0)")),
                        ("nested", plain_dependency("3.0.0(provider@1.0.0)")),
                    ]),
                ),
                (
                    "nested@3.0.0(provider@1.0.0)",
                    snapshot_with_dependency(
                        "provider",
                        plain_dependency("1.0.0(peer@2.0.0(deep@5.0.0))(nested@3.0.0)"),
                    ),
                ),
            ],
        );
        lockfile.importers.insert(
            "app".to_string(),
            serde_json::from_value(serde_json::json!({
                "dependencies": {
                    "consumer": {
                        "specifier": "1.0.0",
                        "version": version,
                    },
                },
            }))
            .unwrap(),
        );
        assert_eq!(
            importer_locked_peer_versions(Some(&lockfile), "app", |_| true),
            HashMap::from_iter([
                ("provider".to_string(), HashSet::from_iter(["1.0.0".to_string()])),
                ("peer".to_string(), HashSet::from_iter(["1.0.0".to_string()])),
                ("nested".to_string(), HashSet::from_iter(["3.0.0".to_string()])),
                ("deep".to_string(), HashSet::from_iter(["5.0.0".to_string()])),
            ]),
        );
    }
}

#[test]
fn sibling_providers_keep_distinct_pins_at_the_same_depth() {
    let mut lockfile = peer_context_lockfile(
        None,
        [
            (
                "consumer@1.0.0(left@1.0.0(peer@1.0.0))(right@1.0.0(peer@2.0.0))",
                SnapshotEntry::default(),
            ),
            (
                "left@1.0.0(peer@1.0.0)",
                snapshot_with_dependency("peer", plain_dependency("1.0.0(nested@3.0.0)")),
            ),
            (
                "right@1.0.0(peer@2.0.0)",
                snapshot_with_dependency("peer", plain_dependency("2.0.0(nested@4.0.0)")),
            ),
        ],
    );
    lockfile.importers.insert("app".into(), serde_json::from_value(serde_json::json!({
        "dependencies": {"consumer": {"specifier":"1.0.0", "version":"1.0.0(left@1.0.0(peer@1.0.0))(right@1.0.0(peer@2.0.0))"}}
    })).unwrap());
    let versions = importer_locked_peer_versions(Some(&lockfile), "app", |_| true);
    assert_eq!(versions["peer"], HashSet::from_iter(["1.0.0".into(), "2.0.0".into()]));
    assert_eq!(versions["nested"], HashSet::from_iter(["3.0.0".into(), "4.0.0".into()]));
}

#[test]
fn independent_explicit_provider_paths_keep_pins_at_different_depths() {
    let mut lockfile = peer_context_lockfile(None, []);
    lockfile.importers.insert("app".into(), serde_json::from_value(serde_json::json!({
        "dependencies": {"consumer": {"specifier":"1.0.0", "version":"1.0.0(left@1.0.0(peer@1.0.0))(right@1.0.0(bridge@1.0.0(peer@2.0.0)))"}}
    })).unwrap());
    let versions = importer_locked_peer_versions(Some(&lockfile), "app", |_| true);
    assert_eq!(versions["peer"], HashSet::from_iter(["1.0.0".into(), "2.0.0".into()]));
}

#[test]
fn matching_encoded_provider_recovers_deeper_context() {
    for nested in ["1.0.0", "3.0.0"] {
        let version = format!("1.0.0(peer@2.0.0)(nested@{nested})");
        let key = format!("consumer@{version}");
        let mut lockfile = peer_context_lockfile(
            None,
            [
                (
                    key.as_str(),
                    snapshot_with_dependency("peer", plain_dependency("2.0.0(nested@1.0.0)")),
                ),
                (
                    "peer@2.0.0(nested@1.0.0)",
                    snapshot_with_dependency("nested", plain_dependency("1.0.0(deep@4.0.0)")),
                ),
            ],
        );
        lockfile.importers.insert(
            "app".into(),
            serde_json::from_value(serde_json::json!({
                "dependencies": {"consumer": {"specifier":"1.0.0", "version":version}}
            }))
            .unwrap(),
        );
        let versions = importer_locked_peer_versions(Some(&lockfile), "app", |_| true);
        assert_eq!(versions["nested"], HashSet::from_iter([nested.to_string()]));
        assert_eq!(
            versions.get("deep"),
            (nested == "1.0.0")
                .then(|| HashSet::from_iter(["4.0.0".to_string()]))
                .as_ref(),
        );
    }
}
