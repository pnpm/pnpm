use std::future::pending;

use tokio::{io::AsyncReadExt as _, net::TcpListener};

use super::{
    AuthState, Body, Duration, Request, ServiceExt, StatusCode, TempDir, config_for,
    drain_resolve_response, json, router_with_auth,
};
use crate::tokens::issue_token;

/// `/-/pnpr/v0/resolve` and `/-/pnpr/v0/verify-lockfile` cap request bodies
/// at 32 MiB instead of inheriting the server's 100 MiB publish limit.
#[tokio::test]
async fn resolver_routes_reject_bodies_over_route_limit_with_413() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "caller").await;
    let app = router_with_auth(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()), auth);

    let oversized = vec![b' '; 32 * 1024 * 1024 + 1];
    for route in ["/-/pnpr/v0/resolve", "/-/pnpr/v0/verify-lockfile"] {
        let response = app
            .clone()
            .oneshot(
                Request::post(route)
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(oversized.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE, "{route}");
    }
}

#[tokio::test]
async fn request_timeout_continues_to_bound_streaming_resolve_after_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let upstream = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut scratch = [0u8; 256];
                let _ = socket.read(&mut scratch).await;
                pending::<()>().await;
            });
        }
    });

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "caller").await;
    let mut config = config_for(&origin, tmp.path().to_path_buf());
    config.http.request_timeout = Some(Duration::from_millis(250));
    let app = router_with_auth(config, auth);

    let body = json!({
        "dependencies": { "stalled-pkg": "1.0.0" },
        "registry": format!("{origin}/"),
        "trustLockfile": true,
        "preferFrozenLockfile": false,
    });
    let response = app
        .oneshot(
            Request::post("/-/pnpr/v0/resolve")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let (status, body) =
        tokio::time::timeout(Duration::from_secs(2), drain_resolve_response(response))
            .await
            .expect("requestTimeout must stop streaming resolve even after 200 headers are sent");

    assert_eq!(status, StatusCode::OK);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains(r#""code":"ERR_PNPR_RESOLVE_TIMEOUT""#), "unexpected body: {text}");

    upstream.abort();
}
