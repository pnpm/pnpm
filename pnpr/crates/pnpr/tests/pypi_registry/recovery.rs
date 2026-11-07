use super::{
    AuthState, Body, JSON, Request, ServiceExt, StatusCode, TempDir, Value, body_bytes, common,
    get, header, issue_token, json, pypi_config, router_with_auth, upload_form, upload_request,
    wheel_upload,
};

#[tokio::test]
async fn invalid_archives_and_relabelled_wheels_are_rejected() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "publisher").await;
    let app =
        router_with_auth(pypi_config(tmp.path().to_path_buf(), "http://upstream.invalid/"), auth);
    for content in [
        vec![42; 200],
        common::python_wheel("demo-pkg", "0.9.0"),
        common::python_wheel("other", "1.0.0"),
        common::python_wheel_with_metadata("demo-pkg", "1.0.0", "Name: demo-pkg\nVersion: 0.9.0\n"),
        common::python_wheel_with_metadata(
            "demo-pkg",
            "1.0.0",
            "Name: other\nName: demo-pkg\nVersion: 1.0.0\n",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(upload_request(
                Some(&token),
                wheel_upload("demo-pkg", "1.0.0", "demo_pkg-1.0.0-py3-none-any.whl", &content),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert_eq!(
        app.oneshot(get("/pypi/simple/demo-pkg/", Some(JSON))).await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND,
    );
}

fn management_request(method: &str, path: &str, token: Option<&str>, body: &str) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn publishers_yank_and_admins_delete_files() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let publisher = issue_token(&auth, "publisher").await;
    let admin = issue_token(&auth, "administrator").await;
    let mut config = pypi_config(tmp.path().to_path_buf(), "http://upstream.invalid/");
    config.identity.admins.insert(
        auth.users
            .account_id("administrator")
            .await
            .unwrap()
            .unwrap(),
    );
    let app = router_with_auth(config, auth);
    let filename = "demo_pkg-1.0.0-py3-none-any.whl";
    let content = common::python_wheel("demo-pkg", "1.0.0");
    assert_eq!(
        app.clone()
            .oneshot(upload_request(
                Some(&publisher),
                wheel_upload("demo-pkg", "1.0.0", filename, &content)
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK,
    );
    let file = format!("/pypi/files/demo-pkg/{filename}");
    let yank = format!("{file}/yank");
    assert_eq!(
        app.clone()
            .oneshot(management_request("PUT", &yank, None, "{}"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED,
    );
    assert_eq!(
        app.clone()
            .oneshot(management_request(
                "PUT",
                &yank,
                Some(&publisher),
                r#"{"reason":"Broken release"}"#
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK,
    );
    let response = app
        .clone()
        .oneshot(get("/pypi/simple/demo-pkg/", Some(JSON)))
        .await
        .unwrap();
    let page: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(page["files"][0]["yanked"], "Broken release");
    let response = app
        .clone()
        .oneshot(get("/pypi/simple/demo-pkg/", None))
        .await
        .unwrap();
    let html = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(html.contains(r#"data-yanked="Broken release""#));
    assert_eq!(
        app.clone()
            .oneshot(get(&file, None))
            .await
            .unwrap()
            .status(),
        StatusCode::OK,
    );
    assert_eq!(
        app.clone()
            .oneshot(management_request("PUT", &yank, Some(&publisher), r#"{"reason":""}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::OK,
    );
    let response = app
        .clone()
        .oneshot(get("/pypi/simple/demo-pkg/", Some(JSON)))
        .await
        .unwrap();
    let page: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(page["files"][0]["yanked"], true);
    let oversized = json!({"reason": "x".repeat(1025)}).to_string();
    assert_eq!(
        app.clone()
            .oneshot(management_request("PUT", &yank, Some(&publisher), &oversized))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST,
    );
    let oversized_body = json!({"reason": "x".repeat(8192)}).to_string();
    assert_eq!(
        app.clone()
            .oneshot(management_request("PUT", &yank, Some(&publisher), &oversized_body))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE,
    );
    assert_eq!(
        app.clone()
            .oneshot(management_request("DELETE", &yank, Some(&publisher), ""))
            .await
            .unwrap()
            .status(),
        StatusCode::OK,
    );
    let response = app
        .clone()
        .oneshot(get("/pypi/simple/demo-pkg/", Some(JSON)))
        .await
        .unwrap();
    let page: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(page["files"][0]["yanked"], false);
    assert_eq!(
        app.clone()
            .oneshot(management_request("DELETE", &file, None, ""))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED,
    );
    assert_eq!(
        app.clone()
            .oneshot(management_request("DELETE", &file, Some(&publisher), ""))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
    );
    assert_eq!(
        app.clone()
            .oneshot(management_request("DELETE", &file, Some(&admin), ""))
            .await
            .unwrap()
            .status(),
        StatusCode::OK,
    );
    assert_eq!(
        app.clone()
            .oneshot(get(&file, None))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND,
    );
    let response = app
        .clone()
        .oneshot(get("/pypi/simple/demo-pkg/", Some(JSON)))
        .await
        .unwrap();
    let page: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(page["files"], json!([]));
    let corrected = common::python_wheel_with_metadata(
        "demo-pkg",
        "1.0.0",
        "Name: demo-pkg\nVersion: 1.0.0\nSummary: corrected\n",
    );
    assert_ne!(corrected, content);
    let response = app
        .clone()
        .oneshot(upload_request(
            Some(&publisher),
            wheel_upload("demo-pkg", "1.0.0", filename, &corrected),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        app.clone()
            .oneshot(get(&file, None))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND,
    );
    let response = app
        .clone()
        .oneshot(get("/pypi/simple/demo-pkg/", Some(JSON)))
        .await
        .unwrap();
    let page: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(page["files"], json!([]));

    assert_eq!(
        app.oneshot(management_request("PUT", &yank, Some(&publisher), "{}"))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn sdist_metadata_must_match_the_form_and_filename() {
    use std::io::Write as _;
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "publisher").await;
    let app =
        router_with_auth(pypi_config(tmp.path().to_path_buf(), "http://upstream.invalid/"), auth);
    for (version, status) in [("0.9.0", StatusCode::BAD_REQUEST), ("1.0.0", StatusCode::OK)] {
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        archive
            .start_file::<_, ()>(
                "demo_pkg-1.0.0/PKG-INFO",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive
            .write_all(format!("Name: demo-pkg\nVersion: {version}\n").as_bytes())
            .unwrap();
        let content = archive.finish().unwrap().into_inner();
        let body = upload_form(
            &[
                (":action", "file_upload"),
                ("name", "demo-pkg"),
                ("version", "1.0.0"),
                ("filetype", "sdist"),
            ],
            "demo_pkg-1.0.0.zip",
            &content,
        );
        assert_eq!(
            app.clone()
                .oneshot(upload_request(Some(&token), body))
                .await
                .unwrap()
                .status(),
            status,
        );
    }
}
