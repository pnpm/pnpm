use super::{
    directory::{Directory, ScimUser, newly_deprovisioned},
    resources::apply_patch,
};
use serde_json::json;
use std::collections::HashMap;

fn user(active: bool) -> ScimUser {
    ScimUser { active, ..ScimUser::default() }
}

#[test]
fn every_deactivation_is_counted_and_survives_reactivation() {
    let mut directory = Directory::default();
    directory.store("alice", user(true));
    directory.store("alice", user(false));
    directory.store("alice", user(false));
    assert_eq!(directory.users["alice"].deprovisions, 1);
    directory.store("alice", user(true));
    assert_eq!(directory.users["alice"].deprovisions, 1);
    directory.store("alice", user(false));
    assert_eq!(directory.users["alice"].deprovisions, 2);
}

#[test]
fn a_replica_ends_sessions_of_users_deprovisioned_since_its_last_read() {
    let mut directory = Directory::default();
    directory.store("alice", user(true));
    directory.store("bob", user(true));
    let mut seen = HashMap::new();
    assert!(newly_deprovisioned(&mut seen, &directory).is_empty());

    // Deactivated and reactivated between two reads of this replica.
    directory.store("alice", user(false));
    directory.store("alice", user(true));
    assert_eq!(newly_deprovisioned(&mut seen, &directory), ["alice"]);
    assert!(newly_deprovisioned(&mut seen, &directory).is_empty());
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
