//! The transparency log of a hosted registry's release records,
//! `<base>/-/log/`, and a record's evidence,
//! `<base>/<name>/<version>+r<revision>/evidence`.

use super::{
    AccessList, AuthState, Body, Config, Ecosystem, HostedConfig, PackagePattern, PackageRules,
    Registries, Registry, Request, ServiceExt, StatusCode, access_rule, body_bytes, body_json,
    config_for, header, hosted_publish_request, router_with_auth, tar_of,
};
use crate::tokens::issue_token;
use axum::Router;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use pnpm_release_record::{Envelope, PredicateType, RecordDigest, StoredRecord, verify_statement};
use pnpm_transparency_log::{
    CheckpointPolicy, Hash, KeySet, NoteSigner, decode_bundle, verify_inclusion,
};
use std::sync::Arc;
use tempfile::TempDir;

/// Go's `note` test key.
const LOG_SIGNING_KEY: &str =
    "PRIVATE+KEY+PeterNeumann+c74f20a3+AYEKFALVFGyNhPJEMzD1QIDr+Y7hfZx09iUvxdXHKDFz";

/// The origin of the `acme` organization's npm log.
const ORIGIN: &str = "PeterNeumann/npm/~acme";

/// A hosted `acme` registry with `SQLite` metadata and a log signing key, the
/// path-less default through the `main` router. `rules` refine who may read
/// its packages, which anyone may read by default.
fn acme_config(storage: &std::path::Path, rules: Vec<pnpr::PackageRule>) -> Config {
    let mut config = config_for("http://127.0.0.1:1", storage.to_path_buf());
    let rules = PackageRules::new(rules, Some(AccessList::from_tokens(["$all"])))
        .with_default_unpublish(AccessList::from_tokens(["$authenticated"]));
    config.routing.hosted.insert("acme".to_string(), HostedConfig::new("acme", rules));
    let graph = vec![
        ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
        (
            "acme".to_string(),
            Registry::Hosted {
                patterns: vec![PackagePattern::parse("@acme/*", Ecosystem::Npm).unwrap()],
            },
        ),
        ("main".to_string(), Registry::Router { sources: vec!["acme".to_string()] }),
    ];
    config.routing.registries =
        Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    config.storage.metadata = pnpr_config::MetadataBackend::Sqlite(storage.join("metadata.sqlite"));
    let signer = NoteSigner::from_private_key(LOG_SIGNING_KEY).unwrap();
    let statements = statement_signer();
    config.release_records.log_signing_key = Some(Arc::new(signer));
    config.release_records.statement_signing_key = Some(Arc::new(statements));
    config
}

/// The key that signs the registry's statements.
fn statement_signer() -> NoteSigner {
    let signer = NoteSigner::from_private_key(LOG_SIGNING_KEY).unwrap();
    signer.renamed("registry.example/statements").unwrap()
}

async fn setup(rules: Vec<pnpr::PackageRule>) -> (TempDir, Router, String) {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path(), rules), auth);
    (tmp, app, token)
}

