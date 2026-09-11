use super::{
    AccessList, AuthState, Body, Ecosystem, PackagePattern, PackageRule, PackageRules, Request,
    ServiceExt, StatusCode, TempDir, Value, app, app_allowing_deletes, basic, body_bytes,
    digest_of, get, header, image_manifest, json, oci_config, pausing_store, push_blob, push_image,
    router_with_auth, token,
};
use futures_util::StreamExt as _;

#[tokio::test]
async fn a_chunked_upload_resumes_from_where_it_left_off() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-0");

    let request = Request::patch(&location)
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_RANGE, "0-4")
        .body(Body::from("hello"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-4");

    let request = Request::patch(&location)
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_RANGE, "99-103")
        .body(Body::from("nope!"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);

    let request = Request::patch(&location)
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_RANGE, "5-10")
        .body(Body::from(" world"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let digest = digest_of(b"hello world");
    let request = Request::put(format!("{location}?digest={digest}"))
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = get(&app, &format!("/v2/acme/app/blobs/{digest}")).await;
    assert_eq!(body_bytes(response.into_body()).await, b"hello world".as_slice());
}

#[tokio::test]
async fn a_manifest_naming_a_blob_the_repository_lacks_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_blob(&app, &auth, "acme/app", b"config").await;

    let request = Request::put("/v2/acme/app/manifests/1.0")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
        .body(Body::from(image_manifest("config", &["layer"])))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["errors"][0]["code"], "MANIFEST_BLOB_UNKNOWN");

    let response = get(&app, "/v2/acme/app/manifests/1.0").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_upload_started_through_a_prefixed_base_continues_there() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/oci/~images/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap();
    assert!(
        location.starts_with("/oci/~images/v2/acme/app/blobs/uploads/"),
        "an upload must continue where it started, got {location}",
    );
}

#[tokio::test]
async fn concurrent_chunks_of_one_upload_neither_lose_nor_duplicate_bytes() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();

    // Requests for one upload are serialized, so whichever order these land in
    // the upload holds exactly both chunks. Without that the two appends could
    // interleave, and the digest a later PUT verified would not be the bytes
    // that got promoted.
    let patch = |chunk: &'static str| {
        let app = app.clone();
        let auth = auth.clone();
        let location = location.clone();
        async move {
            let request = Request::patch(&location)
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from(chunk))
                .unwrap();
            app.oneshot(request).await.unwrap().status()
        }
    };
    let one = patch("aaaaa");
    let two = patch("bbbbb");
    let (first, second) = tokio::join!(one, two);
    assert_eq!(first, StatusCode::ACCEPTED);
    assert_eq!(second, StatusCode::ACCEPTED);

    let request =
        Request::get(&location).header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-9");
}

#[tokio::test]
async fn deleting_a_referenced_blob_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app_allowing_deletes(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    let request = Request::delete(format!("/v2/acme/app/blobs/{}", digest_of(b"layer")))
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        get(&app, &format!("/v2/acme/app/blobs/{}", digest_of(b"layer"))).await.status(),
        StatusCode::OK,
    );
}

#[tokio::test]
async fn a_malformed_content_range_does_not_advance_an_upload() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();

    // The leading number is where the upload actually stands, so a range read
    // only up to the hyphen would accept this and write the body.
    let request = Request::patch(&location)
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_RANGE, "0-garbage")
        .body(Body::from("nope!"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let request =
        Request::get(&location).header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-0");
}

#[tokio::test]
async fn a_range_spanning_more_than_a_blob_can_hold_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();

    // One past the largest range a client can spell does not fit the number
    // that holds the span, and no chunk may be larger than a whole blob.
    for range in ["0-18446744073709551615", "0-999999999999"] {
        let request = Request::patch(&location)
            .header(header::AUTHORIZATION, &auth)
            .header(header::CONTENT_RANGE, range)
            .body(Body::from("hello"))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{range}");
    }

    let request =
        Request::get(&location).header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-0");
}

