use super::{
    AccessList,
    AuthState,
    Body,
    Request,
    Router,
    ServiceExt,
    StatusCode,
    TempDir,
    Value,
    body_bytes,
    digest_of,
    get,
    header,
    oci_config,
    pausing_store,
    push_blob,
    push_image,
    router_with_auth,
};

fn hosted_backends() -> [pnpr::HostedStoreConfig; 2] {
    [
        pnpr::HostedStoreConfig::Fs,
        pnpr::HostedStoreConfig::ObjectStore {
            store: std::sync::Arc::new(object_store::memory::InMemory::new()),
            prefix: "uploads-auth/".into(),
        },
    ]
}

async fn two_user_app(
    tmp: &TempDir,
    hosted_backend: pnpr::HostedStoreConfig,
    default_publish: Option<AccessList>,
) -> (Router, String, String) {
    use base64::{
        Engine as _,
        engine::general_purpose::STANDARD,
    };

    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.storage.hosted_backend = hosted_backend;
    if let Some(publish) = default_publish {
        let hosted = config.routing.hosted.get_mut("images").unwrap();
        hosted.rules = std::mem::take(&mut hosted.rules).with_default_publish(publish);
    }
    let auth = AuthState::in_memory();
    let alice = crate::tokens::issue_token(&auth, "alice").await;
    let bob = crate::tokens::issue_token(&auth, "bob").await;
    let app = router_with_auth(config, auth);
    let alice_auth = format!("Basic {}", STANDARD.encode(format!("alice:{alice}")));
    let bob_auth = format!("Basic {}", STANDARD.encode(format!("bob:{bob}")));
    (app, alice_auth, bob_auth)
}

async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    auth: Option<&str>,
    body: &'static [u8],
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(caller) = auth {
        builder = builder.header(header::AUTHORIZATION, caller);
    }
    app.clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap()
}

async fn open_upload(
    app: &Router,
    repository: &str,
    auth: Option<&str>,
    chunk: &'static [u8],
) -> String {
    let response =
        send(app, "POST", &format!("/v2/{repository}/blobs/uploads/"), auth, chunk).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string()
}

async fn assert_all_continuation_verbs_refused(
    app: &Router,
    location: &str,
    auth: Option<&str>,
    payload: &'static [u8],
    expected_status: StatusCode,
    expected_code: &str,
) {
    let digest = digest_of(payload);
    let put_uri = format!("{location}?digest={digest}");
    for (method, uri, body) in [
        ("GET", location, b"".as_slice()),
        ("PATCH", location, payload),
        ("PUT", put_uri.as_str(), payload),
        ("DELETE", location, b"".as_slice()),
    ] {
        let response = send(app, method, uri, auth, body).await;
        assert_eq!(response.status(), expected_status, "{method} {uri}");
        let parsed: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(parsed["errors"][0]["code"], expected_code, "{method} {uri}");
    }
}

#[tokio::test]
async fn non_owner_cannot_hijack_corrupt_complete_or_abort_owners_upload_session() {
    for backend in hosted_backends() {
        let tmp = TempDir::new().unwrap();
        let (app, alice, bob) = two_user_app(&tmp, backend, None).await;
        push_image(&app, &alice, "acme/app", "1.0").await;

        assert_eq!(
            send(&app, "POST", "/v2/acme/app/blobs/uploads/", Some(&bob), b"")
                .await
                .status(),
            StatusCode::FORBIDDEN,
        );

        let location = open_upload(&app, "acme/app", Some(&alice), b"").await;
        let patched = send(&app, "PATCH", &location, Some(&alice), b"alice-").await;
        assert_eq!(patched.status(), StatusCode::ACCEPTED);
        assert_eq!(patched.headers()[header::RANGE], "0-5");

        let hijack_payload = b"UNEXPECTED_BLOB_IN_ALICE_REPO";
        assert_all_continuation_verbs_refused(
            &app,
            &location,
            Some(&bob),
            hijack_payload,
            StatusCode::FORBIDDEN,
            "DENIED",
        )
        .await;

        let progress = send(&app, "GET", &location, Some(&alice), b"").await;
        assert_eq!(progress.status(), StatusCode::ACCEPTED);
        assert_eq!(progress.headers()[header::RANGE], "0-5");

        let alice_digest = digest_of(b"alice-blob");
        let completed =
            send(&app, "PUT", &format!("{location}?digest={alice_digest}"), Some(&alice), b"blob")
                .await;
        assert_eq!(completed.status(), StatusCode::CREATED);

        let fetched = get(&app, &format!("/v2/acme/app/blobs/{alice_digest}")).await;
        assert_eq!(fetched.status(), StatusCode::OK);
        assert_eq!(body_bytes(fetched.into_body()).await, b"alice-blob".as_slice());

        let hijack_digest = digest_of(hijack_payload);
        assert_eq!(
            get(&app, &format!("/v2/acme/app/blobs/{hijack_digest}")).await.status(),
            StatusCode::NOT_FOUND,
        );

        let cancel_location = open_upload(&app, "acme/app", Some(&alice), b"discard").await;
        assert_eq!(
            send(&app, "DELETE", &cancel_location, Some(&alice), b"").await.status(),
            StatusCode::NO_CONTENT,
        );
    }
}

