use super::{
    directory::{ScimTracker, ScimUser, record_status_deltas},
    resources::apply_patch,
};
use pnpr_storage::database::ScimStatus;
use serde_json::json;

fn user(active: bool) -> ScimUser {
    ScimUser { active, ..ScimUser::default() }
}

fn status(user_id: &str, active: bool, deprovisions: u64, generation: u64) -> ScimStatus {
    ScimStatus { external_id: None, user_id: user_id.to_string(), active, deprovisions, generation }
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

#[test]
fn subject_bindings_reject_ambiguity_and_follow_updates_and_removal() {
    let mut subjects = super::subjects::SubjectDirectory::default();
    let mut first = status("account-one", true, 0, 1);
    first.external_id = Some("subject-one".to_string());
    assert!(!subjects.apply(&first));
    assert_eq!(subjects.resolve("subject-one").unwrap(), "account-one");
    assert!(subjects.resolve("unknown").is_err());
    let mut second = status("account-two", false, 0, 2);
    second.external_id = first.external_id.clone();
    subjects.apply(&second);
    assert!(subjects.resolve("subject-one").is_err());
    second.external_id = Some("subject-two".to_string());
    assert!(subjects.apply(&second));
    assert_eq!(subjects.resolve("subject-one").unwrap(), "account-one");
    assert_eq!(subjects.resolve("subject-two").unwrap(), "account-two");
    first.external_id = None;
    assert!(subjects.apply(&first));
    assert!(subjects.resolve("subject-one").is_err());
}

#[test]
fn external_id_is_case_insensitive_but_its_value_is_opaque() {
    let (_, mut user) = super::resources::parse_user(&json!({
        "userName": "example-user", "ExternalID": " Subject-One ",
    }))
    .unwrap();
    assert_eq!(user.attributes["externalId"], " Subject-One ");
    apply_patch(
        &mut user,
        &json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
            "Operations": [{"op": "replace", "path": "EXTERNALID", "value": "subject-two"}],
        }),
    )
    .unwrap();
    assert_eq!(user.attributes["externalId"], "subject-two");
    for value in [json!(null), json!(1), json!([]), json!("")] {
        assert!(
            super::resources::parse_user(&json!({
                "userName": "example-user", "externalId": value,
            }))
            .is_err(),
        );
    }
}

#[tokio::test]
async fn provisioned_subjects_follow_scim_lifecycle_and_reload() {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use pnpr_auth::AuthState;
    use pnpr_config::{Config, ScimConfig};
    use pnpr_fixtures::MockConfigPresets;
    use tower::ServiceExt;

    let temp = tempfile::TempDir::new().unwrap();
    let mut config =
        Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().to_path_buf());
    let token = "scim-secret-0123456789abcdef0123456789";
    config.identity.auth.scim = Some(ScimConfig { token: token.into() });
    let (state, app) =
        crate::server::state_and_router(config, AuthState::in_memory(), None).unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/-/pnpr/v0/scim/v2/Users")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(
                    json!({"userName": "example-user", "externalId": "subject-one"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    let user_id = body["id"].as_str().unwrap();
    assert_eq!(super::resolve_subject(&state, "subject-one").unwrap().as_ref(), user_id);
    let binding = super::resolve_subject(&state, "subject-one").unwrap();
    let subjects = &state.inner.identity.managed.scim.as_ref().unwrap().subjects;
    assert!(subjects.try_write().is_err());
    drop(binding);
    assert!(subjects.try_write().is_ok());
    let directory = super::ScimState::default();
    assert!(super::reload_directory(&state, &directory).await);
    assert_eq!(
        directory.subjects
            .read()
            .unwrap()
            .resolve("subject-one")
            .unwrap(),
        user_id,
    );
    for active in [false, true] {
        let response = app.clone().oneshot(Request::builder()
            .method("PUT").uri(format!("/-/pnpr/v0/scim/v2/Users/{user_id}"))
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(json!({"userName": "example-user", "externalId": "subject-one", "active": active}).to_string())).unwrap()
        ).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(super::resolve_subject(&state, "subject-one").is_ok(), active);
    }
    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/-/pnpr/v0/scim/v2/Users/{user_id}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(super::resolve_subject(&state, "subject-one").is_err());
}

#[test]
fn scim_subject_binding_requires_a_single_provider_and_a_directory() {
    use pnpr_config::Config;
    let provider = "
  oidc:
    - name: company
      issuer: https://identity.example
      audience: pnpr
      login: {scim: true}
";
    let config = |auth: &str| {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("pnpr.yaml");
        std::fs::write(&path, format!("storage: ./s\nauth:\n{auth}")).unwrap();
        Config::from_yaml(
            &path,
            "127.0.0.1:0".parse().unwrap(),
            Some("https://registry.example".to_string()),
        )
        .unwrap()
    };
    assert!(super::validate_oidc(&config(provider)).is_err());
    let with_directory =
        format!("  scim:\n    token: scim-secret-0123456789abcdef0123456789\n{provider}");
    assert!(super::validate_oidc(&config(&with_directory)).is_ok());
    let mut multiple = config(&with_directory);
    let mut second = multiple.identity.auth.oidc[0].clone();
    second.name = "another".to_string();
    multiple.identity.auth.oidc.push(second);
    assert!(super::validate_oidc(&multiple).is_err());
}

#[tokio::test]
async fn schema_advertises_case_sensitive_external_ids() {
    use axum::body::to_bytes;
    let response = super::discovery::schemas(super::ScimClient).await;
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    let attribute = body[0]["attributes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attribute| attribute["name"] == "externalId")
        .unwrap();
    assert_eq!(attribute["type"], "string");
    assert_eq!(attribute["required"], false);
    assert_eq!(attribute["caseExact"], true);
}

#[test]
fn pathless_patches_reject_ambiguous_external_id_aliases_before_writing() {
    for alias in ["ExternalID", "urn:ietf:params:scim:schemas:core:2.0:User:externalId"] {
        let (_, mut user) = super::resources::parse_user(&json!({
            "userName": "example-user", "externalId": "original-subject",
        }))
        .unwrap();
        let values = json!({ "externalId": "subject-one", alias: "subject-two" });
        let patch = json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:PatchOp"],
            "Operations": [{ "op": "replace", "value": values }],
        });
        assert!(apply_patch(&mut user, &patch).is_err());
        assert_eq!(user.attributes["externalId"], "original-subject");
        let resource = json!({ "userName": "example-user", "externalId": "subject-one", alias: "subject-two" });
        assert!(super::resources::parse_user(&resource).is_err());
    }
}
