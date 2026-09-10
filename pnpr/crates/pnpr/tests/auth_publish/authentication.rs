use super::{
    BASE64, Body, Request, ServiceExt, StatusCode, TempDir, Value, add_user_and_get_token,
    body_json, common, json, publish_doc, put_json, router, static_config,
    static_config_with_packages,
};
use base64::Engine;

#[tokio::test]
async fn adduser_creates_user_and_returns_token() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (_, token) = add_user_and_get_token(app, "alice", "secret").await;
    assert!(!token.is_empty(), "token should be non-empty");
}

#[tokio::test]
async fn adduser_returns_token_on_repeat_login() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, first) = add_user_and_get_token(app, "alice", "secret").await;
    let (_, second) = add_user_and_get_token(app, "alice", "secret").await;
    assert!(!first.is_empty() && !second.is_empty());
    // We mint a fresh token each call. The important property is
    // that *both* tokens work, not that they're identical.
    assert_ne!(first, second);
}

#[tokio::test]
async fn adduser_rejects_wrong_password_for_existing_user() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, _) = add_user_and_get_token(app, "alice", "secret").await;

    let path = "/-/user/org.couchdb.user:alice";
    let body = json!({
        "name": "alice", "password": "wrong",
        "email": "foo@bar.net", "type": "user", "roles": []
    });
    let response = app.oneshot(put_json(path, body)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn bearer_token_grants_access_to_protected_package() {
    let storage = common::build_storage();
    let app = router(static_config(storage.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let response = app
        .oneshot(
            Request::get("/@pnpm.e2e/needs-auth")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn basic_auth_is_rejected_for_protected_package() {
    let storage = common::build_storage();
    let app = router(static_config(storage.path().to_path_buf()));
    let (app, _) = add_user_and_get_token(app, "alice", "secret").await;

    // pnpr no longer accepts Basic credentials on requests: the header is
    // ignored (treated as anonymous), so a protected package is rejected.
    // Clients must authenticate with a bearer token.
    let basic = BASE64.encode(b"alice:secret");
    let response = app
        .oneshot(
            Request::get("/@pnpm.e2e/needs-auth")
                .header("Authorization", format!("Basic {basic}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authenticated_publish_writes_manifest_and_tarball() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"fake-tarball-bytes";
    let body = publish_doc("mypkg", "1.0.0", bytes);
    let request = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // Packument on disk
    let on_disk_packument =
        std::fs::read(storage.join("mypkg/package.json")).expect("packument written");
    let packument: Value = serde_json::from_slice(&on_disk_packument).unwrap();
    assert_eq!(packument["name"], "mypkg");
    assert_eq!(packument["versions"]["1.0.0"]["version"], "1.0.0");
    assert_eq!(packument["dist-tags"]["latest"], "1.0.0");
    assert!(packument.get("_attachments").is_none(), "_attachments should not be persisted");

    // Tarball on disk
    let on_disk_tarball =
        std::fs::read(storage.join("mypkg/mypkg-1.0.0.tgz")).expect("tarball written");
    assert_eq!(on_disk_tarball, bytes);

    // Subsequent GET serves it back with the public URL rewritten.
    let response = app.oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let served = body_json(response.into_body()).await;
    assert_eq!(
        served["versions"]["1.0.0"]["dist"]["tarball"],
        "http://example.test/mypkg/-/mypkg-1.0.0.tgz",
    );
}

#[tokio::test]
async fn dist_tag_set_requires_auth_before_body_parsing() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let request = Request::put("/-/package/anything/dist-tags/latest")
        .header("content-type", "application/json")
        .body(Body::from("not json"))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unpublish_policy_denies_publish_authorized_package_delete() {
    let tmp = TempDir::new().unwrap();
    let (config, storage) = static_config_with_packages(
        &tmp,
        "  'unpub-policy':
    access: $all
    publish: $authenticated
    unpublish: admin",
    );
    let app = router(config);
    let (app, alice) = add_user_and_get_token(app, "alice", "secret").await;
    let (app, admin) = add_user_and_get_token(app, "admin", "secret").await;

    let body = publish_doc("unpub-policy", "1.0.0", b"contents");
    let request = Request::put("/unpub-policy")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {alice}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);
    assert!(storage.join("unpub-policy/package.json").exists());

    let request = Request::delete("/unpub-policy/-rev/anything")
        .header("Authorization", format!("Bearer {alice}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::FORBIDDEN);
    assert!(storage.join("unpub-policy/package.json").exists());

    let request = Request::delete("/unpub-policy/-rev/anything")
        .header("Authorization", format!("Bearer {admin}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);
    assert!(!storage.join("unpub-policy").exists());
}

#[tokio::test]
async fn unpublish_policy_denies_publish_authorized_tarball_delete() {
    let tmp = TempDir::new().unwrap();
    let (config, storage) = static_config_with_packages(
        &tmp,
        "  'tarball-policy':
    access: $all
    publish: $authenticated
    unpublish: admin",
    );
    let app = router(config);
    let (app, alice) = add_user_and_get_token(app, "alice", "secret").await;
    let (app, admin) = add_user_and_get_token(app, "admin", "secret").await;

    let body = publish_doc("tarball-policy", "1.0.0", b"contents");
    let request = Request::put("/tarball-policy")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {alice}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);
    assert!(storage.join("tarball-policy/tarball-policy-1.0.0.tgz").exists());

    let request = Request::delete("/tarball-policy/-/tarball-policy-1.0.0.tgz/-rev/anything")
        .header("Authorization", format!("Bearer {alice}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::FORBIDDEN);
    assert!(storage.join("tarball-policy/tarball-policy-1.0.0.tgz").exists());

    let request = Request::delete("/tarball-policy/-/tarball-policy-1.0.0.tgz/-rev/anything")
        .header("Authorization", format!("Bearer {admin}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);
    assert!(!storage.join("tarball-policy/tarball-policy-1.0.0.tgz").exists());
}
