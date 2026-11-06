//! Publishing to a registry whose metadata lives in `SQLite`: a retried
//! publish gets its recorded outcome, a batch commits as one publication, and
//! publications are served under `/-/publications/`.

use crate::npm::publish_doc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use pnpr::{Config, MaxUsers, router};
use pnpr_config::MetadataBackend;
use pnpr_fixtures::MockConfigPresets;
use pnpr_policy::{AccessList, PackageRules, RuleOverride};
use pnpr_registry::{PackagePattern, Registries, Registry};
use pnpr_storage::Ecosystem;
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
};
use tempfile::TempDir;
use tower::ServiceExt;

fn sqlite_config(storage: &Path) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, storage.to_path_buf());
    config.http.public_url = "http://example.test".to_string();
    config.identity.auth.htpasswd.max_users = MaxUsers::Unlimited;
    config.storage.metadata = MetadataBackend::Sqlite(storage.join("metadata.sqlite"));
    config
}

struct Answer {
    status: StatusCode,
    generation: Option<String>,
    body: Value,
    text: String,
}

async fn send(app: &Router, method: &str, path: &str, token: &str, body: Option<&Value>) -> Answer {
    send_with_key(app, method, path, token, body, None).await
}

async fn send_with_key(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Value>,
    key: Option<&str>,
) -> Answer {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    if let Some(key) = key {
        request = request.header("idempotency-key", key);
    }
    let body = body.map_or_else(Body::empty, |body| Body::from(serde_json::to_vec(body).unwrap()));
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let generation = response
        .headers()
        .get("pnpr-generation")
        .map(|value| value.to_str().unwrap().to_string());
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Answer { status, generation, body, text }
}

async fn token(app: &Router) -> String {
    token_of(app, "alice").await
}

async fn token_of(app: &Router, user: &str) -> String {
    let body = json!({ "name": user, "password": "secret", "type": "user", "roles": [] });
    let path = format!("/-/user/org.couchdb.user:{user}");
    let answer = send(app, "PUT", &path, "-", Some(&body)).await;
    assert_eq!(answer.status, StatusCode::CREATED);
    answer.body["token"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn a_retried_publish_gets_its_recorded_outcome() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let doc = publish_doc("retried", "1.0.0", b"bytes");
    let first = send(&app, "PUT", "/npm/retried", &token, Some(&doc)).await;
    assert_eq!(first.status, StatusCode::CREATED);
    assert_eq!(first.generation.as_deref(), Some(":1"));
    assert_eq!(first.body["publication"], 1);

    // The publisher never saw that response and sends the same request again.
    let retry = send(&app, "PUT", "/npm/retried", &token, Some(&doc)).await;
    assert_eq!(retry.status, StatusCode::CREATED);
    assert_eq!(retry.generation, first.generation);
    assert_eq!(retry.body["publication"], 1);
    let second = send(&app, "GET", "/-/publications/-/2", &token, None).await;
    assert_eq!(second.status, StatusCode::NOT_FOUND, "a replay commits no new publication");

    let other_archive = publish_doc("retried", "1.0.0", b"other bytes");
    let conflict = send(&app, "PUT", "/npm/retried", &token, Some(&other_archive)).await;
    assert_eq!(conflict.status, StatusCode::CONFLICT);

    // The whole request is the key: the same archive under another tag is a
    // new request, not a replay of the committed one.
    let mut other_tag = doc.clone();
    other_tag["dist-tags"] = json!({ "beta": "1.0.0" });
    let retagged = send(&app, "PUT", "/npm/retried", &token, Some(&other_tag)).await;
    assert_eq!(retagged.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn an_identical_publish_after_unpublish_is_refused() {
    for whole_package in [false, true] {
        let tmp = TempDir::new().unwrap();
        let app = router(sqlite_config(tmp.path()));
        let token = token(&app).await;
        let mut doc = publish_doc("retry-removed", "1.1.0-beta.0", b"bytes");
        doc["dist-tags"] = json!({ "beta": "1.1.0-beta.0" });
        assert_eq!(
            send(&app, "PUT", "/npm/retry-removed", &token, Some(&doc)).await.status,
            StatusCode::CREATED,
        );
        let other = publish_doc("retry-removed", "1.0.0", b"other");
        assert_eq!(
            send(&app, "PUT", "/npm/retry-removed", &token, Some(&other)).await.status,
            StatusCode::CREATED,
        );
        let path = if whole_package {
            "/npm/retry-removed/-rev/anything"
        } else {
            "/npm/retry-removed/-/retry-removed-1.1.0-beta.0.tgz/-rev/anything"
        };
        assert!(send(&app, "DELETE", path, &token, None).await.status.is_success());
        let before = send(&app, "GET", "/npm/retry-removed", &token, None).await;
        let retry = send(&app, "PUT", "/npm/retry-removed", &token, Some(&doc)).await;
        assert_eq!(retry.status, StatusCode::FORBIDDEN, "{}", retry.text);
        assert!(
            retry.text.contains("Cannot republish previously unpublished version"),
            "{}",
            retry.text,
        );
        let version = send(&app, "GET", "/npm/retry-removed/1.1.0-beta.0", &token, None).await;
        assert_eq!(version.status, StatusCode::NOT_FOUND);
        if !whole_package {
            let retained = send(&app, "PUT", "/npm/retry-removed", &token, Some(&other)).await;
            assert_eq!(retained.status, StatusCode::CREATED, "{}", retained.text);
            let metadata = send(&app, "GET", "/npm/retry-removed", &token, None).await;
            assert_eq!(metadata.body["dist-tags"], before.body["dist-tags"]);
        }
    }
}

#[tokio::test]
async fn a_batch_retry_after_unpublishing_one_member_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let batch = json!({ "packages": [
        publish_doc("batch-kept", "1.0.0", b"kept"),
        publish_doc("batch-removed", "1.0.0", b"removed"),
    ] });
    let committed =
        send_with_key(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch), Some("k1")).await;
    assert_eq!(committed.status, StatusCode::CREATED);
    assert!(
        send(&app, "DELETE", "/npm/batch-removed/-rev/anything", &token, None)
            .await
            .status
            .is_success(),
    );
    let retry =
        send_with_key(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch), Some("k1")).await;
    assert_eq!(retry.status, StatusCode::CONFLICT, "{}", retry.text);
    assert!(
        retry.text.contains("Cannot publish over the previously published version"),
        "{}",
        retry.text,
    );
    assert_eq!(
        send(&app, "GET", "/npm/batch-removed", &token, None).await.status,
        StatusCode::NOT_FOUND,
    );
    assert_eq!(
        send(&app, "GET", "/npm/batch-kept/1.0.0", &token, None).await.status,
        StatusCode::OK,
    );
}

