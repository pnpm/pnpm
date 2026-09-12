use super::{
    Body, Config, Request, ServiceExt, StatusCode, TempDir, add_user_and_get_token, body_bytes,
    body_json, common, json, publish_doc, router, sri_sha512, static_config,
};

#[tokio::test]
async fn anonymous_request_to_protected_package_returns_401() {
    let storage = common::build_storage();
    let app = router(static_config(storage.path().to_path_buf()));
    // The fixture publishes @pnpm.e2e/needs-auth — but our access
    // policy still requires auth for it because the package name
    // matches the `@pnpm.e2e/needs-auth` policy rule.
    let response = app
        .oneshot(Request::get("/@pnpm.e2e/needs-auth").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn update_packument_rejects_a_non_string_dist_integrity() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let publish = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&publish_doc("mypkg", "1.0.0", b"real")).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(publish).await.unwrap().status(), StatusCode::CREATED);

    let get =
        app.clone().oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap();
    let mut packument = body_json(get.into_body()).await;
    // A present-but-non-string integrity must be rejected — otherwise it slips
    // past the string-only immutability check and breaks tarball serving.
    packument["versions"]["1.0.0"]["dist"]["integrity"] = json!(null);
    let request = Request::put("/mypkg/-rev/1-0")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&packument).unwrap()))
        .unwrap();
    assert_eq!(app.oneshot(request).await.unwrap().status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn deprecating_an_existing_version_without_an_attachment_is_allowed() {
    // `pnpm deprecate`/`undeprecate` re-PUT the packument with no attachments
    // and an unchanged `dist`, only flipping `deprecated`. Regression for
    // pnpm/pnpm#12646, which 409-rejected that and broke `pnpm deprecate`.
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let first = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&publish_doc("mypkg", "1.0.0", b"original")).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(first).await.unwrap().status(), StatusCode::CREATED);

    let get =
        app.clone().oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap();
    let hosted = body_json(get.into_body()).await;
    let hosted_dist = hosted["versions"]["1.0.0"]["dist"].clone();
    // Baseline so the dist-preservation assertion below isn't vacuous.
    assert_eq!(hosted_dist["integrity"].as_str(), Some(sri_sha512(b"original").as_str()));
    let body = json!({
        "_id": "mypkg",
        "name": "mypkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "deprecated": "use 2.0.0 instead",
                "dist": hosted_dist,
            }
        }
    });
    let deprecate = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(deprecate).await.unwrap().status(), StatusCode::CREATED);

    let after = body_json(
        app.oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap().into_body(),
    )
    .await;
    assert_eq!(after["versions"]["1.0.0"]["deprecated"], "use 2.0.0 instead");
    assert_eq!(after["versions"]["1.0.0"]["dist"], hosted["versions"]["1.0.0"]["dist"]);
}

#[tokio::test]
async fn malformed_version_entry_cannot_corrupt_a_hosted_version() {
    // A metadata-only PUT that sends a non-object (e.g. `null`) for a hosted
    // version must not overwrite — and thereby erase the `dist` of — that
    // version. The hosted manifest is kept untouched.
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let first = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&publish_doc("mypkg", "1.0.0", b"original")).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(first).await.unwrap().status(), StatusCode::CREATED);

    let before = body_json(
        app.clone()
            .oneshot(Request::get("/mypkg").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .into_body(),
    )
    .await;

    let body = json!({ "_id": "mypkg", "name": "mypkg", "versions": { "1.0.0": null } });
    let malformed = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    // Accepted (no integrity change to reject) but the malformed entry is ignored.
    assert_eq!(app.clone().oneshot(malformed).await.unwrap().status(), StatusCode::CREATED);

    // The hosted version is intact — its `dist` was not erased.
    let after = body_json(
        app.oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap().into_body(),
    )
    .await;
    assert_eq!(after["versions"]["1.0.0"], before["versions"]["1.0.0"]);
    assert!(after["versions"]["1.0.0"]["dist"]["integrity"].is_string());
}

#[tokio::test]
async fn metadata_put_cannot_inject_a_tarball_less_version() {
    // A metadata-only PUT must not be able to add a brand-new version entry
    // with no attachment — that would advertise a version with no hosted
    // tarball (installs 404) and block a later real publish of it (409).
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let first = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&publish_doc("mypkg", "1.0.0", b"original")).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(first).await.unwrap().status(), StatusCode::CREATED);

    let body = json!({
        "_id": "mypkg",
        "name": "mypkg",
        "dist-tags": { "latest": "9.9.9" },
        "versions": {
            "9.9.9": {
                "name": "mypkg",
                "version": "9.9.9",
                "dist": {
                    "tarball": "http://example.test/mypkg/-/mypkg-9.9.9.tgz",
                    "integrity": "sha512-AAAA",
                },
            }
        }
    });
    let squat = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(squat).await.unwrap().status(), StatusCode::BAD_REQUEST);

    // 9.9.9 was never added.
    let after = body_json(
        app.oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap().into_body(),
    )
    .await;
    assert!(after["versions"].get("9.9.9").is_none());
}

