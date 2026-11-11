use super::{
    directory::{ScimTracker, ScimUser, record_status_deltas},
    resources::apply_patch,
};
use pnpr_storage::metadata::ScimStatus;
use serde_json::json;

fn user(active: bool) -> ScimUser {
    ScimUser { active, ..ScimUser::default() }
}

fn status(user_id: &str, active: bool, deprovisions: u64, generation: u64) -> ScimStatus {
    ScimStatus { user_id: user_id.to_string(), active, deprovisions, generation }
}

#[test]
fn a_replica_ends_sessions_of_users_deprovisioned_since_its_last_read() {
    let mut tracker = ScimTracker::default();
    let initial = [status("alice", true, 0, 1), status("bob", true, 0, 2)];
    assert!(record_status_deltas(&mut tracker, &initial).is_empty());
    assert_eq!(tracker.since_generation, 2);

    // Deactivated and reactivated between two reads of this replica.
    let delta = [status("alice", true, 1, 4)];
    assert_eq!(record_status_deltas(&mut tracker, &delta), ["alice"]);
    assert_eq!(tracker.since_generation, 4);
    assert!(record_status_deltas(&mut tracker, &[]).is_empty());
}

#[test]
fn extension_attributes_nest_under_their_schema() {
    let mut patched = user(true);
    let patch = json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
        "Operations": [
            {
                "op": "add",
                "path": "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:department",
                "value": "R&D",
            },
            {
                "op": "add",
                "path": "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:manager.value",
                "value": "boss",
            },
            { "op": "replace", "value": { "name.givenName": "Alice" } },
        ],
    });
    apply_patch(&mut patched, &patch).unwrap();
    let enterprise =
        &patched.attributes["urn:ietf:params:scim:schemas:extension:enterprise:2.0:User"];
    assert_eq!(enterprise, &json!({ "department": "R&D", "manager": { "value": "boss" } }));
    assert_eq!(patched.attributes["name"], json!({ "givenName": "Alice" }));
}

#[test]
fn a_patch_path_deeper_than_scim_allows_is_refused() {
    let mut patched = user(true);
    let deep = vec!["a"; 10_000].join(".");
    let patch = json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
        "Operations": [{ "op": "add", "path": deep, "value": 1 }],
    });
    assert!(apply_patch(&mut patched, &patch).is_err());
    assert!(patched.attributes.is_empty());
}