#[tokio::test]
async fn co_owners_and_anonymous_publishers_cannot_use_each_others_upload_sessions() {
    for backend in hosted_backends() {
        let tmp = TempDir::new().unwrap();
        let (app, alice, bob) = two_user_app(&tmp, backend, None).await;
        push_image(&app, &alice, "acme/app", "1.0").await;

        let share = Request::put("/oci/owners/acme/app")
            .header(header::AUTHORIZATION, &alice)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"users":["bob"]}"#))
            .unwrap();
        assert_eq!(
            app.clone()
                .oneshot(share)
                .await
                .unwrap()
                .status(),
            StatusCode::OK,
        );

        let alice_location = open_upload(&app, "acme/app", Some(&alice), b"part1-").await;
        assert_all_continuation_verbs_refused(
            &app,
            &alice_location,
            Some(&bob),
            b"bob-intrusion",
            StatusCode::NOT_FOUND,
            "BLOB_UPLOAD_UNKNOWN",
        )
        .await;

        let alice_digest = digest_of(b"part1-part2");
        let completed = send(
            &app,
            "PUT",
            &format!("{alice_location}?digest={alice_digest}"),
            Some(&alice),
            b"part2",
        )
        .await;
        assert_eq!(completed.status(), StatusCode::CREATED);
        assert_eq!(
            get(&app, &format!("/v2/acme/app/blobs/{}", digest_of(b"bob-intrusion")))
                .await
                .status(),
            StatusCode::NOT_FOUND,
        );
        push_blob(&app, &bob, "acme/app", b"bob-own-blob").await;
    }

    for backend in hosted_backends() {
        let tmp = TempDir::new().unwrap();
        let (app, alice, bob) =
            two_user_app(&tmp, backend, Some(AccessList::from_tokens(["$all"]))).await;

        let alice_session = open_upload(&app, "acme/open", Some(&alice), b"authed").await;
        assert_all_continuation_verbs_refused(
            &app,
            &alice_session,
            None,
            b"anon-hijack",
            StatusCode::NOT_FOUND,
            "BLOB_UPLOAD_UNKNOWN",
        )
        .await;
        let alice_digest = digest_of(b"authed");
        assert_eq!(
            send(&app, "PUT", &format!("{alice_session}?digest={alice_digest}"), Some(&alice), b"")
                .await
                .status(),
            StatusCode::CREATED,
        );

        let anon_session = open_upload(&app, "acme/open", None, b"anon").await;
        assert_all_continuation_verbs_refused(
            &app,
            &anon_session,
            Some(&bob),
            b"bob-hijack",
            StatusCode::NOT_FOUND,
            "BLOB_UPLOAD_UNKNOWN",
        )
        .await;
        let anon_digest = digest_of(b"anon");
        assert_eq!(
            send(&app, "PUT", &format!("{anon_session}?digest={anon_digest}"), None, b"")
                .await
                .status(),
            StatusCode::CREATED,
        );
    }
}

