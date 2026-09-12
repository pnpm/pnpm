use super::{
    AccessList, AuthState, Body, Ecosystem, Request, ServiceExt, StatusCode, TempDir, Value, app,
    app_allowing_deletes, basic, body_bytes, digest_of, get, header, image_manifest, json,
    oci_config, pausing_store, push_blob, push_image, repository_with_colliding_lock_keys,
    router_with_auth, strip_referrer_metadata, token,
};

#[tokio::test]
async fn a_manifest_pushed_under_the_wrong_digest_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_blob(&app, &auth, "acme/app", b"config").await;
    push_blob(&app, &auth, "acme/app", b"layer").await;

    let wrong = digest_of(b"not the manifest");
    let request = Request::put(format!("/v2/acme/app/manifests/{wrong}"))
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
        .body(Body::from(image_manifest("config", &["layer"])))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["errors"][0]["code"], "DIGEST_INVALID");
}

#[tokio::test]
async fn tags_list_in_lexical_order_and_a_moved_tag_repoints() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;
    push_image(&app, &auth, "acme/app", "latest").await;

    let response = get(&app, "/v2/acme/app/tags/list").await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["name"], "acme/app");
    assert_eq!(payload["tags"], json!(["1.0", "latest"]));

    push_blob(&app, &auth, "acme/app", b"config2").await;
    let moved = image_manifest("config2", &[]);
    let request = Request::put("/v2/acme/app/manifests/latest")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
        .body(Body::from(moved.clone()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    let response = get(&app, "/v2/acme/app/manifests/latest").await;
    assert_eq!(response.headers().get("docker-content-digest").unwrap(), &digest_of(&moved));
}

#[tokio::test]
async fn deleting_a_tag_keeps_the_manifest_and_deleting_the_manifest_drops_the_tag() {
    let tmp = TempDir::new().unwrap();
    let app = app_allowing_deletes(&tmp);
    let auth = basic(&token(&app).await);
    let manifest_digest = push_image(&app, &auth, "acme/app", "1.0").await;

    let request = Request::delete("/v2/acme/app/manifests/1.0")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::ACCEPTED);
    assert_eq!(get(&app, "/v2/acme/app/manifests/1.0").await.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        get(&app, &format!("/v2/acme/app/manifests/{manifest_digest}")).await.status(),
        StatusCode::OK,
    );

    let request = Request::delete(format!("/v2/acme/app/manifests/{manifest_digest}"))
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::ACCEPTED);
    assert_eq!(
        get(&app, &format!("/v2/acme/app/manifests/{manifest_digest}")).await.status(),
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn a_delete_is_refused_unless_the_registry_opens_destructive_writes() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    let request = Request::delete("/v2/acme/app/manifests/1.0")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(get(&app, "/v2/acme/app/manifests/1.0").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_manifest_whose_descriptor_size_is_wrong_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_blob(&app, &auth, "acme/app", b"config").await;
    push_blob(&app, &auth, "acme/app", b"layer").await;

    // The blobs are there, but the manifest lies about how long the layer is,
    // which a client that verifies descriptors would refuse to pull.
    let manifest = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": { "digest": digest_of(b"config"), "size": 6 },
        "layers": [{ "digest": digest_of(b"layer"), "size": 9999 }],
    }))
    .unwrap();
    let request = Request::put("/v2/acme/app/manifests/1.0")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
        .body(Body::from(manifest))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["errors"][0]["code"], "MANIFEST_INVALID");
}

#[tokio::test]
async fn a_reference_that_is_neither_tag_nor_digest_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    // `sha256:short` parses as neither, and storing it verbatim would leave a
    // digest-shaped entry in the tag list.
    for reference in ["sha256:short", ".leading-dot", "-leading-dash"] {
        let request = Request::put(format!("/v2/acme/app/manifests/{reference}"))
            .header(header::AUTHORIZATION, &auth)
            .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
            .body(Body::from(image_manifest("config", &["layer"])))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{reference} should not be a tag");
    }

    let response = get(&app, "/v2/acme/app/tags/list").await;
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["tags"], json!(["1.0"]));
}

