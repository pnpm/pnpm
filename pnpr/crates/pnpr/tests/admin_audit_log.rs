//! Integration tests verifying that administrative, user, rule, team, SCIM,
//! and unpublish operations emit structured audit records under `pnpr::audit`.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{Config, router};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};
use tempfile::TempDir;
use tower::ServiceExt;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::npm::publish_doc;

const SCIM_TOKEN: &str = "scim-secret-0123456789abcdef0123456789";

const CONFIG: &str = "\
auth:
  admins: [root]
  scim:
    token: scim-secret-0123456789abcdef0123456789
  htpasswd:
    max_users: 100
registries:
  local:
    type: hosted
    org: \"\"
    rulesManagedBy: api
    teamsManagedBy: api
    teams:
      readers: []
    packages:
      '@secret/*':
        access: $authenticated
      '@audscope/pkg':
        access: $authenticated
      '**':
        access: $authenticated
        publish: $authenticated
        unpublish: $authenticated
  main:
    type: router
    sources: [local]
defaultRegistry: main
";

#[derive(Debug, Clone)]
struct AuditRecord {
    action: String,
    span_name: Option<String>,
    fields: HashMap<String, String>,
}

static AUDIT_LOGS: OnceLock<Arc<Mutex<Vec<AuditRecord>>>> = OnceLock::new();

fn init_audit_subscriber() -> Arc<Mutex<Vec<AuditRecord>>> {
    Arc::clone(AUDIT_LOGS.get_or_init(|| {
        let logs = Arc::new(Mutex::new(Vec::new()));
        let layer = AuditCaptureLayer(Arc::clone(&logs));
        let subscriber = tracing_subscriber::registry().with(layer);
        let _ = subscriber.try_init();
        logs
    }))
}

struct AuditCaptureLayer(Arc<Mutex<Vec<AuditRecord>>>);

impl<Subscriber> tracing_subscriber::Layer<Subscriber> for AuditCaptureLayer
where
    Subscriber:
        tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        ctx: tracing_subscriber::layer::Context<'_, Subscriber>,
    ) {
        if event.metadata().target() == "pnpr::audit" {
            let mut fields = HashMap::new();
            let mut visitor = EventVisitor(&mut fields);
            event.record(&mut visitor);
            let span_name = ctx
                .lookup_current()
                .map(|span| span.name().to_string());
            if let Some(action) = fields.get("action").cloned() {
                self.0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(AuditRecord { action, span_name, fields });
            }
        }
    }
}

struct EventVisitor<'a>(&'a mut HashMap<String, String>);

impl tracing::field::Visit for EventVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let formatted = format!("{value:?}");
        let trimmed =
            if formatted.starts_with('"') && formatted.ends_with('"') && formatted.len() >= 2 {
                &formatted[1..formatted.len() - 1]
            } else {
                &formatted
            };
        self.0
            .entry(field.name().to_string())
            .or_insert_with(|| trimmed.to_string());
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_string(), value.to_string());
    }
}

fn load_config(dir: &Path) -> Config {
    let storage = dir.join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let path = dir.join("config.yaml");
    std::fs::write(&path, format!("storage: {}\n{CONFIG}", storage.display())).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    Config::from_yaml(&path, listen, Some("http://example.test".to_string())).unwrap()
}

async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = body.map(|body| Body::from(body.to_string())).unwrap_or_else(Body::empty);
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn login(app: &axum::Router, username: &str, password: &str) -> String {
    let body = json!({ "name": username, "password": password, "type": "user", "roles": [] });
    let path = format!("/-/user/org.couchdb.user:{username}");
    let (status, payload) = send(app, "PUT", &path, None, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "login must succeed");
    payload["token"]
        .as_str()
        .expect("token in response")
        .to_string()
}

fn find_event(
    logs: &Arc<Mutex<Vec<AuditRecord>>>,
    action: &str,
    key: &str,
    val: &str,
) -> AuditRecord {
    let guard = logs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    guard
        .iter()
        .rev()
        .find(|record| {
            record.action == action
                && record.fields
                    .get(key)
                    .map(|found| found == val)
                    .unwrap_or(false)
        })
        .cloned()
        .unwrap_or_else(|| panic!("audit event {action} with {key}={val} not found in {guard:?}"))
}

