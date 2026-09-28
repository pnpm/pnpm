use super::{
    HashMap, HashSet, assert_eq, importer_locked_peer_versions, peer_context_lockfile,
    peer_declaring_metadata, plain_dependency, snapshot_with_dependencies,
};
use std::str::FromStr;

/// `consumer` declares `peer`, whose own suffix names `nested`. It
/// resolves the undeclared `transitive` through `child`, and `deep`
/// through `wrapper`, which only passes it on to `leaf`.
fn consumer_lockfile(consumer_version: &str) -> pnpm_lockfile::Lockfile {
    let mut consumer = snapshot_with_dependencies([
        ("peer", plain_dependency("1.0.0(nested@3.0.0)")),
        ("child", plain_dependency("1.0.0(transitive@2.0.0)")),
        ("wrapper", plain_dependency("1.0.0(deep@4.0.0)")),
    ]);
    consumer.transitive_peer_dependencies =
        Some(vec!["deep".to_string(), "transitive".to_string()]);
    let mut wrapper = snapshot_with_dependencies([("leaf", plain_dependency("1.0.0(deep@4.0.0)"))]);
    wrapper.transitive_peer_dependencies = Some(vec!["deep".to_string()]);
    let mut lockfile = peer_context_lockfile(
        Some(("consumer@1.0.0", peer_declaring_metadata(["peer"]))),
        [
            (&format!("consumer@{consumer_version}"), consumer),
            (
                "peer@1.0.0(nested@3.0.0)",
                snapshot_with_dependencies([("nested", plain_dependency("3.0.0"))]),
            ),
            (
                "child@1.0.0(transitive@2.0.0)",
                snapshot_with_dependencies([("transitive", plain_dependency("2.0.0"))]),
            ),
            ("wrapper@1.0.0(deep@4.0.0)", wrapper),
            (
                "leaf@1.0.0(deep@4.0.0)",
                snapshot_with_dependencies([("deep", plain_dependency("4.0.0"))]),
            ),
        ],
    );
    let packages = lockfile.packages.get_or_insert_default();
    for (key, peer) in
        [("peer@1.0.0", "nested"), ("child@1.0.0", "transitive"), ("leaf@1.0.0", "deep")]
    {
        packages.insert(
            pnpm_lockfile::PkgNameVerPeer::from_str(key).unwrap(),
            peer_declaring_metadata([peer]),
        );
    }
    lockfile.importers.insert(
        "app".to_string(),
        serde_json::from_value(serde_json::json!({
            "dependencies": {
                "consumer": { "specifier": "1.0.0", "version": consumer_version },
            },
        }))
        .unwrap(),
    );
    lockfile
}

/// `dedupe` can rewrite a key between its spelled-out and hashed forms,
/// so both must pin the same peers or the next run resolves differently
/// (pnpm/pnpm#16331).
#[test]
fn hashed_and_explicit_suffixes_pin_the_same_peers() {
    let explicit =
        consumer_lockfile("1.0.0(deep@4.0.0)(peer@1.0.0(nested@3.0.0))(transitive@2.0.0)");
    let hashed = consumer_lockfile("1.0.0(0123456789abcdef0123456789abcdef)");

    let expected = HashMap::from_iter(
        [("peer", "1.0.0"), ("nested", "3.0.0"), ("transitive", "2.0.0"), ("deep", "4.0.0")].map(
            |(name, version)| (name.to_string(), HashSet::from_iter([version.to_string()])),
        ),
    );
    assert_eq!(importer_locked_peer_versions(Some(&explicit), "app"), expected);
    assert_eq!(importer_locked_peer_versions(Some(&hashed), "app"), expected);
}
