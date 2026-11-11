use crate::server::{
    AuthedCaller, TargetRegistry, state_and_router, tokens::create_token,
    user_accounts::post_profile,
};
use axum::{body::Bytes, extract::State, http::StatusCode, response::Response};
use pnpr_auth::AuthState;
use pnpr_config::Config;
use pnpr_fixtures::MockConfigPresets;
use pnpr_policy::Identity;
use std::{future::Future, pin::Pin, task::Poll};
use tempfile::TempDir;

async fn queued_verification_is_throttled(is_profile: bool) {
    let temp = TempDir::new().unwrap();
    let config = Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().to_path_buf());
    let (state, _) = state_and_router(config, AuthState::in_memory(), None).unwrap();
    let permits = state.inner.identity.login_concurrency.acquire_many(8).await.unwrap();
    let identity = Identity::user("example-user");
    let mut request: Pin<Box<dyn Future<Output = Response> + Send>> = if is_profile {
        Box::pin(post_profile(
            State(state.clone()),
            AuthedCaller(identity),
            TargetRegistry(None),
            None,
            Bytes::from_static(br#"{"password":{"old":"wrong","new":"new-password"}}"#),
        ))
    } else {
        Box::pin(create_token(&state, None, &identity, br#"{"password":"wrong"}"#))
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
    queued_verification_is_throttled(true).await;
}

#[tokio::test]
async fn queued_token_requests_recheck_the_limit_before_verifying_passwords() {
    queued_verification_is_throttled(false).await;
}
