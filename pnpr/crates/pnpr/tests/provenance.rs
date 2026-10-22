//! Integration tests for npm publish with sigstore provenance,
//! and the `/-/npm/v1/attestations` and `/-/npm/v1/keys` endpoints.

use crate::{network::loopback_networks, npm::publish_doc, tokens::issue_token};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use pnpr::{AuthState, Config, MaxUsers, router, router_with_auth};
use pnpr_fixtures::MockConfigPresets;
use pnpr_storage::layout::local_blob_path;
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
    time::Duration,
};
use tempfile::TempDir;
use tower::ServiceExt;

const PUBLIC_URL: &str = "http://example.test";

fn static_config(storage: PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, storage);
    config.http.public_url = PUBLIC_URL.to_string();
    config.identity.auth.htpasswd.max_users = MaxUsers::Unlimited;
    config
}

fn proxy_config(upstream: &str, storage: PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::proxy(listen, storage);
    config.routing.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").url =
        upstream.to_string();
    config.http.public_url = PUBLIC_URL.to_string();
    config.http.packument_ttl = Duration::from_mins(1);
    config.routing.route_policy.allowed_private_networks = loopback_networks();
    config
}

async fn body_json(body: Body) -> Value {
    let bytes = to_bytes(body, usize::MAX).await.expect("read body");
    serde_json::from_slice(&bytes).expect("valid JSON")
}

async fn get_req(app: &axum::Router, path: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::get(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

fn sample_sigstore_bundle(predicate_type: &str) -> Value {
    let statement = json!({
        "_type": "https://in-toto.io/Statement/v0.1",
        "predicateType": predicate_type,
        "subject": []
    });
    json!({
        "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
        "verificationMaterial": {},
        "dsseEnvelope": {
            "payload": BASE64.encode(serde_json::to_vec(&statement).unwrap()),
            "payloadType": "application/vnd.in-toto+json",
            "signatures": []
        }
    })
}

fn doc_with_sigstore(
    name: &str,
    version: &str,
    tarball: &[u8],
    sig_filename: &str,
    sig_bundle: &Value,
) -> Value {
    let mut doc = publish_doc(name, version, tarball);
    let attachments = doc["_attachments"].as_object_mut().expect("attachments object");
    let serialized = serde_json::to_vec(sig_bundle).unwrap();
    attachments.insert(
        sig_filename.to_string(),
        json!({
            "content_type": "application/vnd.dev.sigstore.bundle+json;version=0.3",
            "data": BASE64.encode(&serialized),
            "length": serialized.len(),
        }),
    );
    doc
}

fn put_publish(path: &str, token: &str, doc: &Value) -> Request<Body> {
    Request::put(path)
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(doc).unwrap()))
        .unwrap()
}

#[tokio::test]
async fn publish_with_provenance_and_serve_attestations() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(static_config(storage.clone()), auth);

    let doc = doc_with_sigstore(
        "prov-pkg",
        "1.0.0",
        b"tarball-bytes",
        "prov-pkg-1.0.0.sigstore",
        &sample_sigstore_bundle("https://slsa.dev/provenance/v1"),
    );

    let res = app
        .clone()
        .oneshot(put_publish("/npm/prov-pkg", &token, &doc))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let packument = body_json(get_req(&app, "/npm/prov-pkg").await.into_body()).await;
    assert_eq!(
        packument["versions"]["1.0.0"]["dist"]["attestations"]["url"],
        format!("{PUBLIC_URL}/npm/-/npm/v1/attestations/prov-pkg@1.0.0"),
    );
    assert_eq!(
        packument["versions"]["1.0.0"]["dist"]["attestations"]["provenance"]["predicateType"],
        "https://slsa.dev/provenance/v1",
    );

    let attest_res = get_req(&app, "/npm/-/npm/v1/attestations/prov-pkg@1.0.0").await;
    assert_eq!(attest_res.status(), StatusCode::OK);
    let payload = body_json(attest_res.into_body()).await;
    let items = payload["attestations"].as_array().expect("array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["predicateType"], "https://slsa.dev/provenance/v1");
    assert_eq!(items[0]["bundle"]["mediaType"], "application/vnd.dev.sigstore.bundle.v0.3+json");

    for bad_path in [
        "/npm/-/npm/v1/attestations/prov-pkg@2.0.0",
        "/npm/-/npm/v1/attestations/prov-pkg@1.0.0/subpath",
        "/npm/-/npm/v1/attestations/prov-pkg@",
        "/npm/-/npm/v1/attestations/@prov-pkg",
    ] {
        assert_eq!(get_req(&app, bad_path).await.status(), StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn publish_scoped_package_with_sigstore_json_attachment() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(static_config(storage.clone()), auth);

    let doc = doc_with_sigstore(
        "@scope/prov-pkg",
        "2.0.0",
        b"tarball-bytes-2",
        "prov-pkg-2.0.0.sigstore.json",
        &sample_sigstore_bundle("https://slsa.dev/provenance/v0.2"),
    );

    let res = app
        .clone()
        .oneshot(put_publish("/npm/@scope%2Fprov-pkg", &token, &doc))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let packument = body_json(get_req(&app, "/npm/@scope%2Fprov-pkg").await.into_body()).await;
    assert_eq!(
        packument["versions"]["2.0.0"]["dist"]["attestations"]["url"],
        format!("{PUBLIC_URL}/npm/-/npm/v1/attestations/@scope/prov-pkg@2.0.0"),
    );

    // Both percent-encoded and unencoded slash paths must succeed.
    for path in [
        "/npm/-/npm/v1/attestations/@scope%2Fprov-pkg@2.0.0",
        "/npm/-/npm/v1/attestations/@scope/prov-pkg@2.0.0",
    ] {
        let res = get_req(&app, path).await;
        assert_eq!(res.status(), StatusCode::OK);
        let payload = body_json(res.into_body()).await;
        assert_eq!(payload["attestations"][0]["predicateType"], "https://slsa.dev/provenance/v0.2");
    }
}

#[tokio::test]
async fn unpublish_tarball_cleans_up_sigstore_blob() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(static_config(storage.clone()), auth);

    let doc = doc_with_sigstore(
        "del-pkg",
        "1.0.0",
        b"tarball-bytes",
        "del-pkg-1.0.0.sigstore",
        &sample_sigstore_bundle("https://slsa.dev/provenance/v1"),
    );

    let res = app
        .clone()
        .oneshot(put_publish("/npm/del-pkg", &token, &doc))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    assert!(local_blob_path(&storage, "", "del-pkg", "del-pkg-1.0.0.sigstore").exists());

    let del = Request::delete("/npm/del-pkg/-/del-pkg-1.0.0.tgz/-rev/anything")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(del)
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED,
    );

    assert!(!local_blob_path(&storage, "", "del-pkg", "del-pkg-1.0.0.sigstore").exists());

    let get_attest = Request::get("/npm/-/npm/v1/attestations/del-pkg@1.0.0")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(get_attest)
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn hosted_keys_endpoint_returns_empty_keys() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));

    for path in ["/npm/-/npm/v1/keys", "/npm/~local/-/npm/v1/keys"] {
        let res = get_req(&app, path).await;
        assert_eq!(res.status(), StatusCode::OK);
        let payload = body_json(res.into_body()).await;
        assert_eq!(payload, json!({ "keys": [] }));
    }
}

