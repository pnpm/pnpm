use super::{
    Body, Request, ServiceExt, StatusCode, TempDir, add_user_and_get_token, body_bytes,
    publish_doc, router, static_config,
};

#[tokio::test]
async fn publishing_rejects_invalid_new_names_before_storing_them() {
    let temp = TempDir::new().unwrap();
    let app = router(static_config(temp.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "publisher", "secret").await;
    for (name, path) in [
        ("demo-UPPER", "demo-UPPER"),
        ("demo~pkg", "demo%7Epkg"),
        ("demo'pkg", "demo%27pkg"),
        ("demo!pkg", "demo%21pkg"),
        ("demo(pkg", "demo%28pkg"),
        ("demo)pkg", "demo%29pkg"),
        ("demo*pkg", "demo%2Apkg"),
        ("demoé", "demo%C3%A9"),
        ("_demo", "_demo"),
        ("http", "http"),
        ("node_modules", "node_modules"),
        ("demo@x", "demo%40x"),
        ("@scope/demo@x", "@scope%2Fdemo%40x"),
        ("@Scope/demo", "@Scope%2Fdemo"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::put(format!("/npm/{path}"))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(
                        serde_json::to_vec(&publish_doc(name, "1.0.0", b"data")).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{name}");
        let error = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
        assert!(error.contains(name), "{error}");
        assert!(
            pnpr_fixtures::read_hosted_document(
                temp.path(),
                "",
                pnpr_storage::Ecosystem::Npm,
                name,
            )
            .is_none(),
            "{name}"
        );
    }
}

#[tokio::test]
async fn staged_and_batch_publishes_reject_invalid_new_names() {
    let temp = TempDir::new().unwrap();
    let app = router(static_config(temp.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "publisher", "secret").await;
    let invalid = publish_doc("demo-UPPER", "1.0.0", b"data");
    for (method, path, body) in [
        ("POST", "/npm/-/stage/package/demo-UPPER", invalid.clone()),
        ("PUT", "/npm/-/pnpr/v0/publish", serde_json::json!({ "packages": [invalid] })),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        let error = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
        assert!(error.contains("demo-UPPER"), "{error}");
        assert!(
            pnpr_fixtures::read_hosted_document(
                temp.path(),
                "",
                pnpr_storage::Ecosystem::Npm,
                "demo-UPPER",
            )
            .is_none()
        );
    }
}
