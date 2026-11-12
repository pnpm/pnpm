use super::{CandidatePeer, missing_peers, peers_accept_versions};
use serde_json::json;

#[test]
fn candidate_fallbacks_follow_manifest_peer_classification() {
    let manifest = json!({
        "name": "candidate",
        "dependencies": {"required": "1", "optional": "1", "metadata": "1"},
        "optionalDependencies": {"own": "1"},
        "peerDependencies": {
            "candidate": "*", "required": "^1", "optional": "^1",
            "own": "^1", "external": "^1"
        },
        "peerDependenciesMeta": {
            "optional": {"optional": true}, "metadata": {"optional": true}
        }
    });
    for auto_install_peers in [false, true] {
        let peers = CandidatePeer::from_manifest(&manifest, auto_install_peers);
        let classified: Vec<_> = peers
            .iter()
            .map(|peer| {
                (peer.name.as_str(), peer.range.as_str(), peer.optional, peer.has_fallback_child)
            })
            .collect();
        assert_eq!(
            classified,
            [
                ("external", "^1", false, false),
                ("metadata", "*", true, true),
                ("optional", "^1", true, true),
                ("required", "^1", false, !auto_install_peers),
            ],
        );
        let missing = missing_peers(&peers, &|_| None);
        let missing_names: Vec<_> = missing
            .iter()
            .map(|peer| peer.name.as_str())
            .collect();
        let expected =
            if auto_install_peers { vec!["external", "required"] } else { vec!["external"] };
        assert_eq!(missing_names, expected);
        let incompatible = "2.0.0".to_string();
        assert!(!peers_accept_versions(&peers, &|name| {
            (name == "optional").then_some(&incompatible)
        }));
    }
}
