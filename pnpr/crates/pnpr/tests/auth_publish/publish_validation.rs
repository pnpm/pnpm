use super::{
    BASE64, Body, Request, ServiceExt, StatusCode, TempDir, Value, add_user_and_get_token,
    body_bytes, body_json, json, publish_doc, put_json, router, sha1_hex, sri_sha512,
    static_config,
};
use base64::Engine;

#[tokio::test]
async fn anonymous_publish_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let body = publish_doc("anon-test", "1.0.0", b"tarball-bytes");
    let response = app.oneshot(put_json("/anon-test", body)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn republishing_an_existing_version_is_rejected_with_conflict() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let publish = |bytes: &[u8]| {
        let body = serde_json::to_vec(&publish_doc("mypkg", "1.0.0", bytes)).unwrap();
        Request::put("/mypkg")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::from(body))
            .unwrap()
    };

    let first = app.clone().oneshot(publish(b"original-bytes")).await.unwrap();
    assert_eq!(first.status(), StatusCode::CREATED);

    // Re-publishing the same version with different bytes must be refused.
    let second = app.clone().oneshot(publish(b"backdoored-bytes")).await.unwrap();
    assert_eq!(second.status(), StatusCode::CONFLICT);

    // The originally published tarball must be untouched on disk.
    let on_disk = std::fs::read(storage.join("mypkg/mypkg-1.0.0.tgz")).unwrap();
    assert_eq!(on_disk, b"original-bytes");
}

#[tokio::test]
async fn republish_via_a_smuggled_version_entry_without_an_attachment_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    // Publish 1.0.0 normally.
    let first = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&publish_doc("mypkg", "1.0.0", b"original")).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(first).await.unwrap().status(), StatusCode::CREATED);

    // Publish 1.0.1 (with its own attachment) but smuggle a modified 1.0.0
    // entry into `versions` with no matching attachment. The conflict check
    // must still reject it because 1.0.0 is already hosted.
    let mut body = publish_doc("mypkg", "1.0.1", b"new-bytes");
    body["versions"]["1.0.0"] = json!({
        "name": "mypkg",
        "version": "1.0.0",
        "dist": { "tarball": "http://example.test/mypkg/-/mypkg-1.0.0.tgz", "integrity": "sha512-EVIL" },
    });
    let smuggle = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.oneshot(smuggle).await.unwrap().status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn update_packument_rejects_tampering_with_a_published_version_integrity() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let publish = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::to_vec(&publish_doc("mypkg", "1.0.0", b"real-bytes")).unwrap(),
        ))
        .unwrap();
    assert_eq!(app.clone().oneshot(publish).await.unwrap().status(), StatusCode::CREATED);

    let get =
        app.clone().oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap();
    let mut packument = body_json(get.into_body()).await;
    let original_integrity = packument["versions"]["1.0.0"]["dist"]["integrity"].clone();
    assert!(original_integrity.is_string(), "published version should carry an integrity");

    // Point the already-published version at a bogus integrity and PUT it back
    // through the partial-unpublish endpoint.
    packument["versions"]["1.0.0"]["dist"]["integrity"] = json!(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
    );
    let tamper = Request::put("/mypkg/-rev/1-0")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&packument).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(tamper).await.unwrap().status(), StatusCode::BAD_REQUEST);

    let after = app.oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap();
    let after = body_json(after.into_body()).await;
    assert_eq!(after["versions"]["1.0.0"]["dist"]["integrity"], original_integrity);
}

#[tokio::test]
async fn update_packument_rejects_a_non_object_dist_for_a_published_version() {
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
    let original_integrity = packument["versions"]["1.0.0"]["dist"]["integrity"].clone();
    // A non-object dist would skip the integrity-restore path and strip the hash,
    // so it must be rejected rather than silently persisted.
    packument["versions"]["1.0.0"]["dist"] = json!(null);
    let request = Request::put("/mypkg/-rev/1-0")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&packument).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::BAD_REQUEST);

    let after = app.oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap();
    let after = body_json(after.into_body()).await;
    assert_eq!(after["versions"]["1.0.0"]["dist"]["integrity"], original_integrity);
}

