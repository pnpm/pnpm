use super::{DetermineProvenanceError, ProvenanceError, determine_provenance};
use crate::{
    capabilities::{CiInfo, EnvVar, OidcFetch, OidcFetchError, OidcRequest, OidcResponse},
    oidc::OidcHttpOptions,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pretty_assertions::assert_eq;

const REGISTRY: &str = "https://registry.npmjs.org/";

/// The id-token payload fields [`determine_provenance`] reads, typed to the
/// string the visibility check looks for so a test builds one without an
/// untyped map. Ports the inline `Payload` interface of
/// [`provenance.ts`](https://github.com/pnpm/pnpm/blob/54c5c0e028/pnpm11/releasing/commands/src/publish/oidc/provenance.ts#L86-L89).
#[derive(serde::Serialize)]
struct Payload {
    #[serde(skip_serializing_if = "Option::is_none")]
    repository_visibility: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_visibility: Option<&'static str>,
}

/// Build a JWT-shaped `header.payload.signature` token whose payload is
/// base64url-encoded as a real id-token would be.
fn id_token(payload: &Payload) -> String {
    let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload).unwrap());
    format!("header.{payload_b64}.signature")
}

/// A payload declaring the given repository visibility (the GitHub-Actions field).
fn repository_visibility(value: &'static str) -> Payload {
    Payload { repository_visibility: Some(value), project_visibility: None }
}

macro_rules! github_sys {
    ($name:ident, $fetch:expr) => {
        struct $name;
        impl CiInfo for $name {
            fn github_actions() -> bool {
                true
            }
            fn gitlab() -> bool {
                false
            }
        }
        impl EnvVar for $name {
            fn var(_: &str) -> Option<String> {
                None
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
async fn public_github_package_enables_provenance() {
    github_sys!(Sys, |request: OidcRequest<'_>| {
        assert_eq!(request.url, "https://registry.npmjs.org/-/package/@scope%2fpkg/visibility");
        Ok(OidcResponse { ok: true, status: 200, body: r#"{"public":true}"#.to_owned() })
    });

    let token = id_token(&repository_visibility("public"));
    let result = determine_provenance::<Sys>(
        "auth",
        &token,
        "@scope/pkg",
        REGISTRY,
        &OidcHttpOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(result, Some(true));
}

#[tokio::test]
async fn private_visibility_yields_no_provenance() {
    github_sys!(Sys, |_: OidcRequest<'_>| Ok(OidcResponse {
        ok: true,
        status: 200,
        body: r#"{"public":false}"#.to_owned(),
    }));

    let token = id_token(&repository_visibility("public"));
    let result =
        determine_provenance::<Sys>("auth", &token, "pkg", REGISTRY, &OidcHttpOptions::default())
            .await
            .unwrap();
    assert_eq!(result, None);
}

#[tokio::test]
async fn malformed_id_token_is_skippable() {
    github_sys!(Sys, |_: OidcRequest<'_>| unreachable!("no request for a malformed token"));

    let err = determine_provenance::<Sys>(
        "auth",
        "not-a-jwt",
        "pkg",
        REGISTRY,
        &OidcHttpOptions::default(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DetermineProvenanceError::Provenance(ProvenanceError::MalformedIdToken)));
}

#[tokio::test]
async fn private_repo_is_insufficient_information() {
    github_sys!(Sys, |_: OidcRequest<'_>| unreachable!(
        "visibility is not probed without public CI"
    ));

    let token = id_token(&repository_visibility("private"));
    let err =
        determine_provenance::<Sys>("auth", &token, "pkg", REGISTRY, &OidcHttpOptions::default())
            .await
            .unwrap_err();
    assert!(matches!(
        err,
        DetermineProvenanceError::Provenance(ProvenanceError::InsufficientInformation)
    ));
}

#[tokio::test]
async fn visibility_failure_carries_code_and_message() {
    github_sys!(Sys, |_: OidcRequest<'_>| Ok(OidcResponse {
        ok: false,
        status: 404,
        body: r#"{"code":"E404","message":"not found"}"#.to_owned(),
    }));

    let token = id_token(&repository_visibility("public"));
    let err =
        determine_provenance::<Sys>("auth", &token, "pkg", REGISTRY, &OidcHttpOptions::default())
            .await
            .unwrap_err();
    match err {
        DetermineProvenanceError::Provenance(ProvenanceError::FailedToFetchVisibility {
            message,
            status,
            ..
        }) => {
            assert_eq!(message, "E404: not found");
            assert_eq!(status, 404);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn fetch_rejection_is_a_hard_error() {
    github_sys!(Sys, |_: OidcRequest<'_>| Err(OidcFetchError { reason: "timeout".to_owned() }));

    let token = id_token(&repository_visibility("public"));
    let err =
        determine_provenance::<Sys>("auth", &token, "pkg", REGISTRY, &OidcHttpOptions::default())
            .await
            .unwrap_err();
    assert!(matches!(err, DetermineProvenanceError::Fetch(_)));
}
