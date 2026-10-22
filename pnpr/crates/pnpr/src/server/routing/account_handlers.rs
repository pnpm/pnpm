use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
};

use crate::server::{
    AppState, MAX_LOGIN_BODY_BYTES,
    tokens::{delete_session_token, delete_token_by_key, get_token_list, post_create_token},
    user_accounts::{get_profile, get_user, get_whoami, put_login},
    web_login::{get_v1_login_poll, get_v1_login_start, post_v1_login},
};

/// The account endpoints — adduser/login, whoami, profile, token
/// creation/listing/revocation, logout. They are pnpr account management, not
/// npm-registry functionality: they mint and manage the tokens every
/// authenticated surface demands, so they ride every tier alongside
/// `/-/ping`. A resolver- or artifacts-only tier can then issue its own
/// credentials (`pnpm login --registry https://<resolver-host>/`) instead of
/// depending on a registry-serving replica that shares the auth backend.
///
/// Each endpoint also answers under any `/~<name>/`, so a client whose
/// registry URL is a registry endpoint can log in against it. The identity
/// endpoints are global and consult no registry state; a registry-table lookup
/// would gate nothing while turning the 401-vs-404 split into an existence
/// oracle for private registry names that the content handlers carefully mask.
pub(super) fn account_routes() -> Router<AppState> {
    let mut router = Router::new();
    for base in ["", "/~{registry}"] {
        let path = |tail: &str| format!("{base}{tail}");
        router = router
            .route(&path("/-/whoami"), get(get_whoami))
            .route(
                &path("/-/user/{user}"),
                put(put_login)
                    .get(get_user)
                    .route_layer(DefaultBodyLimit::max(MAX_LOGIN_BODY_BYTES)),
            )
            .route(&path("/-/user/token/{token}"), delete(delete_session_token))
            .route(&path("/-/npm/v1/user"), get(get_profile))
            .route(
                &path("/-/npm/v1/tokens"),
                get(get_token_list)
                    .post(post_create_token)
                    .route_layer(DefaultBodyLimit::max(MAX_LOGIN_BODY_BYTES)),
            )
            .route(&path("/-/npm/v1/tokens/token/{key}"), delete(delete_token_by_key))
            .route(
                &path("/-/v1/login"),
                post(post_v1_login).route_layer(DefaultBodyLimit::max(MAX_LOGIN_BODY_BYTES)),
            )
            .route(&path("/-/v1/login/start"), get(get_v1_login_start))
            .route(&path("/-/v1/login/poll/{session}"), get(get_v1_login_poll));
    }
    router
}
