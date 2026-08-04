use super::{
    Access, CreatePublishOptionsError, CreatePublishOptionsInput, OidcTokenProvenance,
    create_publish_options, fetch_token_and_provenance_by_oidc, find_registry_info, resolve_access,
    scope_of,
};
use crate::{
    capabilities::{Clock, EnvVar, OidcFetch, OidcFetchError, OidcRequest, OidcResponse},
    oidc::OidcHttpOptions,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pacquet_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn extracts_scope() {
    assert_eq!(scope_of("@scope/pkg"), Some("scope"));
    assert_eq!(scope_of("pkg"), None);
    assert_eq!(scope_of("@scope/"), None);
}

#[test]
fn publish_config_registry_wins() {
    let registry = find_registry_info(
        "@a/b",
        "https://default.example/",
        &BTreeMap::new(),
        Some("https://from-config.example"),
    )
    .unwrap();
    assert_eq!(registry.as_str(), "https://from-config.example/");
}

#[test]
fn scoped_registry_is_used_for_scoped_package() {
    let mut scoped = BTreeMap::new();
    scoped.insert("@a".to_owned(), "https://scoped.example/".to_owned());
    let registry = find_registry_info("@a/b", "https://default.example/", &scoped, None).unwrap();
    assert_eq!(registry.as_str(), "https://scoped.example/");
}

#[test]
fn falls_back_to_default_registry() {
    let registry =
        find_registry_info("pkg", "https://default.example/", &BTreeMap::new(), None).unwrap();
    assert_eq!(registry.as_str(), "https://default.example/");
}

#[test]
fn rejects_unsupported_protocol() {
    let err = find_registry_info("pkg", "ftp://nope.example/", &BTreeMap::new(), None).unwrap_err();
    assert_eq!(err.registry_url, "ftp://nope.example/");
}

#[test]
fn access_prefers_explicit_then_manifest() {
    let manifest = json!({ "publishConfig": { "access": "restricted" } });
    assert_eq!(resolve_access(Some(Access::Public), &manifest), Some(Access::Public));
    assert_eq!(resolve_access(None, &manifest), Some(Access::Restricted));
    assert_eq!(resolve_access(None, &json!({})), None);
}

const REGISTRY: &str = "https://registry.npmjs.org/";

/// Build a JWT-shaped `header.payload.signature` token carrying the GitHub
/// repository visibility the orchestrator's visibility probe relies on.
fn public_repo_id_token() -> String {
    let payload = json!({ "repository_visibility": "public" });
    let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
    format!("header.{payload_b64}.signature")
}

/// A `fetch` body that walks the GitHub happy-path OIDC chain by dispatching on
/// the request URL: id-token request, then token exchange, then the visibility
/// probe. `id_token_value` is spliced into the id-token response so a test can
/// feed either a real JWT or a malformed string.
fn github_chain_fetch(
    request: &OidcRequest<'_>,
    id_token_value: &str,
) -> Result<OidcResponse, OidcFetchError> {
    let body = if request.url.contains("audience=") {
        format!(r#"{{"value":"{id_token_value}"}}"#)
    } else if request.url.contains("/oidc/token/exchange/") {
        r#"{"token":"registry-token"}"#.to_owned()
    } else if request.url.contains("/visibility") {
        r#"{"public":true}"#.to_owned()
    } else {
        unreachable!("unexpected OIDC request URL: {}", request.url)
    };
    Ok(OidcResponse { ok: true, status: 200, body })
}

/// GitHub-Actions provider whose env yields the id-token request token/url.
macro_rules! github_sys {
    ($name:ident, $fetch:expr) => {
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
                    "ACTIONS_ID_TOKEN_REQUEST_TOKEN" => Some("request-token".to_owned()),
                    "ACTIONS_ID_TOKEN_REQUEST_URL" => {
                        Some("https://github.example/token".to_owned())
                    }
                    _ => None,
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
async fn oidc_happy_path_returns_token_and_provenance() {
    github_sys!(Sys, |request: OidcRequest<'_>| github_chain_fetch(
        &request,
        &public_repo_id_token()
    ));

    let result = fetch_token_and_provenance_by_oidc::<Sys, SilentReporter>(
        "pkg",
        REGISTRY,
        None,
        &OidcHttpOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        result,
        Some(OidcTokenProvenance {
            auth_token: "registry-token".to_owned(),
            provenance: Some(true),
        }),
    );
}

#[tokio::test]
async fn oidc_returns_none_outside_ci() {
    struct Sys;
    impl Clock for Sys {
        fn now_ms() -> u64 {
            unreachable!()
        }
    }
    impl EnvVar for Sys {
        fn var(_: &str) -> Option<String> {
            None
        }
    }
    impl OidcFetch for Sys {
        async fn fetch(_: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
            unreachable!("no request outside supported CI")
        }
    }

    let result = fetch_token_and_provenance_by_oidc::<Sys, SilentReporter>(
        "pkg",
        REGISTRY,
        None,
        &OidcHttpOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(result, None);
}

#[tokio::test]
async fn oidc_override_skips_visibility_probe() {
    github_sys!(Sys, |request: OidcRequest<'_>| {
        // With an explicit provenance override the visibility probe is skipped.
        assert!(!request.url.contains("/visibility"));
        github_chain_fetch(&request, &public_repo_id_token())
    });

    let result = fetch_token_and_provenance_by_oidc::<Sys, SilentReporter>(
        "pkg",
        REGISTRY,
        Some(false),
        &OidcHttpOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        result,
        Some(OidcTokenProvenance {
            auth_token: "registry-token".to_owned(),
            provenance: Some(false),
        }),
    );
}

#[tokio::test]
async fn oidc_skips_when_auth_exchange_fails() {
    github_sys!(Sys, |request: OidcRequest<'_>| {
        if request.url.contains("audience=") {
            Ok(OidcResponse {
                ok: true,
                status: 200,
                body: format!(r#"{{"value":"{}"}}"#, public_repo_id_token()),
            })
        } else if request.url.contains("/oidc/token/exchange/") {
            Ok(OidcResponse { ok: false, status: 422, body: String::new() })
        } else {
            unreachable!("visibility is not probed once the exchange fails")
        }
    });

    let result = fetch_token_and_provenance_by_oidc::<Sys, SilentReporter>(
        "pkg",
        REGISTRY,
        None,
        &OidcHttpOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(result, None);
}

#[tokio::test]
async fn oidc_keeps_token_when_provenance_undeterminable() {
    github_sys!(Sys, |request: OidcRequest<'_>| github_chain_fetch(&request, "not-a-jwt"));

    let result = fetch_token_and_provenance_by_oidc::<Sys, SilentReporter>(
        "pkg",
        REGISTRY,
        None,
        &OidcHttpOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        result,
        Some(OidcTokenProvenance { auth_token: "registry-token".to_owned(), provenance: None }),
    );
}

#[tokio::test]
async fn create_publish_options_skips_oidc_when_disabled() {
    struct Sys;
    impl Clock for Sys {
        fn now_ms() -> u64 {
            unreachable!()
        }
    }
    impl EnvVar for Sys {
        fn var(_: &str) -> Option<String> {
            unreachable!()
        }
    }
    impl OidcFetch for Sys {
        async fn fetch(_: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
            unreachable!("no fetch when OIDC is disabled")
        }
    }

    let manifest = json!({ "name": "pkg" });
    let http = OidcHttpOptions::default();
    let input = CreatePublishOptionsInput {
        default_registry: "https://default.example/",
        scoped_registries: &BTreeMap::new(),
        access: None,
        tag: "latest",
        otp: Some("123456"),
        provenance: None,
        http: &http,
    };

    let resolved =
        create_publish_options::<Sys, SilentReporter>(&manifest, &input, false).await.unwrap();
    assert_eq!(resolved.registry.as_str(), "https://default.example/");
    assert_eq!(resolved.default_tag, "latest");
    assert_eq!(resolved.otp, Some("123456".to_owned()));
    assert_eq!(resolved.auth_token_override, None);
    assert_eq!(resolved.provenance, None);
}

#[tokio::test]
async fn create_publish_options_applies_oidc_when_enabled() {
    github_sys!(Sys, |request: OidcRequest<'_>| github_chain_fetch(
        &request,
        &public_repo_id_token()
    ));

    let manifest = json!({ "name": "pkg" });
    let http = OidcHttpOptions::default();
    let input = CreatePublishOptionsInput {
        default_registry: REGISTRY,
        scoped_registries: &BTreeMap::new(),
        access: None,
        tag: "latest",
        otp: None,
        provenance: None,
        http: &http,
    };

    let resolved =
        create_publish_options::<Sys, SilentReporter>(&manifest, &input, true).await.unwrap();
    assert_eq!(resolved.auth_token_override, Some("registry-token".to_owned()));
    assert_eq!(resolved.provenance, Some(true));
}

#[tokio::test]
async fn create_publish_options_rejects_unsupported_protocol() {
    struct Sys;
    impl Clock for Sys {
        fn now_ms() -> u64 {
            unreachable!()
        }
    }
    impl EnvVar for Sys {
        fn var(_: &str) -> Option<String> {
            unreachable!()
        }
    }
    impl OidcFetch for Sys {
        async fn fetch(_: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
            unreachable!("registry resolution fails before any fetch")
        }
    }

    let manifest = json!({ "name": "pkg", "publishConfig": { "registry": "ftp://x/" } });
    let http = OidcHttpOptions::default();
    let input = CreatePublishOptionsInput {
        default_registry: REGISTRY,
        scoped_registries: &BTreeMap::new(),
        access: None,
        tag: "latest",
        otp: None,
        provenance: None,
        http: &http,
    };

    let err =
        create_publish_options::<Sys, SilentReporter>(&manifest, &input, true).await.unwrap_err();
    assert!(matches!(err, CreatePublishOptionsError::UnsupportedProtocol(_)));
}
