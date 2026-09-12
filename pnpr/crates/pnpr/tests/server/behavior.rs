use super::{
    Body, Request, ServiceExt, StatusCode, TempDir, body_bytes, body_json, config_for, json, router,
};

#[tokio::test]
async fn artifacts_only_advertises_and_mounts_only_the_artifact_protocol() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    config.registry.enabled = false;
    config.resolver.enabled = false;
    config.artifacts.enabled = true;
    let app = router(config);

    let handshake =
        app.clone().oneshot(Request::get("/-/pnpr").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(handshake.status(), StatusCode::OK);
    assert_eq!(
        body_json(handshake.into_body()).await,
        json!({
            "pnpr": {
                "versions": [],
                "artifacts": [0],
                "pipeline": [],
                "fixLockfile": [],
                "ecosystems": [],
                "publish": [],
            }
        }),
    );

    let artifact = app
        .clone()
        .oneshot(Request::post("/-/pnpr/v0/artifacts/resolve").body(Body::from("{}")).unwrap())
        .await
        .unwrap();
    assert_eq!(artifact.status(), StatusCode::UNAUTHORIZED);
    assert!(
        String::from_utf8_lossy(&body_bytes(artifact.into_body()).await)
            .contains("shared artifacts"),
    );

    let resolve = app
        .oneshot(Request::post("/-/pnpr/v0/resolve").body(Body::from("{}")).unwrap())
        .await
        .unwrap();
    assert_eq!(resolve.status(), StatusCode::NOT_FOUND);
}