async fn publish(app: &Router, token: &str, package: &str, version: &str) {
    let manifest = format!(r#"{{"name":"{package}","version":"{version}"}}"#);
    let tar = tar_of(&[("package/package.json", manifest.as_bytes())]);
    let request = hosted_publish_request(
        &format!("/npm/~acme/{package}"),
        package,
        version,
        &super::gzip(&tar),
        token,
    );
    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

async fn get(app: &Router, uri: &str, token: Option<&str>) -> axum::response::Response {
    let mut request = Request::get(uri);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

fn log_policy() -> CheckpointPolicy {
    let signer = NoteSigner::from_private_key(LOG_SIGNING_KEY).unwrap();
    let key = signer
        .renamed(ORIGIN)
        .unwrap()
        .verifier_key()
        .clone();
    CheckpointPolicy::new(
        ORIGIN,
        KeySet::new(vec![key], 1).unwrap(),
        KeySet::new(Vec::new(), 0).unwrap(),
    )
    .unwrap()
}

/// A record's evidence proves that its log holds the record's entry, under a
/// checkpoint its log key signed, and the log serves the checkpoint and tiles
/// a monitor reads.
#[tokio::test]
async fn a_record_is_proven_by_its_evidence() {
    let (_tmp, app, token) = setup(Vec::new()).await;
    publish(&app, &token, "@acme/widget", "1.0.0").await;
    let record =
        body_bytes(get(&app, "/npm/@acme%2Fwidget/1.0.0+r0", None).await.into_body()).await;
    let entry = StoredRecord::parse(record.clone())
        .unwrap()
        .log_entry()
        .unwrap()
        .to_bytes();
    let mut signed = Vec::new();

    for uri in ["/npm/@acme/widget/1.0.0+r0/evidence", "/npm/@acme%2Fwidget/1.0.0+r0/evidence"] {
        let response = get(&app, uri, None).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        let evidence = body_json(response.into_body()).await;
        assert_eq!(evidence["record"], RecordDigest::of(&record).to_string());
        let note = evidence["inclusion"]["checkpoint"].as_str().unwrap();
        let tree_head = log_policy().verify(note.as_bytes()).unwrap().tree_head;
        let proof: Vec<Hash> = evidence["inclusion"]["proof"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hash| {
                Hash(
                    STANDARD
                        .decode(hash.as_str().unwrap())
                        .unwrap()
                        .try_into()
                        .unwrap(),
                )
            })
            .collect();
        let index = evidence["inclusion"]["index"].as_u64().unwrap();
        verify_inclusion(&proof, &tree_head, index, &entry).unwrap();
        let statements = evidence["statements"].as_array().unwrap();
        assert_eq!(statements.len(), 1);
        let envelope: Envelope = serde_json::from_value(statements[0].clone()).unwrap();
        let keys = KeySet::new(vec![statement_signer().verifier_key().clone()], 1).unwrap();
        let stored = StoredRecord::parse(record.clone()).unwrap();
        let statement =
            verify_statement(&envelope, &keys, &stored, PredicateType::Acceptance).unwrap();
        assert_eq!(statement.predicate["interpretation"], "npm-install-manifest/1");
        signed.push(envelope);
    }
    // A statement is signed again on every read, and the same key signs the
    // same envelope.
    assert_eq!(signed[0], signed[1]);
    for missing in [
        "/npm/@acme%2Fwidget/1.0.0+r1/evidence",
        "/npm/@acme%2Fwidget/2.0.0+r0/evidence",
        "/npm/@acme%2Fwidget/1.0.0/evidence",
    ] {
        assert_eq!(get(&app, missing, None).await.status(), StatusCode::NOT_FOUND, "{missing}");
    }

    let checkpoint = get(&app, "/npm/-/log/checkpoint", None).await;
    assert_eq!(checkpoint.status(), StatusCode::OK);
    let note = body_bytes(checkpoint.into_body()).await;
    assert_eq!(log_policy().verify(&note).unwrap().tree_head.size, 1);
    let tile = get(&app, "/npm/-/log/tile/0/000.p/1", None).await;
    assert_eq!(tile.headers()[header::CACHE_CONTROL], "public, max-age=31536000, immutable");
    assert_eq!(body_bytes(tile.into_body()).await.len(), 32);
    let bundle =
        body_bytes(get(&app, "/npm/-/log/tile/entries/000.p/1", None).await.into_body()).await;
    assert_eq!(decode_bundle(&bundle).unwrap(), vec![entry.as_slice()]);
    for missing in ["/npm/-/log/tile/0/001", "/npm/-/log/tile/0/000.p/9", "/npm/-/log/other"] {
        assert_eq!(get(&app, missing, None).await.status(), StatusCode::NOT_FOUND, "{missing}");
    }

    let capabilities = body_json(get(&app, "/-/pnpr", None).await.into_body()).await;
    assert_eq!(capabilities["pnpr"]["records"], serde_json::json!([1]));
    assert_eq!(
        capabilities["pnpr"]["manifestProfiles"],
        serde_json::json!(["npm-install-manifest/1"]),
    );
}

/// Entry bundles name every logged package, so only a caller who may read
/// every package gets them. Anyone who may read some package gets the
/// checkpoint and hash tiles.
#[tokio::test]
async fn entry_bundles_are_read_only_by_who_may_read_every_package() {
    let (_tmp, app, token) = setup(vec![access_rule("@acme/secret", "$authenticated")]).await;
    publish(&app, &token, "@acme/widget", "1.0.0").await;
    publish(&app, &token, "@acme/secret", "1.0.0").await;
    // The secret package's reader waits for its entry, the log's second.
    let evidence = get(&app, "/npm/@acme%2Fsecret/1.0.0+r0/evidence", Some(&token)).await;
    assert_eq!(evidence.status(), StatusCode::OK);
    let entries = "/npm/-/log/tile/entries/000.p/2";
    for uri in ["/npm/-/log/checkpoint", "/npm/-/log/tile/0/000.p/2"] {
        let response = get(&app, uri, None).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
    }
    // The same refusal for every bundle, whatever it holds.
    for uri in [entries, "/npm/-/log/tile/entries/000.p/1", "/npm/-/log/tile/entries/001"] {
        assert_eq!(get(&app, uri, None).await.status(), StatusCode::NOT_FOUND, "{uri}");
    }
    let secret = get(&app, "/npm/@acme%2Fsecret/1.0.0+r0/evidence", None).await;
    assert_ne!(secret.status(), StatusCode::OK);

    let bundle = get(&app, entries, Some(&token)).await;
    assert_eq!(bundle.status(), StatusCode::OK);
    assert_eq!(bundle.headers()[header::CACHE_CONTROL], "private, no-store");
    assert_eq!(decode_bundle(&body_bytes(bundle.into_body()).await).unwrap().len(), 2);
}

/// A hosted registry serves its organization's log, also when addressed by
/// name, and a registry that reaches no organization serves none.
#[tokio::test]
async fn a_log_is_served_only_where_one_organization_is_reached() {
    let (_tmp, app, token) = setup(Vec::new()).await;
    publish(&app, &token, "@acme/widget", "1.0.0").await;
    let evidence = get(&app, "/npm/~acme/@acme%2Fwidget/1.0.0+r0/evidence", None).await;
    let status = evidence.status();
    let cache_control = evidence
        .headers()
        .get(header::CACHE_CONTROL)
        .cloned();
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&body_bytes(evidence.into_body()).await),
    );
    assert_eq!(cache_control.unwrap(), "no-cache");
    let named = get(&app, "/npm/~acme/-/log/checkpoint", None).await;
    let named_status = named.status();
    let named_cache = named
        .headers()
        .get(header::CACHE_CONTROL)
        .cloned();
    let body = body_bytes(named.into_body()).await;
    assert_eq!(named_status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(named_cache.unwrap(), "private, no-store");
    let upstream = get(&app, "/npm/~npmjs/-/log/checkpoint", None).await;
    assert_eq!(upstream.status(), StatusCode::NOT_FOUND);
}

/// A router built outside a Tokio runtime integrates the entries sequenced
/// before it at the first read of its log.
#[tokio::test]
async fn a_router_built_outside_a_runtime_recovers_its_log() {
    let tmp = TempDir::new().unwrap();
    let mut unsigned = acme_config(tmp.path(), Vec::new());
    unsigned.release_records.log_signing_key = None;
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(unsigned, auth.clone());
    publish(&app, &token, "@acme/widget", "1.0.0").await;
    assert_eq!(get(&app, "/npm/-/log/checkpoint", None).await.status(), StatusCode::NOT_FOUND);

    let config = acme_config(tmp.path(), Vec::new());
    let app = std::thread::spawn(move || router_with_auth(config, auth)).join().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let checkpoint = get(&app, "/npm/-/log/checkpoint", None).await;
        if checkpoint.status() == StatusCode::OK {
            let note = body_bytes(checkpoint.into_body()).await;
            assert_eq!(log_policy().verify(&note).unwrap().tree_head.size, 1);
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the log was never recovered");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