#[tokio::test]
async fn a_manifest_repeating_one_descriptor_is_not_thousands_of_lookups() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_blob(&app, &auth, "acme/app", b"config").await;
    push_blob(&app, &auth, "acme/app", b"layer").await;

    // One digest repeated far past any real image. It is looked up once, so
    // this is accepted rather than turned into a lookup per entry.
    let layer = json!({ "digest": digest_of(b"layer"), "size": 5 });
    let manifest = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": { "digest": digest_of(b"config"), "size": 6 },
        "layers": vec![layer; 20_000],
    }))
    .unwrap();
    let request = Request::put("/v2/acme/app/manifests/1.0")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
        .body(Body::from(manifest))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn an_index_child_must_be_a_manifest_this_repository_serves() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    // A layer blob is present, but it is not a manifest. An index naming it
    // would publish a child that a client asking for it cannot pull.
    push_blob(&app, &auth, "acme/app", b"layer").await;

    let index = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [{ "digest": digest_of(b"layer"), "size": 5 }],
    }))
    .unwrap();
    let request = Request::put("/v2/acme/app/manifests/multi")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.index.v1+json")
        .body(Body::from(index))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(get(&app, "/v2/acme/app/manifests/multi").await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn referrers_backfill_preexisting_manifests_on_both_backends() {
    for hosted_store in [
        pnpr::HostedStoreConfig::Fs,
        pnpr::HostedStoreConfig::ObjectStore {
            store: std::sync::Arc::new(object_store::memory::InMemory::new()),
            prefix: "legacy/".into(),
        },
    ] {
        let tmp = TempDir::new().unwrap();
        let mut config = oci_config(tmp.path().to_path_buf(), "$all");
        config.hosted_store = hosted_store;
        let storage = pnpr_storage::Storage::new(
            &config.hosted_store,
            config.storage.clone(),
            config.cache_storage.clone(),
        )
        .unwrap()
        .for_hosted("images");
        let app = router_with_auth(config, AuthState::in_memory());
        let auth = basic(&token(&app).await);
        let repository = repository_with_colliding_lock_keys();
        let subject = digest_of(b"subject");
        let manifest = json!({ "schemaVersion": 2, "mediaType": pnpr_oci::media_type::OCI_IMAGE_INDEX,
            "manifests": [], "subject": { "digest": subject, "size": 7 }, "artifactType": "application/example.sbom" });
        let response = app
            .clone()
            .oneshot(
                Request::put(format!("/v2/{repository}/manifests/sbom"))
                    .header(header::AUTHORIZATION, &auth)
                    .body(Body::from(manifest.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let key =
            pnpr_package_name::CanonicalPackageName::parse(&repository, Ecosystem::Oci).unwrap();
        let mut document: Value =
            serde_json::from_slice(&storage.read_hosted_document(&key).await.unwrap().unwrap())
                .unwrap();
        strip_referrer_metadata(&mut document, 1);
        storage
            .update_hosted_document_with_retry(&key, 1, |_| {
                Ok(Some(serde_json::to_vec(&document).unwrap()))
            })
            .await
            .unwrap();
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            get(&app, &format!("/v2/{repository}/referrers/{subject}")),
        )
        .await
        .expect("migration must not acquire the same stripe twice");
        assert_eq!(response.status(), StatusCode::OK);
        let payload: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(payload["manifests"].as_array().unwrap().len(), 1);
        assert_eq!(payload["manifests"][0]["artifactType"], "application/example.sbom");
        let document = pnpr_oci::ImageDocument::parse(
            &storage.read_hosted_document(&key).await.unwrap().unwrap(),
        )
        .unwrap();
        assert_eq!(
            document.manifests()[0]
                .referrer
                .as_ref()
                .unwrap()
                .subject
                .as_ref()
                .unwrap()
                .to_string(),
            subject,
        );
    }
}

#[tokio::test]
async fn referrer_migration_does_not_block_writers_or_restore_deleted_manifests() {
    let tmp = TempDir::new().unwrap();
    let objects = std::sync::Arc::new(pausing_store::PausingStore::default());
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.hosted_store = pnpr::HostedStoreConfig::ObjectStore {
        store: std::sync::Arc::<pausing_store::PausingStore>::clone(&objects),
        prefix: "migration/".into(),
    };
    let hosted = config.hosted.get_mut("images").unwrap();
    hosted.rules = std::mem::take(&mut hosted.rules)
        .with_default_unpublish(AccessList::from_tokens(["$authenticated"]));
    let storage = pnpr_storage::Storage::new(
        &config.hosted_store,
        config.storage.clone(),
        config.cache_storage.clone(),
    )
    .unwrap()
    .for_hosted("images");
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    let subject = digest_of(b"subject");
    let manifest = json!({ "schemaVersion": 2, "mediaType": pnpr_oci::media_type::OCI_IMAGE_INDEX,
        "manifests": [], "subject": { "digest": subject, "size": 7 } })
    .to_string();
    let digest = pnpr_oci::Digest::of(manifest.as_bytes());
    let response = app
        .clone()
        .oneshot(
            Request::put("/v2/acme/migration/manifests/sbom")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from(manifest))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let key =
        pnpr_package_name::CanonicalPackageName::parse("acme/migration", Ecosystem::Oci).unwrap();
    let mut document: Value =
        serde_json::from_slice(&storage.read_hosted_document(&key).await.unwrap().unwrap())
            .unwrap();
    strip_referrer_metadata(&mut document, 1);
    storage
        .update_hosted_document_with_retry(&key, 1, |_| {
            Ok(Some(serde_json::to_vec(&document).unwrap()))
        })
        .await
        .unwrap();
    objects.pause(digest.blob_filename());
    let reader = app.clone();
    let path = format!("/v2/acme/migration/referrers/{subject}");
    let read = tokio::spawn(async move { get(&reader, &path).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), objects.started.notified())
        .await
        .unwrap();
    let update = async {
        let published = push_image(&app, &auth, "acme/migration", "fresh").await;
        let deleted = app
            .clone()
            .oneshot(
                Request::delete(format!("/v2/acme/migration/manifests/{digest}"))
                    .header(header::AUTHORIZATION, &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(deleted.status(), StatusCode::ACCEPTED);
        published
    };
    let published = tokio::time::timeout(std::time::Duration::from_secs(5), update).await;
    objects.resume.notify_one();
    let published = published.expect("manifest reads must not hold the package writer lock");
    let response =
        tokio::time::timeout(std::time::Duration::from_secs(5), read).await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let document =
        pnpr_oci::ImageDocument::parse(&storage.read_hosted_document(&key).await.unwrap().unwrap())
            .unwrap();
    assert!(document.manifest(&digest).is_none());
    assert_eq!(document.resolve("fresh").unwrap().digest.to_string(), published);
}

#[tokio::test]
async fn batch_publishes_an_oci_manifest_and_rolls_back_on_invalid_siblings() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_blob(&app, &auth, "acme/app", b"config").await;
    let entry = json!({ "ecosystem": "oci", "name": "acme/app", "reference": "release", "manifest": STANDARD.encode(image_manifest("config", &[])) });
    let invalid = json!({ "ecosystem": "oci", "name": "acme/missing", "reference": "release", "manifest": STANDARD.encode(image_manifest("missing", &[])) });
    let response = app
        .clone()
        .oneshot(
            Request::put("/-/pnpr/v0/publish")
                .header(header::AUTHORIZATION, &auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "packages": [entry.clone(), invalid] }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(get(&app, "/v2/acme/app/manifests/release").await.status(), StatusCode::NOT_FOUND);
    let response = app
        .clone()
        .oneshot(
            Request::put("/-/pnpr/v0/publish")
                .header(header::AUTHORIZATION, &auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "packages": [entry] }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(get(&app, "/v2/acme/app/manifests/release").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn cold_manifest_head_forwards_headers_without_getting_or_caching_a_body() {
    for cache in [true, false] {
        let mut upstream = mockito::Server::new_async().await;
        let digest = digest_of(b"manifest");
        let head = upstream
            .mock("HEAD", "/v2/other/app/manifests/latest")
            .with_header("content-length", "123")
            .with_header("content-type", pnpr_oci::media_type::OCI_IMAGE_MANIFEST)
            .with_header("docker-content-digest", &digest)
            .expect(2)
            .create_async()
            .await;
        let tmp = TempDir::new().unwrap();
        let mut config = oci_config(tmp.path().to_path_buf(), "$all");
        let source = config.upstreams.get_mut("dockerhub").unwrap();
        source.url = format!("{}/", upstream.url());
        source.cache = cache;
        let app = router_with_auth(config, AuthState::in_memory());
        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(
                    Request::head("/v2/other/app/manifests/latest").body(Body::empty()).unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_LENGTH], "123");
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                pnpr_oci::media_type::OCI_IMAGE_MANIFEST,
            );
            assert_eq!(response.headers()["docker-content-digest"], digest);
            assert!(body_bytes(response.into_body()).await.is_empty());
        }
        head.assert_async().await;
    }
}

#[tokio::test]
async fn manifest_head_checks_digests_and_verifies_legacy_responses_without_a_header() {
    let manifest = image_manifest("config", &[]);
    let digest = digest_of(&manifest);
    let wrong = digest_of(b"different manifest");
    for (reference, declared, valid_body, expected) in [
        (digest.as_str(), Some(digest.as_str()), true, StatusCode::OK),
        (digest.as_str(), Some(wrong.as_str()), true, StatusCode::BAD_REQUEST),
        (digest.as_str(), Some("invalid"), true, StatusCode::BAD_REQUEST),
        (digest.as_str(), None, true, StatusCode::OK),
        (digest.as_str(), None, false, StatusCode::BAD_REQUEST),
        ("latest", Some(digest.as_str()), true, StatusCode::OK),
        ("latest", Some("invalid"), true, StatusCode::BAD_REQUEST),
    ] {
        let mut upstream = mockito::Server::new_async().await;
        let path = format!("/v2/other/app/manifests/{reference}");
        let mut head = upstream.mock("HEAD", path.as_str()).expect(1);
        if let Some(declared) = declared {
            head = head.with_header("docker-content-digest", declared);
        }
        let head = head.create_async().await;
        let get = upstream
            .mock("GET", path.as_str())
            .with_body(if valid_body { manifest.as_slice() } else { b"corrupt" })
            .expect(usize::from(declared.is_none()))
            .create_async()
            .await;
        let tmp = TempDir::new().unwrap();
        let mut config = oci_config(tmp.path().to_path_buf(), "$all");
        config.upstreams.get_mut("dockerhub").unwrap().url = format!("{}/", upstream.url());
        let app = router_with_auth(config, AuthState::in_memory());
        let response = app.oneshot(Request::head(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), expected, "declared digest: {declared:?}");
        if expected == StatusCode::OK {
            assert_eq!(response.headers()["docker-content-digest"], digest);
            assert!(body_bytes(response.into_body()).await.is_empty());
        }
        head.assert_async().await;
        get.assert_async().await;
    }
}
