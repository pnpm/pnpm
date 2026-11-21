use pnpm_lockfile::{
    LockfileResolution,
    PkgNameVerPeer,
    RegistryResolution,
};
use pnpm_lockfile_verification::LocalResolutionProbeParent;
use serde_json::json;

use super::describes_locked_artifact;

const LOCKED: &str = "sha512-deadbeef";

fn parent() -> LocalResolutionProbeParent {
    let resolution: RegistryResolution =
        serde_json::from_value(json!({ "integrity": LOCKED })).expect("parse registry resolution");
    LocalResolutionProbeParent {
        pkg_key: "remote@1.0.0".parse::<PkgNameVerPeer>().expect("parse key"),
        resolution: Some(LockfileResolution::Registry(resolution)),
        name: "remote".to_string(),
        version: Some("1.0.0".to_string()),
        dep_aliases: Vec::new(),
        optional_aliases: Vec::new(),
        peer_aliases: Vec::new(),
    }
}

#[test]
fn registry_fallback_counts_only_for_the_locked_integrity() {
    let manifest = |integrity: &str| json!({ "dist": { "integrity": integrity } });
    assert!(describes_locked_artifact(&parent(), &manifest(LOCKED)));
    assert!(!describes_locked_artifact(&parent(), &manifest("sha512-other")));
    assert!(!describes_locked_artifact(&parent(), &json!({})));
}
