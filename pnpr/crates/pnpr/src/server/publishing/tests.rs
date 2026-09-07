use super::record_publisher;
use pnpr_policy::Identity;
use serde_json::json;

#[test]
fn publisher_attribution_comes_only_from_an_authenticated_identity() {
    let mut authenticated = json!({
        "versions": { "1.0.0": { "_npmUser": { "name": "mallory" } } },
    });
    record_publisher(&mut authenticated, &Identity::user("alice"));
    assert_eq!(authenticated["versions"]["1.0.0"]["_npmUser"]["name"], "alice");

    let mut anonymous = json!({
        "versions": { "1.0.0": { "_npmUser": { "name": "mallory" } } },
    });
    record_publisher(&mut anonymous, &Identity::Anonymous);
    assert!(anonymous["versions"]["1.0.0"].get("_npmUser").is_none());
}
