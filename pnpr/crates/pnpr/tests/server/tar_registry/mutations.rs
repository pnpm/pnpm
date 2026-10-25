use super::{
    AuthState, Body, Request, ServiceExt, StatusCode, TempDir, acme_config, body_json, get, gzip,
    header, hosted_publish_request, issue_token, router_with_auth, tar_of, widget_tar,
};

async fn put_json(
    app: &axum::Router,
    uri: &str,
    token: &str,
    body: &serde_json::Value,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::put(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(serde_json::to_vec(body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn a_deprecate_through_a_tar_registry_keeps_the_stored_dist() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path().to_path_buf()), auth);
    let tgz = gzip(&widget_tar());
    let publish = app
        .clone()
        .oneshot(hosted_publish_request(
            "/npm/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &tgz,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);
    let stored = body_json(get(&app, "/npm/@acme%2Fwidget", None).await.into_body()).await;

    let mut packument =
        body_json(get(&app, "/npm/~zstd/@acme%2Fwidget", None).await.into_body()).await;
    packument["versions"]["1.0.0"]["deprecated"] = serde_json::json!("use 2.0.0 instead");
    let response = put_json(&app, "/npm/~zstd/@acme%2Fwidget", &token, &packument).await;
    assert_eq!(response.status(), StatusCode::CREATED);

    let after = body_json(get(&app, "/npm/@acme%2Fwidget", None).await.into_body()).await;
    assert_eq!(after["versions"]["1.0.0"]["deprecated"], "use 2.0.0 instead");
    assert_eq!(after["versions"]["1.0.0"]["dist"], stored["versions"]["1.0.0"]["dist"]);
}

#[tokio::test]
async fn a_partial_unpublish_through_a_tar_registry_keeps_the_stored_dist_and_deletes_the_tarball()
{
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path().to_path_buf()), auth);
    for (version, content) in [("1.0.0", "a"), ("1.0.1", "b")] {
        let tar = tar_of(&[("package/index.js", content.as_bytes())]);
        let publish = app
            .clone()
            .oneshot(hosted_publish_request(
                "/npm/~acme/@acme/widget",
                "@acme/widget",
                version,
                &gzip(&tar),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(publish.status(), StatusCode::CREATED);
    }
    let stored = body_json(get(&app, "/npm/@acme%2Fwidget", None).await.into_body()).await;

    let mut packument =
        body_json(get(&app, "/npm/~zstd/@acme%2Fwidget", None).await.into_body()).await;
    assert_ne!(
        packument["versions"]["1.0.0"]["dist"]["integrity"],
        stored["versions"]["1.0.0"]["dist"]["integrity"],
    );
    packument["versions"]
        .as_object_mut()
        .unwrap()
        .remove("1.0.1");
    packument["dist-tags"]["latest"] = serde_json::json!("1.0.0");
    let response = put_json(&app, "/npm/~zstd/@acme%2Fwidget/-rev/1-0", &token, &packument).await;
    assert_eq!(response.status(), StatusCode::CREATED);

    let after = body_json(get(&app, "/npm/@acme%2Fwidget", None).await.into_body()).await;
    assert!(after["versions"].get("1.0.1").is_none());
    assert_eq!(after["versions"]["1.0.0"]["dist"], stored["versions"]["1.0.0"]["dist"]);

    let kept = get(&app, "/npm/@acme/widget/-/widget-1.0.0.tgz", None).await;
    assert_eq!(kept.status(), StatusCode::OK);
    let removed = get(&app, "/npm/@acme/widget/-/widget-1.0.1.tgz", None).await;
    assert_eq!(removed.status(), StatusCode::NOT_FOUND);

    let client_delete = app
        .clone()
        .oneshot(
            Request::delete("/npm/@acme/widget/-/widget-1.0.1.tgz/-rev/2-0")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(client_delete.status(), StatusCode::CREATED);
}
