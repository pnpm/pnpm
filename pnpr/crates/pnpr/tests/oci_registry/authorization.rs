use super::{
    AccessList, AuthState, Body, Request, Router, ServiceExt, StatusCode, TempDir, Value, basic,
    body_bytes, digest_of, get, header, oci_config, push_blob, push_image, router_with_auth, token,
};

#[tokio::test]
async fn reads_of_a_private_repository_are_kept_out_of_shared_caches() {
    let tmp = TempDir::new().unwrap();
    let config = oci_config(tmp.path().to_path_buf(), "$authenticated");
    let accounts = crate::tokens::seeded_auth(&config, &[("alice", "secret123")]);
    let app = router_with_auth(config, accounts);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    for path in ["/v2/acme/app/manifests/1.0", "/v2/acme/app/tags/list", "/v2/_catalog"] {
        let request = Request::get(path)
            .header(header::AUTHORIZATION, &auth)
            .body(Body::empty())
            .unwrap();
        let response = app
            .clone()
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .map(|value| value.to_str().unwrap()),
            Some("private, no-store"),
            "{path} must not be storable by a shared cache",
        );
    }
}

#[tokio::test]
async fn deletion_requires_read_access_even_with_a_permissive_unpublish_rule() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "alice");
    let hosted = config.routing.hosted.get_mut("images").unwrap();
    hosted.rules =
        std::mem::take(&mut hosted.rules).with_default_unpublish(AccessList::from_tokens(["$all"]));
    let accounts = crate::tokens::seeded_auth(&config, &[("alice", "secret123")]);
    let app = router_with_auth(config, accounts);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "latest").await;
    let digest = push_blob(&app, &auth, "acme/app", b"orphan").await;
    for path in
        ["/v2/acme/app/manifests/latest".to_string(), format!("/v2/acme/app/blobs/{digest}")]
    {
        let response = app
            .clone()
            .oneshot(
                Request::delete(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let response = app
        .oneshot(
            Request::get(format!("/v2/acme/app/blobs/{digest}"))
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn token_scopes_ignore_unknown_resources_and_count_distinct_repositories() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.http.oci.bearer_auth = true;
    let accounts = crate::tokens::seeded_auth(&config, &[("alice", "secret123")]);
    let app = router_with_auth(config, accounts);
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    for index in 0..32 {
        query.append_pair("scope", &format!("repository:other/app{index}:pull"));
    }
    query.append_pair("scope", "repository(plugin):other/app0:pull");
    query.append_pair("scope", "registry:catalog:*");
    query.append_pair("scope", "repository:host:5000/app:pull");
    let query = query.finish();
    let response = get(&app, &format!("/v2/token?{query}")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::get("/v2/_catalog")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", payload["token"].as_str().unwrap()),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = get(&app, "/v2/token?scope=repository(plugin):acme/app:pull").await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::get("/v2/acme/app/manifests/latest")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", payload["token"].as_str().unwrap()),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = get(&app, &format!("/v2/token?{query}&scope=repository:other/extra:pull")).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn oci_bearer_updates_parent_token_last_used_at() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$authenticated");
    config.http.oci.bearer_auth = true;
    let auth = AuthState::in_memory();
    let tokens = std::sync::Arc::clone(&auth.tokens);
    let app = router_with_auth(config, auth);

    let raw_parent = token(&app).await;

    // Mint an OCI token with pull scope for acme/app
    let res = app
        .clone()
        .oneshot(
            Request::get("/v2/token?scope=repository:acme/app:pull")
                .header(header::AUTHORIZATION, basic(&raw_parent))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let oci_res: Value = serde_json::from_slice(&body_bytes(res.into_body()).await).unwrap();
    let oci_token = oci_res["token"].as_str().unwrap();

    let aged = crate::tokens::age_last_use(&tokens, &raw_parent).await;

    // A request the scope does not permit is rejected before it counts as a use.
    let req = Request::get("/v2/_catalog")
        .header(header::AUTHORIZATION, format!("Bearer {oci_token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let parent_mid = tokens
        .lookup_record(&raw_parent)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(parent_mid.last_used_at, aged);

    let req = Request::get("/v2/acme/app/manifests/latest")
        .header(header::AUTHORIZATION, format!("Bearer {oci_token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    crate::tokens::wait_for_last_use_after(&tokens, &raw_parent, aged).await;
}

#[tokio::test]
async fn only_the_owner_pushes_to_a_claimed_repository() {
    use base64::{
        Engine as _,
        engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    };

    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.http.oci.bearer_auth = true;
    let auth = AuthState::in_memory();
    let alice = crate::tokens::issue_token(&auth, "alice").await;
    let bob = crate::tokens::issue_token(&auth, "bob").await;
    let app = router_with_auth(config, auth);
    let alice = format!("Basic {}", STANDARD.encode(format!("alice:{alice}")));
    let bob = format!("Basic {}", STANDARD.encode(format!("bob:{bob}")));
    push_image(&app, &alice, "acme/app", "1.0").await;

    let blob = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &bob)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(blob)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
    );
    let manifest = Request::put("/v2/acme/app/manifests/2.0")
        .header(header::AUTHORIZATION, &bob)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
        .body(Body::from(super::image_manifest("config", &["layer"])))
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(manifest)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
    );

    for (caller, granted) in [(&alice, vec!["pull", "push"]), (&bob, vec!["pull"])] {
        let request = Request::get("/v2/token?scope=repository:acme/app:pull,push")
            .header(header::AUTHORIZATION, caller)
            .body(Body::empty())
            .unwrap();
        let response = app
            .clone()
            .oneshot(request)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        let token = body["token"]
            .as_str()
            .unwrap()
            .strip_prefix("pnpr_oci_")
            .unwrap();
        let payload = URL_SAFE_NO_PAD
            .decode(token.split_once('.').unwrap().0)
            .unwrap();
        let claims: Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(claims["scopes"]["acme/app"], serde_json::json!(granted), "{caller}");
    }
}

#[tokio::test]
async fn deleting_every_image_keeps_the_repository_owned() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    let hosted = config.routing.hosted.get_mut("images").unwrap();
    hosted.rules = std::mem::take(&mut hosted.rules)
        .with_default_unpublish(AccessList::from_tokens(["$owners"]));
    let auth = AuthState::in_memory();
    let alice = crate::tokens::issue_token(&auth, "alice").await;
    let bob = crate::tokens::issue_token(&auth, "bob").await;
    let app = router_with_auth(config, auth);
    let alice = format!("Basic {}", STANDARD.encode(format!("alice:{alice}")));
    let bob = format!("Basic {}", STANDARD.encode(format!("bob:{bob}")));
    let digest = push_image(&app, &alice, "acme/app", "1.0").await;
    let delete = Request::delete(format!("/v2/acme/app/manifests/{digest}"))
        .header(header::AUTHORIZATION, &alice)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(delete)
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED,
    );

    let blob = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &bob)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(blob)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
    );
    push_image(&app, &alice, "acme/app", "1.0").await;
}

#[tokio::test]
async fn the_owner_shares_a_repository_through_the_owners_endpoint() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let alice = crate::tokens::issue_token(&auth, "alice").await;
    let bob = crate::tokens::issue_token(&auth, "bob").await;
    let app = router_with_auth(oci_config(tmp.path().to_path_buf(), "$all"), auth);
    let alice = format!("Basic {}", STANDARD.encode(format!("alice:{alice}")));
    let bob = format!("Basic {}", STANDARD.encode(format!("bob:{bob}")));
    push_image(&app, &alice, "acme/app", "1.0").await;

    let change = |method: &str, caller: &str| {
        Request::builder()
            .method(method)
            .uri("/oci/owners/acme/app")
            .header(header::AUTHORIZATION, caller)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"users":["bob"]}"#))
            .unwrap()
    };
    let status = |request: Request<Body>| {
        let app = app.clone();
        async move {
            app.oneshot(request).await
                .unwrap()
                .status()
        }
    };
    assert_eq!(status(change("PUT", &bob)).await, StatusCode::FORBIDDEN);
    assert_eq!(status(change("PUT", &alice)).await, StatusCode::OK);
    push_image(&app, &bob, "acme/app", "2.0").await;

    let response = app
        .clone()
        .oneshot(
            Request::get("/oci/owners/acme/app")
                .header(header::AUTHORIZATION, &alice)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let logins: Vec<_> = body["users"]
        .as_array()
        .unwrap()
        .iter()
        .map(|user| user["login"].as_str().unwrap())
        .collect();
    assert_eq!(logins, ["alice", "bob"]);
    assert_eq!(status(change("DELETE", &alice)).await, StatusCode::OK);
    let blob = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &bob)
        .body(Body::empty())
        .unwrap();
    assert_eq!(status(blob).await, StatusCode::FORBIDDEN);
}

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
    use base64::{Engine as _, engine::general_purpose::STANDARD};

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
