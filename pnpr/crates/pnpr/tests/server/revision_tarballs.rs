use super::{
    AuthState, Body, HeaderValue, PackageRules, Registries, Registry, Request, Router, ServiceExt,
    StatusCode, TempDir, Value, access_rule, body_bytes, config_for, header,
    integrity_addressed_tarball_path, json, package_tar_and_tgz, router_config, router_with_auth,
    sha512_integrity, upstream_endpoint_config,
};
use crate::tokens::issue_token;

/// An upstream-backed registry whose upstream sends `x-upstream-auth`, and
/// the bearer authorization of `alice`, who may read it.
async fn revision_upstream_app(upstream: &mockito::Server, storage: &TempDir) -> (Router, String) {
    let mut config =
        upstream_endpoint_config(&upstream.url(), storage.path().to_path_buf(), "alice");
    config.routing.upstreams
        .get_mut("npmjs")
        .unwrap()
        .headers
        .insert("x-upstream-auth", HeaderValue::from_static("secret"));
    config.routing.registries = Registries::new(
        std::iter::once(("npmjs".to_string(), Registry::Upstream { patterns: Vec::new() }))
            .collect(),
        Some("npmjs".to_string()),
    );
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    (router_with_auth(config, auth), format!("Bearer {token}"))
}

/// A version whose selected revision is `selected`, with an original and a
/// first replacement in its history.
fn revision_version(
    upstream_url: &str,
    original: &[u8],
    replacement: &[u8],
    selected: u64,
) -> Value {
    let record = |revision: u64, tar: &[u8]| {
        let integrity_text = sha512_integrity(tar);
        let path = integrity_addressed_tarball_path(&integrity_text.parse().unwrap()).unwrap();
        json!({
            "revision": revision,
            "integrity": integrity_text,
            "tarball": format!("{upstream_url}/{path}"),
            "manifest": {},
        })
    };
    let history = vec![record(0, original), record(1, replacement)];
    let current = &history[usize::try_from(selected).unwrap()];
    json!({ "name": "foo", "version": "1.0.0", "dist": {
        "tarball": current["tarball"],
        "integrity": current["integrity"],
        "shasum": "0000000000000000000000000000000000000000",
        "signatures": [{ "keyid": "SHA256:key", "sig": "signature" }],
        "revision": selected,
        "revisions": history,
    } })
}