#[tokio::test]
async fn update_packument_rejects_tampering_with_a_published_version_tarball() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let publish = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::to_vec(&publish_doc("mypkg", "1.0.0", b"real-bytes")).unwrap(),
        ))
        .unwrap();
    assert_eq!(app.clone().oneshot(publish).await.unwrap().status(), StatusCode::CREATED);

    let get =
        app.clone().oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap();
    let mut packument = body_json(get.into_body()).await;
    let original_tarball = packument["versions"]["1.0.0"]["dist"]["tarball"].clone();

    // Repoint the published version at a different tarball basename. The version
    // keeps its immutable integrity, so this would make clients fetch bytes that
    // can't match it (or 404) — it must be rejected, not persisted.
    packument["versions"]["1.0.0"]["dist"]["tarball"] =
        json!("http://example.test/mypkg/-/mypkg-9.9.9.tgz");
    let tamper = Request::put("/mypkg/-rev/1-0")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&packument).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(tamper).await.unwrap().status(), StatusCode::BAD_REQUEST);

    let after = app.oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap();
    let after = body_json(after.into_body()).await;
    assert_eq!(after["versions"]["1.0.0"]["dist"]["tarball"], original_tarball);
}

#[tokio::test]
async fn update_packument_rejects_adding_a_version_via_the_unpublish_put() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let publish = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::to_vec(&publish_doc("mypkg", "1.0.0", b"real-bytes")).unwrap(),
        ))
        .unwrap();
    assert_eq!(app.clone().oneshot(publish).await.unwrap().status(), StatusCode::CREATED);

    let get =
        app.clone().oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap();
    let mut packument = body_json(get.into_body()).await;
    let original_versions = packument["versions"].clone();

    // Add a new version whose tarball basename collides with 1.0.0's. A duplicate
    // basename makes expected_tarball_dist fail closed (502), so the addition must
    // be rejected — even with an otherwise-valid integrity.
    packument["versions"]["9.9.9"] = json!({
        "name": "mypkg",
        "version": "9.9.9",
        "dist": {
            "tarball": "http://example.test/mypkg/-/mypkg-1.0.0.tgz",
            "integrity": packument["versions"]["1.0.0"]["dist"]["integrity"].clone(),
        },
    });
    let request = Request::put("/mypkg/-rev/1-0")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&packument).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::BAD_REQUEST);

    let after = app.oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap();
    let after = body_json(after.into_body()).await;
    assert_eq!(after["versions"], original_versions);
}

#[tokio::test]
async fn update_packument_protects_a_published_tarball_with_a_basenameless_url() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let publish = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::to_vec(&publish_doc("mypkg", "1.0.0", b"real-bytes")).unwrap(),
        ))
        .unwrap();
    assert_eq!(app.clone().oneshot(publish).await.unwrap().status(), StatusCode::CREATED);

    // Force the stored dist.tarball to a basename-less URL (ends in `/`), which
    // rewrite_tarball_urls serves under the version-derived canonical name — so
    // the served basename is still well-defined and must stay pinned.
    let packument_path = storage.join("mypkg/package.json");
    let mut stored: Value =
        serde_json::from_slice(&std::fs::read(&packument_path).unwrap()).unwrap();
    stored["versions"]["1.0.0"]["dist"]["tarball"] = json!("http://localhost:4873/mypkg/-/");
    std::fs::write(&packument_path, serde_json::to_vec(&stored).unwrap()).unwrap();

    let mut tampered = stored.clone();
    tampered["versions"]["1.0.0"]["dist"]["tarball"] =
        json!("http://example.test/mypkg/-/mypkg-9.9.9.tgz");
    let request = Request::put("/mypkg/-rev/1-0")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&tampered).unwrap()))
        .unwrap();
    assert_eq!(app.oneshot(request).await.unwrap().status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn update_packument_rejects_seeding_a_package_with_no_published_packument() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    // PUT to the unpublish route with no prior publish would seed an authoritative
    // version that publish can never overwrite, so it must be rejected.
    let body = publish_doc("ghost", "1.0.0", b"bytes");
    let request = Request::put("/ghost/-rev/anything")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.oneshot(request).await.unwrap().status(), StatusCode::BAD_REQUEST);

    assert!(!storage.join("ghost/package.json").exists());
}