/// Leave `bob` alone allowed to publish.
fn only_bob_publishes(rules: &PackageRules) {
    let bob = AccessList::from_tokens(["bob"]);
    let lists = RuleOverride { publish: Some(bob.clone()), ..RuleOverride::default() };
    let table = rules.snapshot();
    let overrides = table
        .rules()
        .iter()
        .map(|rule| (rule.pattern.clone(), lists.clone()))
        .collect();
    let changed = table.with_overrides(None, overrides).unwrap();
    rules.replace(changed.with_default_publish(bob));
}

#[tokio::test]
async fn a_retry_needs_current_publish_permission() {
    let tmp = TempDir::new().unwrap();
    let config = sqlite_config(tmp.path());
    let rules = config.routing.hosted["local"].rules.clone();
    let app = router(config);
    let token = token(&app).await;
    let doc = publish_doc("revoked", "1.0.0", b"bytes");
    let first = send(&app, "PUT", "/npm/revoked", &token, Some(&doc)).await;
    assert_eq!(first.status, StatusCode::CREATED);
    let batch = json!({ "packages": [publish_doc("batched", "1.0.0", b"batched")] });
    let committed =
        send_with_key(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch), Some("k1")).await;
    assert_eq!(committed.status, StatusCode::CREATED);

    only_bob_publishes(&rules);
    let retry = send(&app, "PUT", "/npm/revoked", &token, Some(&doc)).await;
    assert_eq!((retry.status, retry.generation), (StatusCode::FORBIDDEN, None));
    assert_eq!(retry.body.get("publication"), None);
    let batch_retry =
        send_with_key(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch), Some("k1")).await;
    assert_eq!((batch_retry.status, batch_retry.generation), (StatusCode::FORBIDDEN, None));

    // The outcome is still there to read.
    let publication =
        send(&app, "GET", "/-/publications/-?member=npm:revoked@1.0.0", &token, None).await;
    assert_eq!((publication.status, &publication.body["id"]), (StatusCode::OK, &json!(1)));
}

