//! Integration tests verifying that administrative, user, rule, team, SCIM,
//! and unpublish operations emit structured audit records under `pnpr::audit`.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{AuthState, Config, router_with_auth};
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

use crate::{ecosystem::sha256_hex, npm::publish_doc};

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
    span_fields: HashMap<String, String>,
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
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: tracing_subscriber::layer::Context<'_, Subscriber>,
    ) {
        if let Some(span) = ctx.span(id) {
            let mut fields = HashMap::new();
            let mut visitor = EventVisitor(&mut fields);
            attrs.record(&mut visitor);
            span.extensions_mut().insert(fields);
        }
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        ctx: tracing_subscriber::layer::Context<'_, Subscriber>,
    ) {
        if let Some(span) = ctx.span(id) {
            let mut extensions = span.extensions_mut();
            if let Some(fields) = extensions.get_mut::<HashMap<String, String>>() {
                values.record(&mut EventVisitor(fields));
            }
        }
    }

    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        ctx: tracing_subscriber::layer::Context<'_, Subscriber>,
    ) {
        if event.metadata().target() == "pnpr::audit" {
            let mut fields = HashMap::new();
            let mut visitor = EventVisitor(&mut fields);
            event.record(&mut visitor);
            let current = ctx.lookup_current();
            let span_name = current
                .as_ref()
                .map(|span| span.name().to_string());
            let span_fields = current
                .as_ref()
                .and_then(|span| {
                    span.extensions()
                        .get::<HashMap<String, String>>()
                        .cloned()
                })
                .unwrap_or_default();
            if let Some(action) = fields.get("action").cloned() {
                self.0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(AuditRecord { action, span_name, fields, span_fields });
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

async fn app_router(config: Config) -> axum::Router {
    let auth = AuthState::in_memory_with_max_users(config.identity.auth.htpasswd.max_users);
    auth.users.create_user("root", "secret").await.unwrap();
    router_with_auth(config, auth)
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

fn assert_request_span(record: &AuditRecord, expected_method: &str) {
    assert_eq!(record.span_name.as_deref(), Some("request"));
    assert!(record.span_fields.contains_key("request_id"));
    assert_eq!(record.span_fields.get("method").map(String::as_str), Some(expected_method));
    assert!(record.span_fields.contains_key("uri"));
    // The span records the caller. A SCIM call authenticates with the SCIM
    // token rather than as a user, and a registration's actor is the new
    // account, not the anonymous caller.
    let caller = match (record.action.as_str(), record.fields.get("actor").map(String::as_str)) {
        (_, Some("scim")) => return,
        ("user.register", _) => Some("-"),
        (_, actor) => actor,
    };
    assert_eq!(record.span_fields.get("user").map(String::as_str), caller, "{record:?}");
}

async fn send_bytes(
    app: &axum::Router,
    method: &str,
    path: &str,
    auth: Option<&str>,
    content_type: Option<&str>,
    bytes: Vec<u8>,
) -> StatusCode {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(auth) = auth {
        request = request.header("authorization", auth);
    }
    if let Some(content_type) = content_type {
        request = request.header("content-type", content_type);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(bytes)).unwrap())
        .await
        .unwrap();
    response.status()
}

#[tokio::test]
async fn audit_log_user_admin_and_registration() {
    let logs = init_audit_subscriber();
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path())).await;
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
    assert_request_span(&ev, "PUT");

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
    assert_request_span(&ev, "PUT");

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
    assert_request_span(&ev, "DELETE");

    // 4. user.delete
    let (status, _) =
        send(&app, "DELETE", "/-/pnpr/v0/admin/users/aud_u1", Some(&root), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let ev = find_event(&logs, "user.delete", "user", "aud_u1");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_request_span(&ev, "DELETE");

    // 5. user.register
    let _reg_token = login(&app, "aud_reg_user", "regpass").await;
    let ev = find_event(&logs, "user.register", "user", "aud_reg_user");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("aud_reg_user"));
    assert_request_span(&ev, "PUT");
}

#[tokio::test]
async fn audit_log_rule_admin() {
    let logs = init_audit_subscriber();
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path())).await;
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
    assert_request_span(&ev, "PUT");

    // 2. rules.delete
    let (status, _) =
        send(&app, "DELETE", "/-/pnpr/v0/admin/rules/npm/local", Some(&root), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let ev = find_event(&logs, "rules.delete", "registry", "local");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_request_span(&ev, "DELETE");
}