#[tokio::test]
async fn metadata_only_republish_cannot_mutate_resolution_metadata() {
    // A metadata-only re-PUT (no attachment, unchanged integrity) is allowed
    // for `deprecated`, but must not be able to rewrite resolution-relevant
    // fields of an already-hosted version — that would change what clients
    // install without changing the tarball.
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let mut publish_body = publish_doc("mypkg", "1.0.0", b"original");
    publish_body["versions"]["1.0.0"]["dependencies"] = json!({ "lodash": "^4.0.0" });
    let first = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&publish_body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(first).await.unwrap().status(), StatusCode::CREATED);

    let hosted = body_json(
        app.clone()
            .oneshot(Request::get("/mypkg").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .into_body(),
    )
    .await;
    let body = json!({
        "_id": "mypkg",
        "name": "mypkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "dependencies": { "left-pad": "1.0.0" },
                "dist": hosted["versions"]["1.0.0"]["dist"].clone(),
            }
        }
    });
    let tamper = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    // The PUT is accepted (integrity unchanged) but the dependency change is
    // not applied: the published version's metadata is immutable.
    assert_eq!(app.clone().oneshot(tamper).await.unwrap().status(), StatusCode::CREATED);
    let after = body_json(
        app.oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap().into_body(),
    )
    .await;
    assert_eq!(after["versions"]["1.0.0"]["dependencies"], json!({ "lodash": "^4.0.0" }));
}