#[tokio::test]
async fn a_publication_name_is_never_a_decimal_number() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let batch = json!({ "name": "12", "packages": [publish_doc("numbered", "1.0.0", b"bytes")] });
    let refused = send(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch)).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert!(refused.text.contains("decimal number"), "{}", refused.text);
    let numbered = send(&app, "GET", "/npm/numbered", &token, None).await;
    assert_eq!(numbered.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_batch_is_one_publication() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let batch = json!({
        "name": "v2.0.0",
        "packages": [publish_doc("core", "2.0.0", b"core"), publish_doc("widgets", "2.0.0", b"widgets")],
    });
    let committed =
        send_with_key(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch), Some("k1")).await;
    assert_eq!(committed.status, StatusCode::CREATED);
    assert_eq!(committed.generation.as_deref(), Some(":1"));

    let publication = send(&app, "GET", "/-/publications/-/v2.0.0", &token, None).await;
    assert_eq!(publication.status, StatusCode::OK);
    assert_eq!(publication.body["id"], 1);
    assert_eq!(publication.body["publisher"], "alice");
    let members: Vec<&str> = publication.body["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member["name"].as_str().unwrap())
        .collect();
    assert_eq!(members, ["core", "widgets"]);
    let by_member =
        send(&app, "GET", "/-/publications/-?member=npm:widgets@2.0.0", &token, None).await;
    assert_eq!(by_member.body["name"], "v2.0.0");
    let missing = send(&app, "GET", "/-/publications/-/v9", &token, None).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    let replay =
        send_with_key(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch), Some("k1")).await;
    assert_eq!((replay.status, replay.generation), (StatusCode::CREATED, committed.generation));
    let other = json!({ "packages": [publish_doc("gadgets", "1.0.0", b"gadgets")] });
    let reused =
        send_with_key(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&other), Some("k1")).await;
    assert_eq!(reused.status, StatusCode::CONFLICT);
    let gadgets = send(&app, "GET", "/npm/gadgets", &token, None).await;
    assert_eq!(gadgets.status, StatusCode::NOT_FOUND);
    // The refused commit took the archive it had promoted with it.
    assert!(
        crate::storage::served_file(
            &sqlite_config(tmp.path()),
            ("", Ecosystem::Npm),
            "gadgets",
            "gadgets-1.0.0.tgz"
        )
        .await
        .is_none(),
    );
    assert_eq!(crate::storage::stored_archives(tmp.path(), "", Ecosystem::Npm).len(), 2);
}

#[tokio::test]
async fn a_publication_name_is_bounded() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let batch =
        json!({ "name": "n".repeat(215), "packages": [publish_doc("named", "1.0.0", b"bytes")] });
    let refused = send(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch)).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    let named = send(&app, "GET", "/npm/named", &token, None).await;
    assert_eq!(named.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_member_lookup_hides_a_package_the_caller_cannot_read() {
    let tmp = TempDir::new().unwrap();
    let config = sqlite_config(tmp.path());
    let rules = config.routing.hosted["local"].rules.clone();
    let app = router(config);
    let alice = token(&app).await;
    let bob = token_of(&app, "bob").await;
    let batch = json!({
        "packages": [
            publish_doc("@private/hidden", "1.0.0", b"hidden"),
            publish_doc("@foo/open", "1.0.0", b"open"),
        ],
    });
    let committed = send(&app, "PUT", "/-/pnpr/v0/publish", &bob, Some(&batch)).await;
    assert_eq!(committed.status, StatusCode::CREATED, "{}", committed.text);
    only_bob_reads(&rules, "@private/*");

    let hidden =
        send(&app, "GET", "/-/publications/-?member=npm:@private/hidden@1.0.0", &alice, None).await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    let open =
        send(&app, "GET", "/-/publications/-?member=npm:@foo/open@1.0.0", &alice, None).await;
    assert_eq!(open.status, StatusCode::OK);
    assert_eq!(open.body["members"].as_array().map(Vec::len), Some(1));
    let shown =
        send(&app, "GET", "/-/publications/-?member=npm:@private/hidden@1.0.0", &bob, None).await;
    assert_eq!(shown.body["members"].as_array().map(Vec::len), Some(2));

    let crate_member =
        send(&app, "GET", "/-/publications/-?member=cargo:serde@1.0.0", &bob, None).await;
    assert_eq!(crate_member.status, StatusCode::BAD_REQUEST);
}

/// Leave `bob` alone allowed to read the packages of `pattern`.
fn only_bob_reads(rules: &PackageRules, pattern: &str) {
    let pattern = PackagePattern::parse(pattern, Ecosystem::Npm).unwrap();
    let lists =
        RuleOverride { access: Some(AccessList::from_tokens(["bob"])), ..RuleOverride::default() };
    let changed = rules
        .snapshot()
        .with_overrides(None, vec![(pattern, lists)])
        .unwrap();
    rules.replace(changed);
}

#[tokio::test]
async fn an_empty_publication_name_names_nothing() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    for package in ["first", "second"] {
        let batch = json!({ "name": "", "packages": [publish_doc(package, "1.0.0", b"bytes")] });
        let committed = send(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch)).await;
        assert_eq!(committed.status, StatusCode::CREATED, "{}", committed.text);
    }
    let publication = send(&app, "GET", "/-/publications/-/2", &token, None).await;
    assert_eq!(publication.body["name"], Value::Null);
}