#[tokio::test]
async fn an_upload_cannot_be_finished_into_another_repository() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();
    let id = location.rsplit('/').next().unwrap().to_string();

    // The same organization, and the caller may publish to both. The id still
    // only names bytes the other repository's client sent.
    let request = Request::patch(format!("/v2/acme/other/blobs/uploads/{id}"))
        .header(header::AUTHORIZATION, &auth)
        .body(Body::from("hello"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn s3_upload_moves_between_replicas_and_keeps_an_interrupted_chunks_prefix() {
    let first_disk = TempDir::new().unwrap();
    let second_disk = TempDir::new().unwrap();
    let objects: std::sync::Arc<dyn object_store::ObjectStore> =
        std::sync::Arc::new(object_store::memory::InMemory::new());
    let auth_state = AuthState::in_memory();
    let replica = |disk: &TempDir| {
        let mut config = oci_config(disk.path().to_path_buf(), "$all");
        config.hosted_store = pnpr::HostedStoreConfig::ObjectStore {
            store: std::sync::Arc::clone(&objects),
            prefix: "shared/".into(),
        };
        router_with_auth(config, auth_state.clone())
    };
    let first = replica(&first_disk);
    let second = replica(&second_disk);
    let auth = basic(&token(&first).await);
    let response = first
        .clone()
        .oneshot(
            Request::post("/v2/acme/app/blobs/uploads/")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from("hello "))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let location = response.headers()[header::LOCATION].to_str().unwrap().to_string();
    let torn = futures_util::stream::iter([
        Ok(axum::body::Bytes::from_static(b"wor")),
        Err(std::io::Error::other("disconnected")),
    ]);
    let response = second
        .clone()
        .oneshot(
            Request::patch(&location)
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from_stream(torn))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = first
        .clone()
        .oneshot(
            Request::get(&location)
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()[header::RANGE], "0-8");
    let digest = digest_of(b"hello world");
    let response = first
        .clone()
        .oneshot(
            Request::put(format!("{location}?digest={digest}"))
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from("ld"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let response = get(&second, &format!("/v2/acme/app/blobs/{digest}")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, b"hello world");
}

#[tokio::test]
async fn mounts_and_discovery_respect_source_read_permissions() {
    let tmp = TempDir::new().unwrap();
    let auth_state = AuthState::in_memory();
    let setup = router_with_auth(oci_config(tmp.path().to_path_buf(), "$all"), auth_state.clone());
    let auth = basic(&token(&setup).await);
    let digest = push_blob(&setup, &auth, "acme/secret", b"secret layer").await;
    push_image(&setup, &auth, "acme/secret", "latest").await;
    push_image(&setup, &auth, "acme/public", "latest").await;
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.hosted.get_mut("images").unwrap().rules = PackageRules::new(
        vec![PackageRule {
            pattern: PackagePattern::parse("acme/secret", Ecosystem::Oci).unwrap(),
            access: Some(AccessList::from_tokens(["bob"])),
            publish: Some(AccessList::from_tokens(["$authenticated"])),
            unpublish: None,
        }],
        Some(AccessList::from_tokens(["$all"])),
    );
    let app = router_with_auth(config, auth_state);
    let response = app
        .clone()
        .oneshot(
            Request::post(format!(
                "/v2/acme/public/blobs/uploads/?mount={digest}&from=acme/secret",
            ))
            .header(header::AUTHORIZATION, &auth)
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        get(&app, &format!("/v2/acme/public/blobs/{digest}")).await.status(),
        StatusCode::NOT_FOUND,
    );
    let response = app
        .clone()
        .oneshot(
            Request::post(format!(
                "/v2/acme/public/blobs/uploads/?mount={digest}&from=acme/secret",
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    for path in [
        format!("/v2/acme/secret/referrers/{digest}"),
        "/v2/acme/secret/tags/list?n=1".into(),
        format!("/v2/acme/secret/blobs/{digest}"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::AUTHORIZATION, &auth)
                    .header(header::RANGE, "bytes=0-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = get(&app, "/v2/_catalog?n=1").await;
    assert!(!response.headers().contains_key(header::LINK));
    assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["repositories"], json!(["acme/public"]));
}

#[tokio::test]
async fn catalog_lists_repositories_under_published_and_blob_only_parents() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "latest").await;
    push_image(&app, &auth, "acme/app/tool", "latest").await;
    push_blob(&app, &auth, "acme/blobs", b"loose").await;
    push_image(&app, &auth, "acme/blobs/tool", "latest").await;
    let response = get(&app, "/v2/_catalog").await;
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["repositories"], json!(["acme/app", "acme/app/tool", "acme/blobs/tool"]));
}

#[tokio::test]
async fn unreferenced_blobs_can_be_deleted_and_uploaded_again() {
    let tmp = TempDir::new().unwrap();
    let app = app_allowing_deletes(&tmp);
    let auth = basic(&token(&app).await);
    let digest = push_blob(&app, &auth, "acme/app", b"config").await;
    let response = app
        .clone()
        .oneshot(
            Request::delete(format!("/v2/acme/app/blobs/{digest}"))
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        get(&app, &format!("/v2/acme/app/blobs/{digest}")).await.status(),
        StatusCode::NOT_FOUND,
    );
    push_image(&app, &auth, "acme/app", "after-delete").await;
    assert_eq!(get(&app, "/v2/acme/app/manifests/after-delete").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn configured_limits_bound_monolithic_resumable_and_manifest_uploads() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.oci.max_blob_bytes = 5;
    config.oci.max_manifest_bytes = 16;
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/v2/acme/app/blobs/uploads/?digest={}", digest_of(b"123456")))
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from("123456"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app
        .clone()
        .oneshot(
            Request::post("/v2/acme/app/blobs/uploads/")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from("123"))
                .unwrap(),
        )
        .await
        .unwrap();
    let location = response.headers()[header::LOCATION].to_str().unwrap().to_string();
    let response = app
        .clone()
        .oneshot(
            Request::patch(&location)
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from("456"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app
        .clone()
        .oneshot(
            Request::put("/v2/acme/app/manifests/latest")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from(image_manifest("", &[])))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn proxy_verifies_and_caches_manifest_and_blob_content() {
    let mut upstream = mockito::Server::new_async().await;
    let manifest = image_manifest("config", &[]);
    let manifest_mock = upstream
        .mock("GET", "/v2/other/app/manifests/latest")
        .with_header("content-type", "application/vnd.oci.image.manifest.v1+json")
        .with_header("docker-content-digest", &digest_of(&manifest))
        .with_body(manifest.clone())
        .expect(1)
        .create_async()
        .await;
    let blob_mock = upstream
        .mock("GET", format!("/v2/other/app/blobs/{}", digest_of(b"config")).as_str())
        .with_body("config")
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.upstreams.get_mut("dockerhub").unwrap().url = format!("{}/", upstream.url());
    let app = router_with_auth(config, AuthState::in_memory());
    for _ in 0..2 {
        let response = get(&app, "/v2/other/app/manifests/latest").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, manifest);
        let response = get(&app, &format!("/v2/other/app/blobs/{}", digest_of(b"config"))).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, b"config");
    }
    let response = app
        .clone()
        .oneshot(Request::head("/v2/other/app/manifests/latest").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_LENGTH], manifest.len().to_string());
    assert_eq!(response.headers()[header::CONTENT_TYPE], pnpr_oci::media_type::OCI_IMAGE_MANIFEST);
    assert_eq!(response.headers()["docker-content-digest"], digest_of(&manifest));
    assert!(body_bytes(response.into_body()).await.is_empty());
    manifest_mock.assert_async().await;
    blob_mock.assert_async().await;
}

#[tokio::test]
async fn blob_deletion_fences_a_manifest_commit_on_another_replica() {
    let tmp = TempDir::new().unwrap();
    let objects = std::sync::Arc::new(pausing_store::PausingStore::default());
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.hosted_store = pnpr::HostedStoreConfig::ObjectStore {
        store: std::sync::Arc::clone(&objects) as std::sync::Arc<dyn object_store::ObjectStore>,
        prefix: "fenced/".into(),
    };
    let hosted = config.hosted.get_mut("images").unwrap();
    hosted.rules = std::mem::take(&mut hosted.rules)
        .with_default_unpublish(AccessList::from_tokens(["$authenticated"]));
    let auth_state = AuthState::in_memory();
    let first = router_with_auth(config.clone(), auth_state.clone());
    config.cache_storage = tmp.path().join("second-cache");
    let second = router_with_auth(config, auth_state);
    let auth = basic(&token(&first).await);
    let digest = push_blob(&first, &auth, "acme/app", b"config").await;
    objects.pause_write("package.json".to_string());
    let publish = tokio::spawn(
        first.oneshot(
            Request::put("/v2/acme/app/manifests/latest")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from(image_manifest("config", &[])))
                .unwrap(),
        ),
    );
    tokio::time::timeout(std::time::Duration::from_secs(10), objects.started.notified())
        .await
        .unwrap();
    let deleted = second
        .clone()
        .oneshot(
            Request::delete(format!("/v2/acme/app/blobs/{digest}"))
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::ACCEPTED);
    objects.resume.notify_one();
    assert_eq!(publish.await.unwrap().unwrap().status(), StatusCode::CONFLICT);
    assert_eq!(get(&second, "/v2/acme/app/manifests/latest").await.status(), StatusCode::NOT_FOUND);
    push_image(&second, &auth, "acme/app", "retry").await;
}

#[tokio::test]
async fn proxy_does_not_cache_corrupt_content_or_download_blob_bodies_for_head() {
    let mut upstream = mockito::Server::new_async().await;
    let manifest = image_manifest("config", &[]);
    let corrupt_manifest = upstream
        .mock("GET", "/v2/other/app/manifests/latest")
        .with_header("docker-content-digest", &digest_of(b"different"))
        .with_body(manifest)
        .expect(2)
        .create_async()
        .await;
    let path = format!("/v2/other/app/blobs/{}", digest_of(b"config"));
    let corrupt_blob =
        upstream.mock("GET", path.as_str()).with_body("poison").expect(2).create_async().await;
    let head = upstream
        .mock("HEAD", path.as_str())
        .with_header("content-length", "6")
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.upstreams.get_mut("dockerhub").unwrap().url = format!("{}/", upstream.url());
    let app = router_with_auth(config, AuthState::in_memory());
    let response =
        app.clone().oneshot(Request::head(&path).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "6");
    assert!(body_bytes(response.into_body()).await.is_empty());
    for _ in 0..2 {
        assert_eq!(
            get(&app, "/v2/other/app/manifests/latest").await.status(),
            StatusCode::BAD_REQUEST,
        );
        let response = get(&app, &path).await;
        let mut stream = response.into_body().into_data_stream();
        let mut bytes = Vec::new();
        while let Ok(chunk) =
            stream.next().await.expect("corrupt content must terminate with an integrity error")
        {
            bytes.extend_from_slice(&chunk);
        }
        assert_eq!(bytes, b"poison");
        assert!(stream.next().await.is_none());
    }
    head.assert_async().await;
    corrupt_manifest.assert_async().await;
    corrupt_blob.assert_async().await;
}

#[tokio::test]
async fn scoped_mount_without_source_pull_permission_falls_back_to_upload() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.oci.bearer_auth = true;
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    let digest = push_blob(&app, &auth, "acme/source", b"source layer").await;
    let response = app
        .clone()
        .oneshot(
            Request::get("/v2/token?service=pnpr&scope=repository:acme/dest:push")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let scoped = format!("Bearer {}", payload["token"].as_str().unwrap());
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/v2/acme/dest/blobs/uploads/?mount={digest}&from=acme/source"))
                .header(header::AUTHORIZATION, &scoped)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let location = response.headers()[header::LOCATION].to_str().unwrap().to_string();
    let response = app
        .clone()
        .oneshot(
            Request::get(&location)
                .header(header::AUTHORIZATION, &scoped)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        get(&app, &format!("/v2/acme/dest/blobs/{digest}")).await.status(),
        StatusCode::NOT_FOUND,
    );
}
