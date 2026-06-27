use super::{AuthTokenError, fetch_auth_token};
use crate::{
    capabilities::{OidcFetch, OidcFetchError, OidcMethod, OidcRequest, OidcResponse},
    oidc::OidcHttpOptions,
};
use pretty_assertions::assert_eq;

const REGISTRY: &str = "https://registry.npmjs.org/";

macro_rules! oidc_fetch {
    ($name:ident, $body:expr) => {
        struct $name;
        impl OidcFetch for $name {
            async fn fetch(request: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
                $body(request)
            }
        }
    };
}

#[tokio::test]
async fn posts_to_escaped_exchange_endpoint_and_returns_token() {
    oidc_fetch!(Sys, |request: OidcRequest<'_>| {
        assert_eq!(request.method, OidcMethod::Post);
        assert_eq!(
            request.url,
            "https://registry.npmjs.org/-/npm/v1/oidc/token/exchange/package/@scope%2fpkg"
        );
        assert_eq!(request.authorization, "Bearer id-token");
        Ok(OidcResponse { ok: true, status: 200, body: r#"{"token":"registry-token"}"#.to_owned() })
    });

    let token =
        fetch_auth_token::<Sys>("id-token", "@scope/pkg", REGISTRY, &OidcHttpOptions::default())
            .await
            .unwrap();
    assert_eq!(token, "registry-token");
}

#[tokio::test]
async fn surfaces_exchange_error_message_from_body() {
    oidc_fetch!(Sys, |_: OidcRequest<'_>| Ok(OidcResponse {
        ok: false,
        status: 422,
        body: r#"{"body":{"message":"package not configured"}}"#.to_owned(),
    }));

    let err = fetch_auth_token::<Sys>("id-token", "pkg", REGISTRY, &OidcHttpOptions::default())
        .await
        .unwrap_err();
    match err {
        AuthTokenError::Exchange { message, http_status } => {
            assert_eq!(message, "package not configured");
            assert_eq!(http_status, 422);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn defaults_exchange_message_when_body_is_unhelpful() {
    oidc_fetch!(Sys, |_: OidcRequest<'_>| Ok(OidcResponse {
        ok: false,
        status: 500,
        body: "not json".to_owned(),
    }));

    let err = fetch_auth_token::<Sys>("id-token", "pkg", REGISTRY, &OidcHttpOptions::default())
        .await
        .unwrap_err();
    assert!(
        matches!(err, AuthTokenError::Exchange { ref message, .. } if message == "Unknown error")
    );
}

#[tokio::test]
async fn errors_on_response_without_token() {
    oidc_fetch!(Sys, |_: OidcRequest<'_>| Ok(OidcResponse {
        ok: true,
        status: 200,
        body: r#"{"other":1}"#.to_owned(),
    }));

    let err = fetch_auth_token::<Sys>("id-token", "pkg", REGISTRY, &OidcHttpOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, AuthTokenError::MalformedJson { .. }));
}

#[tokio::test]
async fn wraps_fetch_rejection() {
    oidc_fetch!(Sys, |_: OidcRequest<'_>| Err(OidcFetchError {
        reason: "connection refused".to_owned(),
    }));

    let err = fetch_auth_token::<Sys>("id-token", "pkg", REGISTRY, &OidcHttpOptions::default())
        .await
        .unwrap_err();
    assert!(
        matches!(err, AuthTokenError::Fetch { ref error_source, .. } if error_source == "connection refused")
    );
}