#[tokio::test]
async fn concurrent_identical_publishes_get_one_outcome() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let doc = publish_doc("raced", "1.0.0", b"bytes");
    let first = send(&app, "PUT", "/npm/raced", &token, Some(&doc));
    let second = send(&app, "PUT", "/npm/raced", &token, Some(&doc));
    let (first, second) = tokio::join!(first, second);
    for answer in [&first, &second] {
        assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.text);
    }
    assert_eq!(first.generation, second.generation);
}

#[tokio::test]
async fn a_member_lookup_answers_only_for_a_package_its_registry_still_claims() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let batch = json!({ "packages": [publish_doc("unclaimed", "1.0.0", b"bytes")] });
    let committed = send(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch)).await;
    assert_eq!(committed.status, StatusCode::CREATED);

    // The same store, served by a registry whose namespace no longer covers
    // the package.
    let mut narrowed = sqlite_config(tmp.path());
    let patterns = vec![PackagePattern::parse("@foo/*", Ecosystem::Npm).unwrap()];
    let graph = [
        ("local".to_string(), Registry::Hosted { patterns }),
        ("main".to_string(), Registry::Router { sources: vec!["local".to_string()] }),
    ];
    narrowed.routing.registries =
        Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    let narrowed = router(narrowed);
    let token = self::token(&narrowed).await;
    let member =
        send(&narrowed, "GET", "/-/publications/-?member=npm:unclaimed@1.0.0", &token, None).await;
    assert_eq!(member.status, StatusCode::NOT_FOUND);
    let publication = send(&narrowed, "GET", "/-/publications/-/1", &token, None).await;
    assert_eq!(publication.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_refused_publish_keeps_the_bytes_a_committed_file_maps() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let batch = json!({ "packages": [publish_doc("first", "1.0.0", b"first")] });
    let committed =
        send_with_key(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch), Some("k1")).await;
    assert_eq!(committed.status, StatusCode::CREATED);
    let same = json!({ "packages": [publish_doc("same", "1.0.0", b"same")] });
    let placed =
        send_with_key(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&same), Some("k2")).await;
    assert_eq!(placed.status, StatusCode::CREATED);
    let stored = crate::storage::served_file(
        &sqlite_config(tmp.path()),
        ("", Ecosystem::Npm),
        "same",
        "same-1.0.0.tgz",
    )
    .await
    .unwrap();

    // A publish refused at its commit stages the same bytes, finds them live,
    // and stores nothing, so it has nothing of its own to discard.
    let reused = json!({ "packages": [publish_doc("same", "1.0.0", b"same"), publish_doc("third", "1.0.0", b"third")] });
    let refused =
        send_with_key(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&reused), Some("k1")).await;
    assert!(refused.status.is_client_error(), "{:?}", refused.status);
    assert_eq!(
        crate::storage::served_file(
            &sqlite_config(tmp.path()),
            ("", Ecosystem::Npm),
            "same",
            "same-1.0.0.tgz"
        )
        .await,
        Some(stored),
    );
    assert!(
        crate::storage::served_file(
            &sqlite_config(tmp.path()),
            ("", Ecosystem::Npm),
            "third",
            "third-1.0.0.tgz"
        )
        .await
        .is_none(),
    );
    assert_eq!(crate::storage::stored_archives(tmp.path(), "", Ecosystem::Npm).len(), 2);
}
