use super::{
    BASE64, Body, Request, ServiceExt, StatusCode, TempDir, Value, add_user_and_get_token,
    body_bytes, body_json, json, publish_doc, router, sha1_hex, sri_sha512, static_config,
    static_config_with_packages,
};
use base64::Engine;

/// `libnpmpublish` (the library `pnpm publish` / `npm publish` use under
/// the hood) names the `_attachments` key by the full package name —
/// for `@scope/name` that's `@scope/name-1.0.0.tgz`, with a literal `/`
/// in the filename. The server has to accept that shape and normalize
/// it to the canonical `<basename>-<version>.tgz` form on disk, otherwise
/// `pnpm publish` against pnpr fails with 400 for every scoped
/// package — see `recursivePublish.ts` in `@pnpm/releasing.commands`.
#[tokio::test]
async fn publish_accepts_libnpmpublish_scoped_attachment_filename() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"libnpmpublish-form";
    let pkg = "@pnpmtest/lib-pub-form";
    let version = "1.0.0";
    // _attachments key is the FULL scoped name + version, NOT the basename.
    let scoped_filename = format!("{pkg}-{version}.tgz");
    let body = json!({
        "_id": pkg,
        "name": pkg,
        "dist-tags": { "latest": version },
        "versions": {
            version: {
                "name": pkg,
                "version": version,
                "dist": {
                    "tarball": format!("http://localhost:4873/{pkg}/-/lib-pub-form-{version}.tgz"),
                    "shasum": sha1_hex(bytes),
                    "integrity": sri_sha512(bytes),
                },
            },
        },
        "_attachments": {
            scoped_filename: {
                "content_type": "application/octet-stream",
                "data": BASE64.encode(bytes),
                "length": bytes.len(),
            },
        },
    });
    let request = Request::put(format!("/{pkg}"))
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // On disk: canonical `<basename>-<version>.tgz` path, NOT the
    // scoped form. That's where serve_tarball looks.
    let on_disk = storage.join("@pnpmtest/lib-pub-form/lib-pub-form-1.0.0.tgz");
    assert!(on_disk.exists(), "tarball should be persisted at canonical path");
    assert_eq!(std::fs::read(&on_disk).unwrap(), bytes);

    // And it serves back via the spec URL form.
    let served = app
        .oneshot(
            Request::get("/@pnpmtest/lib-pub-form/-/lib-pub-form-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(served.status(), StatusCode::OK);
    let served_bytes = body_bytes(served.into_body()).await;
    assert_eq!(served_bytes, bytes);
}

#[tokio::test]
async fn unpublish_partial_writes_modified_packument() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    // Publish two versions, then PUT a modified packument with the
    // older one removed — simulating the partial-unpublish flow.
    for version in ["1.0.0", "2.0.0"] {
        let body = publish_doc("unpub-partial", version, version.as_bytes());
        let request = Request::put("/unpub-partial")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        app.clone().oneshot(request).await.unwrap();
    }

    let modified = json!({
        "name": "unpub-partial",
        "_rev": "ignored",
        "dist-tags": { "latest": "2.0.0" },
        "versions": {
            "2.0.0": { "name": "unpub-partial", "version": "2.0.0", "dist": {
                "tarball": "http://example.test/unpub-partial/-/unpub-partial-2.0.0.tgz"
            }},
        },
    });
    let request = Request::put("/unpub-partial/-rev/anything")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&modified).unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // GET packument back — should only contain 2.0.0.
    let response = app
        .clone()
        .oneshot(Request::get("/unpub-partial").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let served = body_json(response.into_body()).await;
    assert_eq!(served["versions"].as_object().unwrap().keys().collect::<Vec<_>>(), vec!["2.0.0"]);
    // The PUT body dropped dist.integrity for the retained version; the server
    // restores the *exact* published hash (2.0.0 was published with its version
    // string as the tarball bytes), so the round-trip can neither strip nor
    // corrupt a published version's integrity.
    assert_eq!(
        served["versions"]["2.0.0"]["dist"]["integrity"],
        json!(sri_sha512(b"2.0.0")),
        "the original published integrity must be restored, unchanged",
    );

    // DELETE the 1.0.0 tarball next.
    let request = Request::delete("/unpub-partial/-/unpub-partial-1.0.0.tgz/-rev/anything")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(!storage.join("unpub-partial/unpub-partial-1.0.0.tgz").exists());
    assert!(storage.join("unpub-partial/unpub-partial-2.0.0.tgz").exists());

    // Second DELETE of the same tarball is a no-op — verdaccio
    // returns 201 here too (idempotent). The pnpm unpublish flow
    // tolerates 404 separately as a fallback, but we shouldn't even
    // get there.
    let request = Request::delete("/unpub-partial/-/unpub-partial-1.0.0.tgz/-rev/anything")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

/// Deleting a hosted tarball must also drop any proxied copy with the
/// same filename, so `open_hosted_blob`'s cache fallback can't keep serving
/// the just-removed version.
#[tokio::test]
async fn unpublish_tarball_also_clears_the_proxied_copy() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let body = publish_doc("blend-pkg", "1.0.0", b"hosted-bytes");
    let request = Request::put("/blend-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    // Plant a stale proxied copy of the same tarball in the cache root,
    // as a `proxy:` rule would have left behind.
    let cached = storage.join(".pnpr-cache").join("blend-pkg");
    std::fs::create_dir_all(&cached).unwrap();
    std::fs::write(cached.join("blend-pkg-1.0.0.tgz"), b"stale-proxied-bytes").unwrap();

    let request = Request::delete("/blend-pkg/-/blend-pkg-1.0.0.tgz/-rev/anything")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    assert!(!storage.join("blend-pkg/blend-pkg-1.0.0.tgz").exists());
    assert!(!cached.join("blend-pkg-1.0.0.tgz").exists(), "proxied copy must be removed too");

    // With both stores cleared and no upstream, the version is gone.
    let response = app
        .oneshot(Request::get("/blend-pkg/-/blend-pkg-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unpublish_force_removes_entire_package() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let body = publish_doc("unpub-force", "1.0.0", b"contents");
    let request = Request::put("/unpub-force")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap();
    assert!(storage.join("unpub-force/package.json").exists());

    let request = Request::delete("/unpub-force/-rev/anything")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(!storage.join("unpub-force").exists());

    // Re-fetch returns 404 (static mode + no on-disk packument).
    let response =
        app.oneshot(Request::get("/unpub-force").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unpublish_scoped_tarball_via_six_segment_route() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let body = publish_doc("@scope/unpub", "1.0.0", b"bytes");
    let request = Request::put("/@scope/unpub")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap();
    assert!(storage.join("@scope/unpub/unpub-1.0.0.tgz").exists());

    // pnpm reconstructs the DELETE URL from the rewritten tarball URL
    // in the packument, which uses literal `/` for the scope segment
    // (not `%2F`). That lands on the 6-seg route.
    let request = Request::delete("/@scope/unpub/-/unpub-1.0.0.tgz/-rev/anything")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(!storage.join("@scope/unpub/unpub-1.0.0.tgz").exists());
}

#[tokio::test]
async fn missing_unpublish_policy_denies_destructive_writes() {
    let tmp = TempDir::new().unwrap();
    let (config, storage) = static_config_with_packages(
        &tmp,
        "  'missing-unpublish':
    access: $all
    publish: alice",
    );
    let app = router(config);
    let (app, alice) = add_user_and_get_token(app, "alice", "secret").await;

    let body = publish_doc("missing-unpublish", "1.0.0", b"contents");
    let request = Request::put("/missing-unpublish")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {alice}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    let request = Request::delete("/missing-unpublish/-rev/anything")
        .header("Authorization", format!("Bearer {alice}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::FORBIDDEN);
    assert!(storage.join("missing-unpublish/package.json").exists());
}

#[tokio::test]
async fn packument_replacement_requires_publish_and_unpublish_policy() {
    let tmp = TempDir::new().unwrap();
    let (config, storage) = static_config_with_packages(
        &tmp,
        "  'replace-policy':
    access: $all
    publish: $authenticated
    unpublish: admin",
    );
    let app = router(config);
    let (app, alice) = add_user_and_get_token(app, "alice", "secret").await;
    let (app, admin) = add_user_and_get_token(app, "admin", "secret").await;

    let body = publish_doc("replace-policy", "1.0.0", b"contents");
    let request = Request::put("/replace-policy")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {alice}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    let packument_path = storage.join("replace-policy/package.json");
    let mut replacement: Value =
        serde_json::from_slice(&std::fs::read(&packument_path).unwrap()).unwrap();
    replacement["owner"] = json!("admin");

    let request = Request::put("/replace-policy/-rev/anything")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {alice}"))
        .body(Body::from(serde_json::to_vec(&replacement).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::FORBIDDEN);
    let on_disk: Value = serde_json::from_slice(&std::fs::read(&packument_path).unwrap()).unwrap();
    assert!(on_disk.get("owner").is_none());

    let request = Request::put("/replace-policy/-rev/anything")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {admin}"))
        .body(Body::from(serde_json::to_vec(&replacement).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);
    let on_disk: Value = serde_json::from_slice(&std::fs::read(&packument_path).unwrap()).unwrap();
    assert_eq!(on_disk["owner"], "admin");
}

/// Two different versions of the same package, published concurrently,
/// both survive: the per-package serialization guard makes each publish
/// read-merge-write atomic, so neither overwrites the other's version
/// (the lost-update the guard exists to prevent). Without the guard the
/// two publishes can read the same empty packument and the second write
/// clobbers the first version.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_publishes_of_distinct_versions_all_survive() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let publish = |version: &'static str| {
        let app = app.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let body = publish_doc("racer", version, version.as_bytes());
            let request = Request::put("/racer")
                .header("content-type", "application/json")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap();
            app.oneshot(request).await.unwrap().status()
        })
    };

    let first_publish = publish("1.0.0");
    let second_publish = publish("2.0.0");
    let (first, second) = tokio::join!(first_publish, second_publish);
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first, StatusCode::CREATED);
    assert_eq!(second, StatusCode::CREATED);

    let on_disk = std::fs::read(storage.join("racer/package.json")).expect("packument written");
    let packument: Value = serde_json::from_slice(&on_disk).unwrap();
    assert_eq!(packument["versions"]["1.0.0"]["version"], "1.0.0", "1.0.0 must survive");
    assert_eq!(packument["versions"]["2.0.0"]["version"], "2.0.0", "2.0.0 must survive");
}
