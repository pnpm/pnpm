use crate::server::{
    AuthedCaller, TargetRegistry, state_and_router,
    tokens::{Caller, create_token},
    user_accounts::post_profile,
};
use axum::{body::Bytes, extract::State, http::StatusCode, response::Response};
use pnpr_auth::AuthState;
use pnpr_config::Config;
use pnpr_fixtures::MockConfigPresets;
use pnpr_policy::Identity;
use std::{future::Future, pin::Pin, task::Poll};
use tempfile::TempDir;

enum VerificationRoute {
    Profile,
    Token,
}

async fn queued_verification_is_throttled(route: VerificationRoute) {
    let temp = TempDir::new().unwrap();
    let config = Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().to_path_buf());
    let (state, _) = state_and_router(config, AuthState::in_memory(), None).unwrap();
    let permits = state.inner.identity.login_concurrency.acquire_many(8).await.unwrap();
    let identity = Identity::user("example-user");
    let caller = Caller { identity: &identity, credential: None };
    let mut request: Pin<Box<dyn Future<Output = Response> + Send>> = match route {
        VerificationRoute::Profile => Box::pin(post_profile(
            State(state.clone()),
            AuthedCaller(identity.clone()),
            TargetRegistry(None),
            None,
            Bytes::from_static(br#"{"password":{"old":"wrong","new":"new-password"}}"#),
        )),
        VerificationRoute::Token => {
            Box::pin(create_token(&state, None, &caller, br#"{"password":"wrong"}"#))
        }
    };
    std::future::poll_fn(|context| {
        assert!(
            request
                .as_mut()
                .poll(context)
                .is_pending(),
        );
        Poll::Ready(())
    })
    .await;
    for _ in 0..5 {
        state.inner.identity.login_rate_limiter.record_failure(None, "example-user");
    }
    drop(permits);
    let response = request.await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().contains_key("retry-after"));
}

#[tokio::test]
async fn queued_profile_requests_recheck_the_limit_before_verifying_passwords() {
    queued_verification_is_throttled(VerificationRoute::Profile).await;
}

#[tokio::test]
async fn queued_token_requests_recheck_the_limit_before_verifying_passwords() {
    queued_verification_is_throttled(VerificationRoute::Token).await;
}

#[tokio::test]
async fn passwordless_tokens_stay_within_the_calling_tokens_networks() {
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, header},
    };
    use tower::ServiceExt as _;

    let temp = TempDir::new().unwrap();
    let config = Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().to_path_buf());
    let auth = AuthState::in_memory();
    let id = auth.users.provision_user("example-user").await.unwrap();
    let (caller, _) = auth.tokens
        .issue_token(
            &id,
            pnpr_auth::NewToken::new().with_cidr_whitelist(vec!["10.1.0.0/16".to_string()]),
        )
        .await
        .unwrap();
    let (_, router) = state_and_router(config, auth, None).unwrap();
    let create = |body: &'static str| {
        let mut request = Request::post("/-/npm/v1/tokens")
            .header(header::AUTHORIZATION, format!("Bearer {caller}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(crate::server::PeerAddr("10.1.2.3:1".parse().unwrap())));
        router.clone().oneshot(request)
    };

    let inherited = create("{}").await.unwrap();
    assert_eq!(inherited.status(), StatusCode::OK);
    let body = axum::body::to_bytes(inherited.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["cidr_whitelist"], serde_json::json!(["10.1.0.0/16"]));

    let narrower = create(r#"{"cidr_whitelist":["10.1.2.0/24"]}"#).await.unwrap();
    assert_eq!(narrower.status(), StatusCode::OK);
    let wider = create(r#"{"cidr_whitelist":["10.0.0.0/8"]}"#).await.unwrap();
    assert_eq!(wider.status(), StatusCode::BAD_REQUEST);
}