#[tokio::test]
async fn upstream_attestations_and_keys_proxy() {
    let mut upstream = mockito::Server::new_async().await;
    let keys_mock = upstream
        .mock("GET", "/-/npm/v1/keys")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"keys":[{"keyid":"sigstore-root"}]}"#)
        .create_async()
        .await;

    let attest_mock = upstream
        .mock("GET", "/-/npm/v1/attestations/proxied-pkg@1.0.0")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"attestations":[{"predicateType":"https://slsa.dev/provenance/v1","bundle":{}}]}"#,
        )
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let config = proxy_config(&upstream.url(), tmp.path().to_path_buf());
    let app = router(config);

    // Keys proxied
    let keys_res = get_req(&app, "/npm/-/npm/v1/keys").await;
    assert_eq!(keys_res.status(), StatusCode::OK);
    let keys_payload = body_json(keys_res.into_body()).await;
    assert_eq!(keys_payload["keys"][0]["keyid"], "sigstore-root");

    // Attestations proxied
    let attest_res = get_req(&app, "/npm/-/npm/v1/attestations/proxied-pkg@1.0.0").await;
    assert_eq!(attest_res.status(), StatusCode::OK);
    let attest_payload = body_json(attest_res.into_body()).await;
    assert_eq!(
        attest_payload["attestations"][0]["predicateType"],
        "https://slsa.dev/provenance/v1",
    );

    keys_mock.assert_async().await;
    attest_mock.assert_async().await;
}

#[tokio::test]
async fn invalid_sigstore_attachment_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(static_config(tmp.path().to_path_buf()), auth);

    let mut doc = publish_doc("bad-sig", "1.0.0", b"tarball");
    doc["_attachments"]["bad-sig-1.0.0.sigstore"] = json!({
        "content_type": "application/json",
        "data": "not-valid-json-or-base64-json!!!",
    });

    let res = app
        .oneshot(put_publish("/npm/bad-sig", &token, &doc))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn orphan_sigstore_attachment_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(static_config(tmp.path().to_path_buf()), auth);

    let mut doc = doc_with_sigstore(
        "orphan-sig",
        "1.0.0",
        b"tarball",
        "orphan-sig-1.0.0.sigstore",
        &sample_sigstore_bundle("https://slsa.dev/provenance/v1"),
    );
    doc["_attachments"]
        .as_object_mut()
        .unwrap()
        .remove("orphan-sig-1.0.0.tgz");

    let res = app
        .oneshot(put_publish("/npm/orphan-sig", &token, &doc))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn duplicate_sigstore_attachments_are_rejected() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(static_config(tmp.path().to_path_buf()), auth);

    let mut doc = doc_with_sigstore(
        "dup-sig",
        "1.0.0",
        b"tarball",
        "dup-sig-1.0.0.sigstore",
        &sample_sigstore_bundle("https://slsa.dev/provenance/v1"),
    );
    let second_bytes =
        serde_json::to_vec(&sample_sigstore_bundle("https://slsa.dev/provenance/v0.2")).unwrap();
    doc["_attachments"]["dup-sig-1.0.0.sigstore.json"] = json!({
        "content_type": "application/json",
        "data": BASE64.encode(&second_bytes),
        "length": second_bytes.len(),
    });

    let res = app
        .oneshot(put_publish("/npm/dup-sig", &token, &doc))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    assert!(
        String::from_utf8_lossy(&bytes).contains("duplicate attachment for dup-sig-1.0.0.sigstore"),
    );
}