#[tokio::test]
async fn claiming_repository_or_revoking_owner_denies_in_flight_upload_session() {
    for backend in hosted_backends() {
        let tmp = TempDir::new().unwrap();
        let (app, alice, bob) = two_user_app(&tmp, backend, None).await;

        let unclaimed_session = open_upload(&app, "acme/unclaimed", Some(&bob), b"bob-early").await;
        push_image(&app, &alice, "acme/unclaimed", "1.0").await;

        assert_all_continuation_verbs_refused(
            &app,
            &unclaimed_session,
            Some(&bob),
            b"bob-late",
            StatusCode::FORBIDDEN,
            "DENIED",
        )
        .await;
        for candidate in [b"bob-early".as_slice(), b"bob-earlybob-late".as_slice(), b"bob-late"] {
            assert_eq!(
                get(&app, &format!("/v2/acme/unclaimed/blobs/{}", digest_of(candidate)))
                    .await
                    .status(),
                StatusCode::NOT_FOUND,
            );
        }

        push_image(&app, &alice, "acme/shared", "1.0").await;
        let grant = Request::put("/oci/owners/acme/shared")
            .header(header::AUTHORIZATION, &alice)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"users":["bob"]}"#))
            .unwrap();
        assert_eq!(
            app.clone()
                .oneshot(grant)
                .await
                .unwrap()
                .status(),
            StatusCode::OK,
        );

        let shared_session = open_upload(&app, "acme/shared", Some(&bob), b"bob-coowner").await;
        let revoke = Request::delete("/oci/owners/acme/shared")
            .header(header::AUTHORIZATION, &alice)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"users":["bob"]}"#))
            .unwrap();
        assert_eq!(
            app.clone()
                .oneshot(revoke)
                .await
                .unwrap()
                .status(),
            StatusCode::OK,
        );

        assert_all_continuation_verbs_refused(
            &app,
            &shared_session,
            Some(&bob),
            b"bob-revoked",
            StatusCode::FORBIDDEN,
            "DENIED",
        )
        .await;
        for candidate in [b"bob-coowner".as_slice(), b"bob-coownerbob-revoked".as_slice()] {
            assert_eq!(
                get(&app, &format!("/v2/acme/shared/blobs/{}", digest_of(candidate)))
                    .await
                    .status(),
                StatusCode::NOT_FOUND,
            );
        }
    }
}