/// When the same tarball filename exists in both stores, `open_hosted_blob`
/// serves the hosted copy — a stale proxied copy can't shadow it.
#[tokio::test]
async fn hosted_tarball_is_preferred_over_a_cached_copy() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let hosted_bytes = b"hosted-bytes";
    let body = publish_doc("pref-pkg", "1.0.0", hosted_bytes);
    let request = Request::put("/pref-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    // Plant a divergent proxied copy with the same filename.
    let cached = storage.join(".pnpr-cache").join("pref-pkg");
    std::fs::create_dir_all(&cached).unwrap();
    std::fs::write(cached.join("pref-pkg-1.0.0.tgz"), b"stale-proxied-bytes").unwrap();

    let response = app
        .oneshot(Request::get("/pref-pkg/-/pref-pkg-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, hosted_bytes);
}

#[tokio::test]
async fn search_finds_packages_by_substring_in_local_storage() {
    let storage = common::build_storage();
    let app = router(static_config(storage.path().to_path_buf()));
    let response = app
        .oneshot(Request::get("/-/v1/search?text=no-deps&size=20").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    let objects = body["objects"].as_array().expect("objects is array");
    assert!(!objects.is_empty(), "expected no-deps to match the storage fixture");
    let names: Vec<&str> =
        objects.iter().map(|object| object["package"]["name"].as_str().unwrap()).collect();
    assert!(names.iter().any(|n| n.contains("no-deps")), "got names: {names:?}");
}

#[tokio::test]
async fn search_filters_protected_packages_for_anonymous_callers() {
    let storage = common::build_storage();
    let app = router(static_config(storage.path().to_path_buf()));

    // Anonymous: `@pnpm.e2e/needs-auth` matches the access policy
    // for $authenticated, so search shouldn't surface it.
    let response = app
        .clone()
        .oneshot(Request::get("/-/v1/search?text=needs-auth&size=20").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    let names: Vec<&str> = body["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|object| object["package"]["name"].as_str().unwrap())
        .collect();
    assert!(
        !names.contains(&"@pnpm.e2e/needs-auth"),
        "anonymous search must not enumerate @pnpm.e2e/needs-auth; got {names:?}",
    );
    assert_eq!(body["total"], names.len(), "total must reflect post-filter count");

    // Authenticated: same query should return the package.
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;
    let response = app
        .oneshot(
            Request::get("/-/v1/search?text=needs-auth&size=20")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    let names: Vec<&str> = body["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|object| object["package"]["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"@pnpm.e2e/needs-auth"),
        "authenticated search must include @pnpm.e2e/needs-auth; got {names:?}",
    );
}

#[tokio::test]
async fn search_returns_empty_for_made_up_query() {
    let storage = common::build_storage();
    let app = router(static_config(storage.path().to_path_buf()));
    let response = app
        .oneshot(
            Request::get("/-/v1/search?text=zzz-does-not-exist-99999&size=20")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    assert_eq!(body["objects"].as_array().unwrap().len(), 0);
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn search_augment_skips_when_upstream_404s() {
    use std::{
        net::{Ipv4Addr, SocketAddr, SocketAddrV4},
        time::Duration,
    };

    let mut upstream = mockito::Server::new_async().await;
    let _mock = upstream
        .mock("GET", "/this-package-definitely-does-not-exist-xyz-123")
        .with_status(404)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
    let mut config = Config::proxy(listen, tmp.path().to_path_buf());
    config.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").url = upstream.url();
    config.public_url = "http://example.test".to_string();
    config.packument_ttl = Duration::from_mins(1);
    let app = router(config);

    let response = app
        .oneshot(
            Request::get(
                "/-/v1/search?text=this-package-definitely-does-not-exist-xyz-123&size=20",
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    assert_eq!(body["objects"].as_array().unwrap().len(), 0);
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn search_returns_empty_objects_in_static_mode() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let response = app
        .oneshot(Request::get("/-/v1/search?text=anything&size=20").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    assert_eq!(body["objects"].as_array().unwrap().len(), 0);
    assert_eq!(body["total"], 0);
}