/// Published packages are the source of truth: they live in the
/// authoritative `storage` root, never in the disposable proxy cache,
/// and survive a full wipe of that cache.
#[tokio::test]
async fn published_package_survives_wiping_the_proxy_cache() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"durable-tarball-bytes";
    let body = publish_doc("durable-pkg", "1.0.0", bytes);
    let request = Request::put("/durable-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    // The artifacts land in the authoritative root, not the cache.
    assert!(storage.join("durable-pkg/package.json").exists());
    assert!(storage.join("durable-pkg/durable-pkg-1.0.0.tgz").exists());
    assert!(
        !storage.join(".pnpr-cache/durable-pkg").exists(),
        "published package must not be written into the disposable proxy cache",
    );

    // Blow away the entire proxy cache, the way an operator reclaiming
    // disk (or a fresh container on an ephemeral cache volume) would.
    let cache_root = storage.join(".pnpr-cache");
    std::fs::create_dir_all(&cache_root).unwrap();
    std::fs::remove_dir_all(&cache_root).unwrap();

    // The package is still served, tarball and all.
    let response = app
        .clone()
        .oneshot(Request::get("/durable-pkg").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let served = body_json(response.into_body()).await;
    assert_eq!(served["versions"]["1.0.0"]["version"], "1.0.0");

    let response = app
        .oneshot(Request::get("/durable-pkg/-/durable-pkg-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
}

#[tokio::test]
async fn publish_followed_by_dist_tag_set_works() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    // First publish 1.0.0
    let body = publish_doc("tagpkg", "1.0.0", b"v1");
    let request = Request::put("/tagpkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap();

    // Then publish 2.0.0 (without changing latest)
    let mut body = publish_doc("tagpkg", "2.0.0", b"v2");
    body["dist-tags"] = json!({}); // don't bump latest
    let request = Request::put("/tagpkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap();

    // Confirm latest is still 1.0.0.
    let tags = app
        .clone()
        .oneshot(Request::get("/-/package/tagpkg/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(tags.status(), StatusCode::OK);
    let tags_body = body_json(tags.into_body()).await;
    assert_eq!(tags_body["latest"], "1.0.0");

    // PUT a new "beta" tag pointing at 2.0.0.
    let request = Request::put("/-/package/tagpkg/dist-tags/beta")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_string("2.0.0").unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // Confirm the tag landed.
    let tags = app
        .clone()
        .oneshot(Request::get("/-/package/tagpkg/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let tags_body = body_json(tags.into_body()).await;
    assert_eq!(tags_body["beta"], "2.0.0");
    assert_eq!(tags_body["latest"], "1.0.0");

    // And via the version-manifest endpoint resolving the tag.
    let manifest = app
        .clone()
        .oneshot(Request::get("/tagpkg/beta").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let manifest_body = body_json(manifest.into_body()).await;
    assert_eq!(manifest_body["version"], "2.0.0");

    // Now DELETE it.
    let request = Request::delete("/-/package/tagpkg/dist-tags/beta")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let tags = app
        .oneshot(Request::get("/-/package/tagpkg/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let tags_body = body_json(tags.into_body()).await;
    assert!(tags_body.get("beta").is_none(), "beta tag should be removed");
}

#[tokio::test]
async fn dist_tag_mutations_refresh_time_modified() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let body = publish_doc("time-mod-pkg", "1.0.0", b"x");
    let request = Request::put("/time-mod-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap();

    let initial_time = serde_json::from_slice::<Value>(
        &std::fs::read(storage.join("time-mod-pkg/package.json")).unwrap(),
    )
    .unwrap()["time"]["modified"]
        .as_str()
        .expect("modified is a string")
        .to_string();

    // Wait long enough that ISO-millisecond timestamps will differ.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    let request = Request::put("/-/package/time-mod-pkg/dist-tags/next")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_string("1.0.0").unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let after_set = serde_json::from_slice::<Value>(
        &std::fs::read(storage.join("time-mod-pkg/package.json")).unwrap(),
    )
    .unwrap()["time"]["modified"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(initial_time, after_set, "dist-tag PUT should bump time.modified");

    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    let request = Request::delete("/-/package/time-mod-pkg/dist-tags/next")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let after_delete = serde_json::from_slice::<Value>(
        &std::fs::read(storage.join("time-mod-pkg/package.json")).unwrap(),
    )
    .unwrap()["time"]["modified"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(after_set, after_delete, "dist-tag DELETE should bump time.modified too");
}

#[tokio::test]
async fn publish_rejects_body_name_that_doesnt_match_url() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let body = publish_doc("other-name", "1.0.0", b"x");
    let request = Request::put("/url-name")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn publish_rejects_tarball_that_doesnt_match_package() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"tarball-bytes";
    let mut body = publish_doc("foo", "1.0.0", bytes);
    // Override _attachments to use a filename for a different package
    body["_attachments"] = json!({
        "bar-1.0.0.tgz": {
            "content_type": "application/octet-stream",
            "data": BASE64.encode(bytes),
            "length": bytes.len()
        }
    });
    let request = Request::put("/foo")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn publish_rejects_integrity_mismatch_and_leaves_no_artifacts() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"actual-bytes";
    let mut body = publish_doc("bad-pkg", "1.0.0", bytes);
    // Swap the integrity for one computed over different bytes — the
    // body keeps the original bytes, so the server's recomputed hash
    // won't match.
    body["versions"]["1.0.0"]["dist"]["integrity"] = json!(sri_sha512(b"different-bytes"));

    let request = Request::put("/bad-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body_text = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(
        body_text.contains("EINTEGRITY"),
        "error body should carry EINTEGRITY code: {body_text}",
    );

    // Neither the packument nor the tarball should have been written.
    assert!(
        !storage.join("bad-pkg/package.json").exists(),
        "packument must not be written when integrity check fails",
    );
    assert!(
        !storage.join("bad-pkg/bad-pkg-1.0.0.tgz").exists(),
        "tarball must not be written when integrity check fails",
    );
}

#[tokio::test]
async fn publish_rejects_shasum_mismatch() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"shasum-test-bytes";
    let mut body = publish_doc("shasum-pkg", "1.0.0", bytes);
    // Keep integrity valid but corrupt the legacy shasum.
    body["versions"]["1.0.0"]["dist"]["shasum"] = json!("0000000000000000000000000000000000000000");

    let request = Request::put("/shasum-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body_text = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(
        body_text.contains("EINTEGRITY"),
        "shasum mismatch must surface EINTEGRITY: {body_text}",
    );
    assert!(!storage.join("shasum-pkg/package.json").exists());
}

#[tokio::test]
async fn publish_rejects_missing_integrity_field() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"no-integrity-bytes";
    let mut body = publish_doc("no-int-pkg", "1.0.0", bytes);
    body["versions"]["1.0.0"]["dist"].as_object_mut().unwrap().remove("integrity");

    let request = Request::put("/no-int-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body_text = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(
        body_text.contains("EINTEGRITY") && body_text.contains("integrity"),
        "missing integrity must surface a clear EINTEGRITY message: {body_text}",
    );
    assert!(!storage.join("no-int-pkg/package.json").exists());
}

/// Real npm clients send one attachment per publish, but the
/// handler is written to iterate N: if the M-th attachment fails
/// integrity, every already-written tmp file from the earlier
/// attachments must be cleaned up so a rejected publish leaves no
/// on-disk artifact. This pins down the `cleanup_tmp_slots` path —
/// without it, a regression that no-op'd the cleanup would leak
/// `*.tmp.*` files for every successful attachment before the bad
/// one.
#[tokio::test]
async fn publish_with_failed_attachment_cleans_up_earlier_tmp_files() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes_v1 = b"valid-first-attachment";
    let bytes_v2 = b"second-attachment";
    let body = json!({
        "_id": "multi-attach",
        "name": "multi-attach",
        "dist-tags": { "latest": "2.0.0" },
        "versions": {
            "1.0.0": {
                "name": "multi-attach", "version": "1.0.0",
                "dist": {
                    "tarball": "http://localhost:4873/multi-attach/-/multi-attach-1.0.0.tgz",
                    "shasum": sha1_hex(bytes_v1),
                    "integrity": sri_sha512(bytes_v1),
                }
            },
            "2.0.0": {
                "name": "multi-attach", "version": "2.0.0",
                "dist": {
                    "tarball": "http://localhost:4873/multi-attach/-/multi-attach-2.0.0.tgz",
                    "shasum": sha1_hex(bytes_v2),
                    "integrity": sri_sha512(b"WRONG-BYTES-FOR-2.0.0"),
                }
            }
        },
        "_attachments": {
            "multi-attach-1.0.0.tgz": {
                "content_type": "application/octet-stream",
                "data": BASE64.encode(bytes_v1),
                "length": bytes_v1.len(),
            },
            "multi-attach-2.0.0.tgz": {
                "content_type": "application/octet-stream",
                "data": BASE64.encode(bytes_v2),
                "length": bytes_v2.len(),
            },
        }
    });
    let request = Request::put("/multi-attach")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // The package dir may exist (the handler creates it while
    // reserving paths) but it must be empty: no .tgz files, no
    // .tmp.* leftovers, no package.json.
    let pkg_dir = storage.join("multi-attach");
    if pkg_dir.exists() {
        let entries: Vec<String> = std::fs::read_dir(&pkg_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            entries.is_empty(),
            "expected no artifacts after rejected publish, found: {entries:?}",
        );
    }
}

/// `anonymous-npm-registry-client`'s `distTags.add` URL-encodes the
/// `/` in scoped names: `@scope/pkg` → `@scope%2Fpkg` in the URL.
/// axum's `Path` extractor percent-decodes path segments, so the
/// handler sees the decoded value verbatim and never needs to decode
/// again. This regression test pins that down: if someone reintroduces
/// a manual `urldecode` (which was previously here and was both
/// redundant and buggy on literal `%` chars), the `@scope/pkg`
/// `CanonicalPackageName::parse` would still pass but a future bug-fix that
/// changes the decoder could break percent-encoded scoped paths.
#[tokio::test]
async fn dist_tag_set_works_with_url_encoded_scoped_path() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    // Publish @scope/pkg@1.0.0 (literal slash in the URL — pnpm
    // publish uses this form), then set a dist-tag using the
    // npm-client-style `%2F` encoding to verify the decoded path
    // reaches the handler.
    let body = publish_doc("@scope/pkg", "1.0.0", b"v1");
    let request = Request::put("/@scope/pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap();

    let request = Request::put("/-/package/@scope%2Fpkg/dist-tags/beta")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_string("1.0.0").unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // Fetch via the encoded form too — both should round-trip cleanly.
    let tags = app
        .oneshot(Request::get("/-/package/@scope%2Fpkg/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(tags.status(), StatusCode::OK);
    let tags_body = body_json(tags.into_body()).await;
    assert_eq!(tags_body["beta"], "1.0.0");
    assert_eq!(tags_body["latest"], "1.0.0");
}

#[tokio::test]
async fn publish_supports_scoped_packages() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"scoped-tarball";
    let body = publish_doc("@scope/pkg", "1.0.0", bytes);
    let request = Request::put("/@scope/pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    assert!(storage.join("@scope/pkg/package.json").exists());
    assert!(storage.join("@scope/pkg/pkg-1.0.0.tgz").exists());

    // And we can read it back.
    let response =
        app.oneshot(Request::get("/@scope/pkg").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
