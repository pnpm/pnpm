use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{AuthState, Config, router_with_auth};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt as _;

struct Fixture {
    _directory: TempDir,
    app: axum::Router,
    operator: String,
    member: String,
    member_id: String,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let auth = AuthState::in_memory();
        let (_, operator_id) = auth.users.add_or_login("operator", "password").await.unwrap();
        let (_, member_id) = auth.users.add_or_login("member", "password").await.unwrap();
        let operator = auth.tokens.issue(&operator_id).await.unwrap();
        let member = auth.tokens.issue(&member_id).await.unwrap();
        let yaml = format!(
            "auth:\n  admins: [{operator_id}, {member_id}]\nregistries:\n  local:\n    type: hosted\n    teams:\n      release-id:\n        name: release\n        members: [{member_id}]\n    packages:\n      '@secret/*':\n        access: [{member_id}]\n      '@team/*':\n        access: team:release-id\n      '**':\n        access: $all\n        publish: $owners\n        unpublish: $owners\n",
        );
        let path = directory.path().join("config.yaml");
        std::fs::write(&path, format!("{yaml}defaultRegistry: local\n")).unwrap();
        let config = Config::from_yaml(&path, "127.0.0.1:0".parse().unwrap(), None).unwrap();
        let app = router_with_auth(config, auth);
        Self { _directory: directory, app, operator, member, member_id }
    }

    async fn replace_member(&self) -> (String, String) {
        let (status, _) = send(
            &self.app,
            "DELETE",
            &format!("/-/pnpr/v0/admin/users/{}", self.member_id),
            &self.operator,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, created) = send(
            &self.app,
            "PUT",
            "/-/user/member",
            "",
            Some(json!({"name": "member", "password": "new-password"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let id = created["id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_ne!(id, self.member_id);
        (
            id,
            created["token"]
                .as_str()
                .unwrap()
                .to_string(),
        )
    }
}

async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if !token.is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = body.map_or_else(Body::empty, |body| Body::from(body.to_string()));
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn reusing_a_username_inherits_no_admin_user_or_team_grants() {
    let fixture = Fixture::new().await;
    assert_eq!(
        send(&fixture.app, "GET", "/-/pnpr/v0/admin/users", &fixture.member, None)
            .await
            .0,
        StatusCode::OK,
    );
    assert_eq!(
        send(&fixture.app, "GET", "/npm/@secret/missing", &fixture.member, None)
            .await
            .0,
        StatusCode::NOT_FOUND,
    );
    assert_eq!(
        send(&fixture.app, "GET", "/npm/@team/missing", &fixture.member, None)
            .await
            .0,
        StatusCode::NOT_FOUND,
    );
    let (_, replacement) = fixture.replace_member().await;
    assert_eq!(
        send(&fixture.app, "GET", "/-/whoami", &fixture.member, None).await.0,
        StatusCode::UNAUTHORIZED,
    );
    assert_eq!(
        send(&fixture.app, "GET", "/-/whoami", &replacement, None).await.1["username"],
        "member",
    );
    for path in ["/-/pnpr/v0/admin/users", "/npm/@secret/missing", "/npm/@team/missing"] {
        assert_eq!(
            send(&fixture.app, "GET", path, &replacement, None).await.0,
            StatusCode::FORBIDDEN,
        );
    }
}

#[tokio::test]
async fn reusing_a_username_inherits_no_package_ownership() {
    let fixture = Fixture::new().await;
    let document = crate::npm::publish_doc("owned-package", "1.0.0", b"first");
    assert_eq!(
        send(&fixture.app, "PUT", "/npm/owned-package", &fixture.member, Some(document))
            .await
            .0,
        StatusCode::CREATED,
    );
    let (_, replacement) = fixture.replace_member().await;
    let document = crate::npm::publish_doc("owned-package", "2.0.0", b"second");
    assert_eq!(
        send(&fixture.app, "PUT", "/npm/owned-package", &replacement, Some(document))
            .await
            .0,
        StatusCode::FORBIDDEN,
    );
    let (_, stored) = send(&fixture.app, "GET", "/npm/owned-package", "", None).await;
    assert_eq!(stored["maintainers"][0]["userId"], fixture.member_id);
}

#[tokio::test]
async fn renaming_an_account_preserves_tokens_and_grants() {
    let fixture = Fixture::new().await;
    let (status, renamed) = send(
        &fixture.app,
        "PATCH",
        &format!("/-/pnpr/v0/admin/users/{}", fixture.member_id),
        &fixture.operator,
        Some(json!({"name": "renamed-member"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["id"], fixture.member_id);
    assert_eq!(
        send(&fixture.app, "GET", "/-/whoami", &fixture.member, None).await.1["username"],
        "renamed-member",
    );
    assert_eq!(
        send(&fixture.app, "GET", "/-/pnpr/v0/admin/users", &fixture.member, None)
            .await
            .0,
        StatusCode::OK,
    );
    assert_eq!(
        send(&fixture.app, "GET", "/npm/@secret/missing", &fixture.member, None)
            .await
            .0,
        StatusCode::NOT_FOUND,
    );
    assert_eq!(
        send(&fixture.app, "GET", "/npm/@team/missing", &fixture.member, None)
            .await
            .0,
        StatusCode::NOT_FOUND,
    );
    let (status, new_account) = send(
        &fixture.app,
        "PUT",
        "/-/user/member",
        "",
        Some(json!({"name": "member", "password": "new-password"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(new_account["id"], fixture.member_id);
    assert_eq!(
        send(
            &fixture.app,
            "GET",
            "/-/pnpr/v0/admin/users",
            new_account["token"].as_str().unwrap(),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN,
    );
}

#[tokio::test]
async fn a_maintainer_update_cannot_forge_an_account_id() {
    let fixture = Fixture::new().await;
    let document = crate::npm::publish_doc("owned-package", "1.0.0", b"first");
    assert_eq!(
        send(&fixture.app, "PUT", "/npm/owned-package", &fixture.member, Some(document))
            .await
            .0,
        StatusCode::CREATED,
    );
    let body = json!({"maintainers": [{"name": "operator", "userId": fixture.member_id}]});
    assert_eq!(
        send(&fixture.app, "PUT", "/npm/owned-package/-rev/1-fixture", &fixture.member, Some(body))
            .await
            .0,
        StatusCode::BAD_REQUEST,
    );
    let (_, stored) = send(&fixture.app, "GET", "/npm/owned-package", "", None).await;
    assert_eq!(stored["maintainers"][0]["userId"], fixture.member_id);
}

#[tokio::test]
async fn an_admin_can_explicitly_assign_a_reused_name_to_its_new_account() {
    let fixture = Fixture::new().await;
    let document = crate::npm::publish_doc("owned-package", "1.0.0", b"first");
    assert_eq!(
        send(&fixture.app, "PUT", "/npm/owned-package", &fixture.member, Some(document))
            .await
            .0,
        StatusCode::CREATED,
    );
    let (replacement_id, replacement) = fixture.replace_member().await;
    let owner_update = "/npm/owned-package/-rev/1-fixture";
    let echoed = json!({"maintainers": [{"name": "member", "userId": fixture.member_id}]});
    assert_eq!(
        send(&fixture.app, "PUT", owner_update, &fixture.operator, Some(echoed))
            .await
            .0,
        StatusCode::CREATED,
    );
    let next = crate::npm::publish_doc("owned-package", "1.1.0", b"next");
    assert_eq!(
        send(&fixture.app, "PUT", "/npm/owned-package", &replacement, Some(next.clone()))
            .await
            .0,
        StatusCode::FORBIDDEN,
    );
    let assigned = json!({"maintainers": [{"name": "member", "userId": replacement_id}]});
    assert_eq!(
        send(&fixture.app, "PUT", owner_update, &fixture.operator, Some(assigned))
            .await
            .0,
        StatusCode::CREATED,
    );
    let (_, stored) = send(&fixture.app, "GET", "/npm/owned-package", "", None).await;
    assert_eq!(stored["maintainers"][0]["userId"], replacement_id);
    assert_eq!(
        send(&fixture.app, "PUT", "/npm/owned-package", &replacement, Some(next))
            .await
            .0,
        StatusCode::CREATED,
    );
}