#[tokio::test]
async fn audit_log_team_admin() {
    let logs = init_audit_subscriber();
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path())).await;
    let root = login(&app, "root", "secret").await;

    // 1. team.create
    let (status, _) = send(
        &app,
        "PUT",
        "/npm/-/org/audscope/team",
        Some(&root),
        Some(json!({ "name": "auddevs" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "team.create", "team", "auddevs");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.fields.get("scope").map(String::as_str), Some("audscope"));
    assert_request_span(&ev, "PUT");

    // 2. team.member_add
    let (status, _) = send(
        &app,
        "PUT",
        "/npm/-/team/audscope/auddevs/user",
        Some(&root),
        Some(json!({ "user": "audmember" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "team.member_add", "user", "audmember");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.fields.get("team").map(String::as_str), Some("auddevs"));
    assert_request_span(&ev, "PUT");

    // 3. team.access_grant
    let (status, _) = send(
        &app,
        "PUT",
        "/npm/-/team/audscope/auddevs/package",
        Some(&root),
        Some(json!({ "package": "@audscope/pkg", "permissions": "read-write" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "team.access_grant", "package", "@audscope/pkg");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_eq!(ev.fields.get("permissions").map(String::as_str), Some("read-write"));
    assert_request_span(&ev, "PUT");

    // 4. team.access_revoke
    let (status, _) = send(
        &app,
        "DELETE",
        "/npm/-/team/audscope/auddevs/package",
        Some(&root),
        Some(json!({ "package": "@audscope/pkg" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let ev = find_event(&logs, "team.access_revoke", "package", "@audscope/pkg");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_request_span(&ev, "DELETE");

    // 5. team.member_remove
    let (status, _) = send(
        &app,
        "DELETE",
        "/npm/-/team/audscope/auddevs/user",
        Some(&root),
        Some(json!({ "user": "audmember" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let ev = find_event(&logs, "team.member_remove", "user", "audmember");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_request_span(&ev, "DELETE");

    // 6. team.destroy
    let (status, _) = send(&app, "DELETE", "/npm/-/team/audscope/auddevs", Some(&root), None).await;
    assert_eq!(status, StatusCode::OK);
    let ev = find_event(&logs, "team.destroy", "team", "auddevs");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("root"));
    assert_request_span(&ev, "DELETE");
}

#[tokio::test]
async fn audit_log_scim_operations() {
    let logs = init_audit_subscriber();
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path())).await;

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
    assert_request_span(&ev, "POST");

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
    assert_request_span(&ev, "PUT");

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
    assert_request_span(&ev, "PATCH");

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
    assert_request_span(&ev, "DELETE");
}

#[tokio::test]
async fn audit_log_unpublish_and_dist_tags() {
    let logs = init_audit_subscriber();
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path())).await;
    let publisher = login(&app, "aud_publisher", "pass").await;

    // Publish package
    let body = publish_doc("audit-unpub-pkg", "1.0.0", b"tarball-contents");
    let (status, _) = send(&app, "PUT", "/npm/audit-unpub-pkg", Some(&publisher), Some(body)).await;
    assert_eq!(status, StatusCode::CREATED);

    // 1. package.dist_tag_set
    let (status, _) = send(
        &app,
        "PUT",
        "/npm/-/package/audit-unpub-pkg/dist-tags/beta",
        Some(&publisher),
        Some(json!("1.0.0")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "package.dist_tag_set", "package", "audit-unpub-pkg");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("aud_publisher"));
    assert_eq!(ev.fields.get("tag").map(String::as_str), Some("beta"));
    assert_eq!(ev.fields.get("version").map(String::as_str), Some("1.0.0"));
    assert_eq!(ev.fields.get("registry").map(String::as_str), Some("local"));
    assert_request_span(&ev, "PUT");

    // Invalid dist-tag name with control character is rejected with BAD_REQUEST
    let (status, _) = send(
        &app,
        "PUT",
        "/npm/-/package/audit-unpub-pkg/dist-tags/bad%0Atag",
        Some(&publisher),
        Some(json!("1.0.0")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Nonexistent dist-tag removal does NOT emit package.dist_tag_remove
    let (status, _) = send(
        &app,
        "DELETE",
        "/npm/-/package/audit-unpub-pkg/dist-tags/nonexistent",
        Some(&publisher),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(
        logs.lock()
            .unwrap()
            .iter()
            .all(|record| {
                !(record.action == "package.dist_tag_remove"
                    && record.fields.get("tag").map(String::as_str) == Some("nonexistent"))
            }),
        "nonexistent dist-tag removal must not emit audit event",
    );

    // 2. package.dist_tag_remove
    let (status, _) = send(
        &app,
        "DELETE",
        "/npm/-/package/audit-unpub-pkg/dist-tags/beta",
        Some(&publisher),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "package.dist_tag_remove", "package", "audit-unpub-pkg");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("aud_publisher"));
    assert_eq!(ev.fields.get("tag").map(String::as_str), Some("beta"));
    assert_eq!(ev.fields.get("registry").map(String::as_str), Some("local"));
    assert_request_span(&ev, "DELETE");

    // 3. package.unpublish_tarball
    let (status, _) = send(
        &app,
        "DELETE",
        "/npm/audit-unpub-pkg/-/audit-unpub-pkg-1.0.0.tgz/-rev/anything",
        Some(&publisher),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "package.unpublish_tarball", "package", "audit-unpub-pkg");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("aud_publisher"));
    assert_eq!(ev.fields.get("tarball").map(String::as_str), Some("audit-unpub-pkg-1.0.0.tgz"));
    assert_eq!(ev.fields.get("registry").map(String::as_str), Some("local"));
    assert_request_span(&ev, "DELETE");

    // 4. package.unpublish
    let (status, _) =
        send(&app, "DELETE", "/npm/audit-unpub-pkg/-rev/anything", Some(&publisher), None).await;
    assert_eq!(status, StatusCode::CREATED);
    let ev = find_event(&logs, "package.unpublish", "package", "audit-unpub-pkg");
    assert_eq!(ev.fields.get("actor").map(String::as_str), Some("aud_publisher"));
    assert_eq!(ev.fields.get("registry").map(String::as_str), Some("local"));
    assert_request_span(&ev, "DELETE");

    // Duplicate package unpublish does not emit duplicate audit record
    let (status, _) =
        send(&app, "DELETE", "/npm/audit-unpub-pkg/-rev/anything", Some(&publisher), None).await;
    assert_eq!(status, StatusCode::CREATED);
    let count = logs
        .lock()
        .unwrap()
        .iter()
        .filter(|record| {
            record.action == "package.unpublish"
                && record.fields.get("package").map(String::as_str) == Some("audit-unpub-pkg")
        })
        .count();
    assert_eq!(count, 1, "duplicate package unpublish must not emit duplicate audit record");
}

fn oci_app(tmp: &TempDir) -> axum::Router {
    let mut config = crate::ecosystem::mixed_router_config(
        tmp.path().to_path_buf(),
        pnpr::Ecosystem::Oci,
        crate::ecosystem::HostedSource {
            name: "images",
            org: "images",
            access: "$all",
            packages: &["acme/*"],
        },
        ("dockerhub", "http://images.invalid/"),
    );
    let hosted = config.routing.hosted.get_mut("images").expect("hosted images registry");
    hosted.rules = std::mem::take(&mut hosted.rules)
        .with_default_unpublish(pnpr::AccessList::from_tokens(["$authenticated"]));
    pnpr::router_with_auth(config, pnpr::AuthState::in_memory())
}

#[tokio::test]
async fn audit_log_oci_deletion_operations() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let logs = init_audit_subscriber();
    let dir = TempDir::new().unwrap();
    let app = oci_app(&dir);
    let user_token = login(&app, "alice", "secret").await;
    let auth = format!("Basic {}", STANDARD.encode(format!("alice:{user_token}")));

    // 1. Push a blob
    let blob_bytes = b"oci-layer-bytes";
    let blob_digest = format!("sha256:{}", sha256_hex(blob_bytes));
    let status = send_bytes(
        &app,
        "POST",
        &format!("/v2/acme/app/blobs/uploads/?digest={blob_digest}"),
        Some(&auth),
        None,
        blob_bytes.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // 2. Push a manifest
    let manifest = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": { "digest": blob_digest, "size": blob_bytes.len() },
        "layers": [],
    });
    let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
    let status = send_bytes(
        &app,
        "PUT",
        "/v2/acme/app/manifests/1.0",
        Some(&auth),
        Some("application/vnd.oci.image.manifest.v1+json"),
        manifest_bytes,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // 3. Delete manifest
    let status =
        send_bytes(&app, "DELETE", "/v2/acme/app/manifests/1.0", Some(&auth), None, Vec::new())
            .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let manifest_event = find_event(&logs, "oci.delete_manifest", "reference", "1.0");
    assert_eq!(manifest_event.fields.get("repository").map(String::as_str), Some("acme/app"));
    assert_eq!(manifest_event.fields.get("actor").map(String::as_str), Some("alice"));
    assert_request_span(&manifest_event, "DELETE");

    // 4. Try deleting a nonexistent blob
    let nonexistent_digest =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000";
    let status = send_bytes(
        &app,
        "DELETE",
        &format!("/v2/acme/app/blobs/{nonexistent_digest}"),
        Some(&auth),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        logs.lock()
            .unwrap()
            .iter()
            .all(|record| {
                !(record.action == "oci.delete_blob"
                    && record.fields.get("digest").map(String::as_str) == Some(nonexistent_digest))
            }),
        "nonexistent blob deletion must not emit audit event",
    );

    // 5. Delete an unreferenced blob
    let unreferenced_bytes = b"unreferenced-blob-bytes";
    let unreferenced_digest = format!("sha256:{}", sha256_hex(unreferenced_bytes));
    let status = send_bytes(
        &app,
        "POST",
        &format!("/v2/acme/app/blobs/uploads/?digest={unreferenced_digest}"),
        Some(&auth),
        None,
        unreferenced_bytes.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let status = send_bytes(
        &app,
        "DELETE",
        &format!("/v2/acme/app/blobs/{unreferenced_digest}"),
        Some(&auth),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let blob_event = find_event(&logs, "oci.delete_blob", "digest", &unreferenced_digest);
    assert_eq!(blob_event.fields.get("repository").map(String::as_str), Some("acme/app"));
    assert_eq!(blob_event.fields.get("actor").map(String::as_str), Some("alice"));
    assert_request_span(&blob_event, "DELETE");
}
