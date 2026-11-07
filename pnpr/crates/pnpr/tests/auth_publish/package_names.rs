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
            "{name}",
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
            .is_none(),
        );
    }
}

#[tokio::test]
async fn legacy_names_allow_direct_and_staged_deprecation_but_not_new_versions() {
    let temp = TempDir::new().unwrap();
    pnpr_storage::layout::write_local_layout_marker(temp.path()).unwrap();
    let mut legacy = publish_doc("Upper", "1.0.0", b"data");
    legacy
        .as_object_mut()
        .unwrap()
        .remove("_attachments");
    pnpr_fixtures::write_hosted_document(
        temp.path(),
        "",
        pnpr_storage::Ecosystem::Npm,
        "Upper",
        &serde_json::to_vec(&legacy).unwrap(),
    );
    let app = router(static_config(temp.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "publisher", "secret").await;
    let get = || {
        Request::get("/npm/Upper")
            .body(Body::empty())
            .unwrap()
    };
    for (method, path, deprecated) in [
        ("PUT", "/npm/Upper", Some("use a lowercase name")),
        ("PUT", "/npm/Upper", None),
        ("POST", "/npm/-/stage/package/Upper", Some("use a lowercase name")),
        ("POST", "/npm/-/stage/package/Upper", None),
    ] {
        let response = app
            .clone()
            .oneshot(get())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut document: serde_json::Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        let version = document["versions"]["1.0.0"].as_object_mut().unwrap();
        if let Some(message) = deprecated {
            version.insert("deprecated".to_string(), serde_json::json!(message));
        } else {
            version.remove("deprecated");
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(serde_json::to_vec(&document).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let error = body_bytes(response.into_body()).await;
        assert_eq!(status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&error));
        if method == "POST" {
            let stage: serde_json::Value = serde_json::from_slice(&error).unwrap();
            let stage_id = stage["stageId"].as_str().unwrap();
            let response = app
                .clone()
                .oneshot(
                    Request::post(format!("/npm/-/stage/{stage_id}/approve"))
                        .header("authorization", format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let error = body_bytes(response.into_body()).await;
            assert_eq!(status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&error));
        }
        let response = app
            .clone()
            .oneshot(get())
            .await
            .unwrap();
        let stored: serde_json::Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(stored["versions"]["1.0.0"]["deprecated"].as_str(), deprecated);
    }
    let new_version = publish_doc("Upper", "2.0.0", b"data");
    let mut without_attachment = new_version.clone();
    without_attachment
        .as_object_mut()
        .unwrap()
        .remove("_attachments");
    for (method, path, body) in [
        ("PUT", "/npm/Upper", new_version.clone()),
        ("POST", "/npm/-/stage/package/Upper", new_version),
        ("POST", "/npm/-/stage/package/Upper", without_attachment),
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
    }
}
