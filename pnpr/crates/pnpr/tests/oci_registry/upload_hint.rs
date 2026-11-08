use super::{
    AuthState, Body, Request, ServiceExt, StatusCode, TempDir, basic, body_bytes, digest_of, get,
    header, oci_config, router_with_auth, token,
};

use axum::response::Response;
use std::num::NonZeroU64;

#[tokio::test]
async fn chunk_hint_is_optional_and_does_not_limit_the_whole_blob() {
    let missing_mount = format!("?mount={}&from=acme/missing", digest_of(b"missing"));
    for (limit, suffix) in [
        (None, ""),
        (None, missing_mount.as_str()),
        (NonZeroU64::new(5), ""),
        (NonZeroU64::new(5), missing_mount.as_str()),
    ] {
        let tmp = TempDir::new().unwrap();
        let mut config = oci_config(tmp.path().to_path_buf(), "$all");
        config.http.oci.chunk_max_bytes = limit;
        let app = router_with_auth(config, AuthState::in_memory());
        let auth = basic(&token(&app).await);
        let response = app
            .clone()
            .oneshot(
                Request::post(format!("/v2/acme/app/blobs/uploads/{suffix}"))
                    .header(header::AUTHORIZATION, &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_chunk_hint(&response, limit);
        let location = response.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .to_owned();
        for chunk in ["hello", "world"] {
            let response = app
                .clone()
                .oneshot(
                    Request::patch(&location)
                        .header(header::AUTHORIZATION, &auth)
                        .body(Body::from(chunk))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::ACCEPTED);
            assert_chunk_hint(&response, limit);
        }
        let response = app
            .clone()
            .oneshot(
                Request::get(&location)
                    .header(header::AUTHORIZATION, &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()[header::RANGE], "0-9");
        assert_chunk_hint(&response, limit);
        let digest = digest_of(b"helloworld");
        let response = app
            .clone()
            .oneshot(
                Request::put(format!("{location}?digest={digest}"))
                    .header(header::AUTHORIZATION, &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert!(!response.headers().contains_key("oci-chunk-max-length"));
        let response = get(&app, &format!("/v2/acme/app/blobs/{digest}")).await;
        assert_eq!(body_bytes(response.into_body()).await, b"helloworld".as_slice());
    }
}

fn assert_chunk_hint(response: &Response, limit: Option<NonZeroU64>) {
    assert_eq!(
        response
            .headers()
            .get("oci-chunk-max-length")
            .map(|value| value.to_str().unwrap()),
        limit.map(|_| "5"),
    );
}