async fn get_with(
    app: &Router,
    path: &str,
    authorization: &str,
    accept_encoding: Option<&str>,
) -> axum::response::Response {
    let mut request = Request::get(path).header(header::AUTHORIZATION, authorization);
    if let Some(coding) = accept_encoding {
        request = request.header(header::ACCEPT_ENCODING, coding);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn upstream_endpoint_preserves_and_serves_revision_tarballs() {
    let mut upstream = mockito::Server::new_async().await;
    let (original, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0"}"#);
    let (replacement, replacement_tgz) =
        package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0","patched":true}"#);
    let revision_path =
        integrity_addressed_tarball_path(&sha512_integrity(&replacement).parse().unwrap()).unwrap();
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": revision_version(&upstream.url(), &original, &replacement, 1) },
    });
    let packument_mock = upstream
        .mock("GET", "/foo")
        .match_header("x-upstream-auth", "secret")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", format!("/{revision_path}").as_str())
        .match_header("x-upstream-auth", "secret")
        .match_header("accept-encoding", "gzip")
        .with_header("content-encoding", "gzip")
        .with_body(&replacement_tgz)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let (app, authorization) = revision_upstream_app(&upstream, &tmp).await;

    let response = get_with(&app, "/npm/~npmjs/foo", &authorization, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let dist = &body["versions"]["1.0.0"]["dist"];
    assert_eq!(dist["revision"], 1);
    assert_eq!(dist["tarball"], format!("http://example.test/npm/~npmjs/{revision_path}"));
    for source_field in ["shasum", "signatures"] {
        assert!(dist.get(source_field).is_none(), "a revision omits dist.{source_field}");
    }

    for path in [format!("/npm/~npmjs/{revision_path}"), format!("/npm/{revision_path}")] {
        let response = get_with(&app, &path, &authorization, None).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/x-tar");
        assert!(
            response
                .headers()
                .get(header::ETAG)
                .is_none(),
        );
        assert_eq!(body_bytes(response.into_body()).await, replacement);

        let gzip = get_with(&app, &path, &authorization, Some("gzip")).await;
        assert_eq!(gzip.headers()[header::CONTENT_ENCODING], "gzip");
        assert_eq!(body_bytes(gzip.into_body()).await, replacement_tgz);
    }

    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn upstream_revision_zero_is_projected_explicitly() {
    let mut upstream = mockito::Server::new_async().await;
    let (original, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0"}"#);
    let (replacement, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0","patched":1}"#);
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": revision_version(&upstream.url(), &original, &replacement, 0) },
    });
    let _packument = upstream
        .mock("GET", "/foo")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let (app, authorization) = revision_upstream_app(&upstream, &tmp).await;

    let response = get_with(&app, "/npm/~npmjs/foo", &authorization, None).await;
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let dist = &body["versions"]["1.0.0"]["dist"];
    dbg!(dist);
    let original_path =
        integrity_addressed_tarball_path(&sha512_integrity(&original).parse().unwrap()).unwrap();
    assert_eq!(dist["revision"], 0);
    assert_eq!(dist["integrity"], sha512_integrity(&original));
    assert_eq!(dist["tarball"], format!("http://example.test/npm/~npmjs/{original_path}"));
    assert_eq!(
        dist["revisions"]
            .as_array()
            .unwrap()
            .len(),
        2,
    );
}

#[tokio::test]
async fn incomplete_upstream_revision_metadata_is_ordinary_metadata() {
    let mut upstream = mockito::Server::new_async().await;
    let (original, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0"}"#);
    let (replacement, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0","patched":1}"#);
    let mut without_original = revision_version(&upstream.url(), &original, &replacement, 1);
    without_original["dist"]["revisions"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    let mut without_marker = revision_version(&upstream.url(), &original, &replacement, 1);
    without_marker["dist"]
        .as_object_mut()
        .unwrap()
        .remove("revision");
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": without_original, "2.0.0": without_marker },
    });
    let _packument = upstream
        .mock("GET", "/foo")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let (app, authorization) = revision_upstream_app(&upstream, &tmp).await;

    let response = get_with(&app, "/npm/~npmjs/foo", &authorization, None).await;
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    for version in ["1.0.0", "2.0.0"] {
        let dist = &body["versions"][version]["dist"];
        dbg!(dist);
        assert!(dist.get("revision").is_none(), "{version}");
        assert!(dist.get("revisions").is_none(), "{version}");
    }
}

#[tokio::test]
async fn an_identity_upstream_revision_is_served_in_every_coding() {
    let mut upstream = mockito::Server::new_async().await;
    let (tar, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0"}"#);
    let revision_path =
        integrity_addressed_tarball_path(&sha512_integrity(&tar).parse().unwrap()).unwrap();
    let tarball_mock = upstream
        .mock("GET", format!("/{revision_path}").as_str())
        .with_body(&tar)
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let (app, authorization) = revision_upstream_app(&upstream, &tmp).await;
    let path = format!("/npm/~npmjs/{revision_path}");

    let identity = get_with(&app, &path, &authorization, None).await;
    assert_eq!(identity.status(), StatusCode::OK);
    assert_eq!(identity.headers()[header::CONTENT_TYPE], "application/x-tar");
    assert!(
        identity.headers()[header::VARY]
            .to_str()
            .unwrap()
            .contains("accept-encoding"),
    );
    assert_eq!(body_bytes(identity.into_body()).await, tar);
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn a_revision_that_does_not_match_its_digest_is_neither_served_nor_cached() {
    let mut upstream = mockito::Server::new_async().await;
    let (tar, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0"}"#);
    let (_, tampered_tgz) = package_tar_and_tgz(r#"{"name":"foo","version":"6.6.6"}"#);
    let revision_path =
        integrity_addressed_tarball_path(&sha512_integrity(&tar).parse().unwrap()).unwrap();
    let tarball_mock = upstream
        .mock("GET", format!("/{revision_path}").as_str())
        .with_header("content-encoding", "gzip")
        .with_body(&tampered_tgz)
        .expect(2)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let (app, authorization) = revision_upstream_app(&upstream, &tmp).await;
    let path = format!("/npm/~npmjs/{revision_path}");

    for _ in 0..2 {
        let response = get_with(&app, &path, &authorization, None).await;
        assert!(!response.status().is_success(), "{:?}", response.status());
    }
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn upstream_revision_tarball_does_not_follow_redirects() {
    let mut upstream = mockito::Server::new_async().await;
    let integrity = sha512_integrity(b"expected revision tarball").parse().unwrap();
    let revision_path = integrity_addressed_tarball_path(&integrity).unwrap();
    let redirect = upstream
        .mock("GET", format!("/{revision_path}").as_str())
        .with_status(302)
        .with_header("location", "/redirected.tgz")
        .expect(1)
        .create_async()
        .await;
    let redirected = upstream
        .mock("GET", "/redirected.tgz")
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let config = upstream_endpoint_config(&upstream.url(), tmp.path().to_path_buf(), "alice");
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(config, auth);
    let response = app
        .oneshot(
            Request::get(format!("/npm/~npmjs/{revision_path}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    redirect.assert_async().await;
    redirected.assert_async().await;
}

#[tokio::test]
async fn revision_tarballs_require_a_concrete_registry_without_package_access_rules() {
    let mut upstream = mockito::Server::new_async().await;
    let integrity = sha512_integrity(b"unreachable revision tarball").parse().unwrap();
    let revision_path = integrity_addressed_tarball_path(&integrity).unwrap();
    let tarball = upstream
        .mock("GET", format!("/{revision_path}").as_str())
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let router_app = router_with_auth(
        router_config(&upstream.url(), &upstream.url(), tmp.path().join("router")),
        AuthState::in_memory(),
    );
    for path in [format!("/npm/~main/{revision_path}"), format!("/npm/{revision_path}")] {
        let response = router_app
            .clone()
            .oneshot(
                Request::get(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    let mut config = config_for(&upstream.url(), tmp.path().join("package-access"));
    config.routing.upstreams.get_mut("npmjs").unwrap().rules =
        PackageRules::new(vec![access_rule("restricted", "$authenticated")], None);
    let response = router_with_auth(config, AuthState::in_memory())
        .oneshot(
            Request::get(format!("/npm/~npmjs/{revision_path}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    tarball.assert_async().await;
}