#[tokio::test]
async fn audit_log_user_admin_and_registration() {
    let logs = init_audit_subscriber();
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let root = login(&app, "root", "secret").await;

    // 1. user.create
    let (status, _) = send(
        &app,
        "PUT",
        "/-/pnpr/v0/admin/users/aud_u1",
        Some(&root),
        Some(json!({ "password": "pass1" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "user.create", "user", "aud_u1");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 2. user.password_update
    let (status, _) = send(
        &app,
        "PUT",
        "/-/pnpr/v0/admin/users/aud_u1",
        Some(&root),
        Some(json!({ "password": "pass2" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ev = find_event(&logs, "user.password_update", "user", "aud_u1");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // Log in to issue a token for aud_u1
    let _ = login(&app, "aud_u1", "pass2").await;
    let (status, tokens) =
        send(&app, "GET", "/-/pnpr/v0/admin/users/aud_u1/tokens", Some(&root), None).await;
    assert_eq!(status, StatusCode::OK);
    let token_key = tokens["objects"][0]["key"].as_str().unwrap();

    // 3. user.token_revoke
    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/-/pnpr/v0/admin/users/aud_u1/tokens/{token_key}"),
        Some(&root),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let ev = find_event(&logs, "user.token_revoke", "user", "aud_u1");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.fields.get("token_key").map(String::as_str), Some(token_key));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 4. user.delete
    let (status, _) =
        send(&app, "DELETE", "/-/pnpr/v0/admin/users/aud_u1", Some(&root), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let ev = find_event(&logs, "user.delete", "user", "aud_u1");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 5. user.register
    let _reg_token = login(&app, "aud_reg_user", "regpass").await;
    let ev = find_event(&logs, "user.register", "user", "aud_reg_user");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("aud_reg_user"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));
}

#[tokio::test]
async fn audit_log_rule_admin() {
    let logs = init_audit_subscriber();
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let root = login(&app, "root", "secret").await;

    // 1. rules.replace
    let (status, _) = send(
        &app,
        "PUT",
        "/-/pnpr/v0/admin/rules/npm/local",
        Some(&root),
        Some(json!({
            "packages": {
                "@secret/*": { "access": ["$authenticated"] }
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ev = find_event(&logs, "rules.replace", "registry", "local");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 2. rules.delete
    let (status, _) =
        send(&app, "DELETE", "/-/pnpr/v0/admin/rules/npm/local", Some(&root), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let ev = find_event(&logs, "rules.delete", "registry", "local");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));
}

#[tokio::test]
async fn audit_log_team_admin() {
    let logs = init_audit_subscriber();
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let root = login(&app, "root", "secret").await;

    // 1. team.create
    let (status, _) =
        send(&app, "PUT", "/-/org/audscope/team", Some(&root), Some(json!({ "name": "auddevs" })))
            .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "team.create", "team", "auddevs");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.fields.get("scope").map(String::as_str), Some("audscope"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 2. team.member_add
    let (status, _) = send(
        &app,
        "PUT",
        "/-/team/audscope/auddevs/user",
        Some(&root),
        Some(json!({ "user": "audmember" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "team.member_add", "user", "audmember");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.fields.get("team").map(String::as_str), Some("auddevs"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 3. team.access_grant
    let (status, _) = send(
        &app,
        "PUT",
        "/-/team/audscope/auddevs/package",
        Some(&root),
        Some(json!({ "package": "@audscope/pkg", "permissions": "read-write" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "team.access_grant", "package", "@audscope/pkg");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.fields.get("permissions").map(String::as_str), Some("read-write"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 4. team.access_revoke
    let (status, _) = send(
        &app,
        "DELETE",
        "/-/team/audscope/auddevs/package",
        Some(&root),
        Some(json!({ "package": "@audscope/pkg" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let ev = find_event(&logs, "team.access_revoke", "package", "@audscope/pkg");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 5. team.member_remove
    let (status, _) = send(
        &app,
        "DELETE",
        "/-/team/audscope/auddevs/user",
        Some(&root),
        Some(json!({ "user": "audmember" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let ev = find_event(&logs, "team.member_remove", "user", "audmember");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 6. team.destroy
    let (status, _) = send(&app, "DELETE", "/-/team/audscope/auddevs", Some(&root), None).await;
    assert_eq!(status, StatusCode::OK);
    let ev = find_event(&logs, "team.destroy", "team", "auddevs");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));
}

#[tokio::test]
async fn audit_log_scim_operations() {
    let logs = init_audit_subscriber();
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));

    // 1. scim.user_create
    let (status, _) = send(
        &app,
        "POST",
        "/-/pnpr/v0/scim/v2/Users",
        Some(SCIM_TOKEN),
        Some(json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:User"],
            "userName": "aud_scim_user@example.com",
            "active": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "scim.user_create", "user", "aud_scim_user@example.com");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("scim"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 2. scim.user_replace
    let (status, _) = send(
        &app,
        "PUT",
        "/-/pnpr/v0/scim/v2/Users/aud_scim_user@example.com",
        Some(SCIM_TOKEN),
        Some(json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:User"],
            "userName": "aud_scim_user@example.com",
            "active": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ev = find_event(&logs, "scim.user_replace", "user", "aud_scim_user@example.com");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("scim"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 3. scim.user_patch
    let (status, _) = send(
        &app,
        "PATCH",
        "/-/pnpr/v0/scim/v2/Users/aud_scim_user@example.com",
        Some(SCIM_TOKEN),
        Some(json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
            "Operations": [{
                "op": "replace",
                "path": "active",
                "value": "False"
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ev = find_event(&logs, "scim.user_patch", "user", "aud_scim_user@example.com");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("scim"));
    assert_eq!(ev.fields.get("active").map(String::as_str), Some("false"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 4. scim.user_delete
    let (status, _) = send(
        &app,
        "DELETE",
        "/-/pnpr/v0/scim/v2/Users/aud_scim_user@example.com",
        Some(SCIM_TOKEN),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let ev = find_event(&logs, "scim.user_delete", "user", "aud_scim_user@example.com");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("scim"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));
}

#[tokio::test]
async fn audit_log_unpublish_and_dist_tags() {
    let logs = init_audit_subscriber();
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let publisher = login(&app, "aud_publisher", "pass").await;

    // Publish package
    let body = publish_doc("audit-unpub-pkg", "1.0.0", b"tarball-contents");
    let (status, _) = send(&app, "PUT", "/audit-unpub-pkg", Some(&publisher), Some(body)).await;
    assert_eq!(status, StatusCode::CREATED);

    // 1. package.dist_tag_set
    let (status, _) = send(
        &app,
        "PUT",
        "/-/package/audit-unpub-pkg/dist-tags/beta",
        Some(&publisher),
        Some(json!("1.0.0")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "package.dist_tag_set", "package", "audit-unpub-pkg");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("aud_publisher"));
    assert_eq!(ev.fields.get("tag").map(String::as_str), Some("beta"));
    assert_eq!(ev.fields.get("version").map(String::as_str), Some("1.0.0"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 2. package.dist_tag_remove
    let (status, _) =
        send(&app, "DELETE", "/-/package/audit-unpub-pkg/dist-tags/beta", Some(&publisher), None)
            .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "package.dist_tag_remove", "package", "audit-unpub-pkg");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("aud_publisher"));
    assert_eq!(ev.fields.get("tag").map(String::as_str), Some("beta"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 3. package.unpublish_tarball
    let (status, _) = send(
        &app,
        "DELETE",
        "/audit-unpub-pkg/-/audit-unpub-pkg-1.0.0.tgz/-rev/anything",
        Some(&publisher),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "package.unpublish_tarball", "package", "audit-unpub-pkg");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("aud_publisher"));
    assert_eq!(ev.fields.get("tarball").map(String::as_str), Some("audit-unpub-pkg-1.0.0.tgz"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));

    // 4. package.unpublish
    let (status, _) =
        send(&app, "DELETE", "/audit-unpub-pkg/-rev/anything", Some(&publisher), None).await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "package.unpublish", "package", "audit-unpub-pkg");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("aud_publisher"));
    assert_eq!(ev.span_name.as_deref(), Some("request"));
}
