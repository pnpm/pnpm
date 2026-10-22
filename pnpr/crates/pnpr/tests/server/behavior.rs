use super::{
    Body, Request, ServiceExt, StatusCode, TempDir, body_bytes, body_json, config_for, json, router,
};

#[tokio::test]
async fn artifacts_only_advertises_and_mounts_only_the_artifact_protocol() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    config.features.registry.enabled = false;
    config.features.resolver.enabled = false;
    config.features.artifacts.enabled = true;
    let app = router(config);

    let handshake = app
        .clone()
        .oneshot(
            Request::get("/-/pnpr")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
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
                "protocols": {
                    "resolver": {
                        "version": 0,
                        "versions": [],
                        "features": [
                            "fixLockfile",
                            "verifyLockfile",
                            "projectTransforms",
                            "policyEcho",
                            "errorCodes",
                            "acceptedLockfileFormats"
                        ],
                        "ecosystems": []
                    },
                    "artifacts": {
                        "version": 0,
                        "versions": [0]
                    },
                    "compilerCache": {
                        "version": 0,
                        "versions": [0]
                    },
                    "pipeline": {
                        "version": 0,
                        "versions": []
                    },
                    "publish": {
                        "version": 0,
                        "versions": [],
                        "pnpmPublish": []
                    }
                },
                "features": {
                    "registries": true,
                    "admin": false,
                    "scim": false,
                    "signIn": false
                }
            }
        }),
    );

    let artifact = app
        .clone()
        .oneshot(
            Request::post("/-/pnpr/v0/artifacts/resolve")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(artifact.status(), StatusCode::UNAUTHORIZED);
    assert!(
        String::from_utf8_lossy(&body_bytes(artifact.into_body()).await)
            .contains("shared artifacts"),
    );

    for method in ["PUT", "HEAD"] {
        let blob = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/-/pnpr/v0/artifacts/blob?organization=acme&integrity=sha512-AAAA")
                    .body(Body::from("blob"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(blob.status(), StatusCode::UNAUTHORIZED, "an anonymous {method}");
    }

    let resolve = app
        .oneshot(
            Request::post("/-/pnpr/v0/resolve")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resolve.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn request_timeout_returns_504_gateway_timeout() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    config.http.request_timeout = Some(std::time::Duration::from_millis(50));
    let app = router(config);

    let stream = futures_util::stream::once(async {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        Ok::<axum::body::Bytes, std::convert::Infallible>(axum::body::Bytes::from("{}"))
    });
    let slow_body = Body::from_stream(stream);

    let response = app
        .oneshot(
            Request::put("/-/user/org.couchdb.user:alice")
                .header("content-type", "application/json")
                .body(slow_body)
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(
        body_json(response.into_body()).await,
        json!({
            "error": "request timed out"
        }),
    );
}
