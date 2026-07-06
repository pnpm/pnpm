use super::{GetIdTokenError, IdTokenError, get_id_token};
use crate::{
    capabilities::{Clock, EnvVar, OidcFetch, OidcFetchError, OidcRequest, OidcResponse},
    oidc::OidcHttpOptions,
};
use pacquet_reporter::SilentReporter;
use pretty_assertions::assert_eq;

const REGISTRY: &str = "https://registry.npmjs.org/";

/// Default clock/CI fakes the GitHub-Actions tests share. A test overrides
/// only the capability it cares about by declaring its own `Sys` struct.
macro_rules! github_actions_env {
    ($name:ident, $var:expr, $fetch:expr) => {
        struct $name;
        impl Clock for $name {
            fn now_ms() -> u64 {
                0
            }
        }
        impl EnvVar for $name {
            fn var(name: &str) -> Option<String> {
                match name {
                    "GITHUB_ACTIONS" => Some("true".to_owned()),
                    "GITLAB_CI" => None,
                    _ => $var(name),
                }
            }
        }
        impl OidcFetch for $name {
            async fn fetch(request: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
                $fetch(request)
            }
        }
    };
}

#[tokio::test]
async fn returns_npm_id_token_without_any_fetch() {
    struct Sys;
    impl EnvVar for Sys {
        fn var(name: &str) -> Option<String> {
            (name == "NPM_ID_TOKEN").then(|| "forwarded-token".to_owned())
        }
    }
    impl Clock for Sys {
        fn now_ms() -> u64 {
            unreachable!("no request is made when NPM_ID_TOKEN is set")
        }
    }
    impl OidcFetch for Sys {
        async fn fetch(_: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
            unreachable!("no request is made when NPM_ID_TOKEN is set")
        }
    }

    let token =
        get_id_token::<Sys, SilentReporter>(REGISTRY, &OidcHttpOptions::default()).await.unwrap();
    assert_eq!(token, Some("forwarded-token".to_owned()));
}

#[tokio::test]
async fn returns_none_outside_supported_ci() {
    struct Sys;
    impl EnvVar for Sys {
        fn var(_: &str) -> Option<String> {
            None
        }
    }
    impl Clock for Sys {
        fn now_ms() -> u64 {
            unreachable!("no request outside GitHub Actions")
        }
    }
    impl OidcFetch for Sys {
        async fn fetch(_: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
            unreachable!("no request outside GitHub Actions")
        }
    }

    let token =
        get_id_token::<Sys, SilentReporter>(REGISTRY, &OidcHttpOptions::default()).await.unwrap();
    assert_eq!(token, None);
}

#[tokio::test]
async fn errors_when_github_permissions_missing() {
    github_actions_env!(Sys, |_: &str| None, |_: OidcRequest<'_>| unreachable!(
        "no request when the request token/url are absent"
    ));

    let err = get_id_token::<Sys, SilentReporter>(REGISTRY, &OidcHttpOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        GetIdTokenError::IdToken(IdTokenError::GitHubWorkflowIncorrectPermissions)
    ));
}

fn github_request_env(name: &str) -> Option<String> {
    match name {
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN" => Some("request-token".to_owned()),
        "ACTIONS_ID_TOKEN_REQUEST_URL" => Some("https://github.example/token".to_owned()),
        _ => None,
    }
}

#[tokio::test]
async fn fetches_and_returns_github_id_token() {
    github_actions_env!(Sys, github_request_env, |request: OidcRequest<'_>| {
        // The audience query param is derived from the registry hostname.
        assert!(request.url.contains("audience=npm%3Aregistry.npmjs.org"));
        assert_eq!(request.authorization, "Bearer request-token");
        Ok(OidcResponse { ok: true, status: 200, body: r#"{"value":"gh-id-token"}"#.to_owned() })
    });

    let token =
        get_id_token::<Sys, SilentReporter>(REGISTRY, &OidcHttpOptions::default()).await.unwrap();
    assert_eq!(token, Some("gh-id-token".to_owned()));
}

#[tokio::test]
async fn errors_on_non_ok_github_response() {
    github_actions_env!(Sys, github_request_env, |_: OidcRequest<'_>| Ok(OidcResponse {
        ok: false,
        status: 403,
        body: String::new(),
    }));

    let err = get_id_token::<Sys, SilentReporter>(REGISTRY, &OidcHttpOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, GetIdTokenError::IdToken(IdTokenError::GitHubInvalidResponse)));
}

#[tokio::test]
async fn errors_on_github_response_without_value() {
    github_actions_env!(Sys, github_request_env, |_: OidcRequest<'_>| Ok(OidcResponse {
        ok: true,
        status: 200,
        body: r#"{"other":1}"#.to_owned(),
    }));

    let err = get_id_token::<Sys, SilentReporter>(REGISTRY, &OidcHttpOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, GetIdTokenError::IdToken(IdTokenError::GitHubJsonInvalidValue)));
}