#[tokio::test]
async fn claiming_repository_while_upload_body_streams_refuses_blob_promotion() {
    let tmp = TempDir::new().unwrap();
    let (app, alice, bob) = two_user_app(&tmp, pnpr::HostedStoreConfig::Fs, None).await;
    let payload = b"slow-streamed-blob";
    let digest = digest_of(payload);

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<axum::body::Bytes, std::io::Error>>(2);
    let (first_seen_tx, first_seen_rx) = tokio::sync::oneshot::channel();
    let stream = async_stream_from_channel(rx, Some(first_seen_tx));
    let bob_push = tokio::spawn({
        let app = app.clone();
        let uri = format!("/v2/acme/streaming/blobs/uploads/?digest={digest}");
        async move {
            app.oneshot(
                Request::post(uri)
                    .header(header::AUTHORIZATION, bob)
                    .body(Body::from_stream(stream))
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    });

    tx.send(Ok(axum::body::Bytes::from_static(b"slow-"))).await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), first_seen_rx).await.unwrap().unwrap();

    push_image(&app, &alice, "acme/streaming", "1.0").await;

    tx.send(Ok(axum::body::Bytes::from_static(b"streamed-blob"))).await
        .unwrap();
    drop(tx);

    let response = bob_push.await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        get(&app, &format!("/v2/acme/streaming/blobs/{digest}")).await.status(),
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn ownership_change_during_s3_blob_promotion_prevents_mapping_across_replicas() {
    use base64::{
        Engine as _,
        engine::general_purpose::STANDARD,
    };

    let tmp = TempDir::new().unwrap();
    let objects = std::sync::Arc::new(pausing_store::PausingStore::default());
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.storage.hosted_backend = pnpr::HostedStoreConfig::ObjectStore {
        store: std::sync::Arc::clone(&objects) as std::sync::Arc<dyn object_store::ObjectStore>,
        prefix: "race/".into(),
    };
    let auth_state = AuthState::in_memory();
    let alice_token = crate::tokens::issue_token(&auth_state, "alice").await;
    let bob_token = crate::tokens::issue_token(&auth_state, "bob").await;
    let first = router_with_auth(config.clone(), auth_state.clone());
    config.storage.cache_dir = tmp.path().join("second-cache");
    let second = router_with_auth(config, auth_state);
    let alice = format!("Basic {}", STANDARD.encode(format!("alice:{alice_token}")));
    let bob = format!("Basic {}", STANDARD.encode(format!("bob:{bob_token}")));

    // Case 1: `acme/unclaimed` is claimed on `second` while `first` is writing
    // `bob`'s blob to object storage (after pre-promotion checks ran).
    let unclaimed_bytes = b"bob-unclaimed-s3-race";
    let unclaimed_parsed = pnpr_oci::Digest::of(unclaimed_bytes);
    let unclaimed_digest = unclaimed_parsed.to_string();
    let unclaimed_location =
        open_upload(&first, "acme/unclaimed", Some(&bob), unclaimed_bytes).await;
    objects.pause_write(format!("{}.1", unclaimed_parsed.blob_filename()));

    let completing_unclaimed = tokio::spawn({
        let first = first.clone();
        let bob = bob.clone();
        let uri = format!("{unclaimed_location}?digest={unclaimed_digest}");
        async move {
            first
                .oneshot(
                    Request::put(uri)
                        .header(header::AUTHORIZATION, bob)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), objects.started.notified())
        .await
        .unwrap();
    push_image(&second, &alice, "acme/unclaimed", "1.0").await;
    objects.resume.notify_one();

    let response = completing_unclaimed.await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let parsed: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(parsed["errors"][0]["code"], "DENIED");
    assert_eq!(
        get(&second, &format!("/v2/acme/unclaimed/blobs/{unclaimed_digest}"))
            .await
            .status(),
        StatusCode::NOT_FOUND,
    );

    // Case 2: co-owner `bob` is removed on `second` while `first` is writing
    // `bob`'s blob to object storage.
    push_image(&first, &alice, "acme/shared", "1.0").await;
    let grant = Request::put("/oci/owners/acme/shared")
        .header(header::AUTHORIZATION, &alice)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"users":["bob"]}"#))
        .unwrap();
    assert_eq!(
        first
            .clone()
            .oneshot(grant)
            .await
            .unwrap()
            .status(),
        StatusCode::OK,
    );

    let revoked_bytes = b"bob-revoked-s3-race";
    let revoked_parsed = pnpr_oci::Digest::of(revoked_bytes);
    let revoked_digest = revoked_parsed.to_string();
    let shared_location = open_upload(&first, "acme/shared", Some(&bob), revoked_bytes).await;
    objects.pause_write(format!("{}.1", revoked_parsed.blob_filename()));

    let completing_revoked = tokio::spawn({
        let first = first.clone();
        let bob = bob.clone();
        let uri = format!("{shared_location}?digest={revoked_digest}");
        async move {
            first
                .oneshot(
                    Request::put(uri)
                        .header(header::AUTHORIZATION, bob)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), objects.started.notified())
        .await
        .unwrap();
    let revoke = Request::delete("/oci/owners/acme/shared")
        .header(header::AUTHORIZATION, &alice)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"users":["bob"]}"#))
        .unwrap();
    assert_eq!(
        second
            .clone()
            .oneshot(revoke)
            .await
            .unwrap()
            .status(),
        StatusCode::OK,
    );
    objects.resume.notify_one();

    let response = completing_revoked.await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let parsed: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(parsed["errors"][0]["code"], "DENIED");
    assert_eq!(
        get(&second, &format!("/v2/acme/shared/blobs/{revoked_digest}")).await.status(),
        StatusCode::NOT_FOUND,
    );
}

fn async_stream_from_channel(
    rx: tokio::sync::mpsc::Receiver<Result<axum::body::Bytes, std::io::Error>>,
    first_seen: Option<tokio::sync::oneshot::Sender<()>>,
) -> impl futures_util::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + 'static {
    futures_util::stream::unfold((rx, first_seen), |(mut receiver, mut notify)| async move {
        let item = receiver.recv().await?;
        if let Some(sender) = notify.take() {
            let _ = sender.send(());
        }
        Some((item, (receiver, notify)))
    })
}
