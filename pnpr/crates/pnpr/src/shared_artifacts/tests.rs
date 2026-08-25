use pnpm_shared_artifact_protocol::{
    ArtifactVariant, MAX_RESOLVE_RESPONSE_SIZE, ResolveArtifactsResponse, ResolvedArtifact,
    SignedArtifactEnvelope,
};

use super::ResolveBudget;

#[test]
fn resolve_budget_bounds_scanned_and_serialized_bytes() {
    let empty_response_size =
        serde_json::to_vec(&ResolveArtifactsResponse { artifacts: Vec::new() }).unwrap().len();
    let mut scan_budget = ResolveBudget { scanned_bytes: 0, response_bytes: empty_response_size };
    scan_budget.add_scan(MAX_RESOLVE_RESPONSE_SIZE as u64).unwrap();
    assert!(scan_budget.add_scan(1).is_err());

    let artifact = ResolvedArtifact {
        key: "dependency-side-effects:v1:deps=abc".to_string(),
        variants: vec![ArtifactVariant {
            envelope: SignedArtifactEnvelope {
                algorithm: "ecdsa-p256-sha256".to_string(),
                key_id: "key".to_string(),
                payload: "e30=".to_string(),
                signature: "eA==".to_string(),
            },
        }],
    };
    let mut response_budget =
        ResolveBudget { scanned_bytes: 0, response_bytes: MAX_RESOLVE_RESPONSE_SIZE };
    assert!(response_budget.add_response(&artifact, false).is_err());
}
