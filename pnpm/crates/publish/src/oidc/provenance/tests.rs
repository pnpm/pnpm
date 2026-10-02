use super::{DetermineProvenanceError, ProvenanceError, determine_provenance};
use crate::{
    capabilities::{EnvVar, OidcFetch, OidcFetchError, OidcMethod, OidcRequest, OidcResponse},
    oidc::OidcHttpOptions,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pretty_assertions::assert_eq;

const REGISTRY: &str = "https://registry.npmjs.org/";

/// The id-token payload fields [`determine_provenance`] reads, typed to the
/// string the visibility check looks for so a test builds one without an
/// untyped map.
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

/// A payload declaring the given project visibility (the GitLab field).
fn project_visibility(value: &'static str) -> Payload {
    Payload { repository_visibility: None, project_visibility: Some(value) }
}

/// A host whose environment holds exactly `$env` and whose visibility fetch
/// is answered by `$fetch`.
macro_rules! ci_sys {
    ($name:ident, [$(($key:literal, $value:literal)),* $(,)?], $fetch:expr) => {
        struct $name;
        impl EnvVar for $name {
            fn var(name: &str) -> Option<String> {
                match name {
                    $($key => Some($value.to_owned()),)*
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

macro_rules! github_sys {
    ($name:ident, $fetch:expr) => {
        ci_sys!($name, [("GITHUB_ACTIONS", "true")], $fetch);
    };
}

/// Run [`determine_provenance`] for `pkg` against the npm registry.
async fn determine<Sys: EnvVar + OidcFetch>(
    id_token: &str,
) -> Result<Option<bool>, DetermineProvenanceError> {
    determine_provenance::<Sys>("auth", id_token, "pkg", REGISTRY, &OidcHttpOptions::default())
        .await
}

#[tokio::test]
async fn public_github_package_enables_provenance() {
    github_sys!(Sys, |request: OidcRequest<'_>| {
        assert_eq!(request.url, "https://registry.npmjs.org/-/package/@scope%2fpkg/visibility");
        assert!(matches!(request.method, OidcMethod::Get));
        assert_eq!(request.authorization, "Bearer auth");
        assert_eq!(request.timeout_ms, Some(40_000));
        Ok(OidcResponse { ok: true, status: 200, body: r#"{"public":true}"#.to_owned() })
    });

    let token = id_token(&repository_visibility("public"));
    let result = determine_provenance::<Sys>(
        "auth",
        &token,
        "@scope/pkg",
        REGISTRY,
        &OidcHttpOptions { fetch_timeout: Some(40_000), ..OidcHttpOptions::default() },
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

#[tokio::test]
async fn id_token_with_an_empty_payload_is_malformed() {
    github_sys!(Sys, |_: OidcRequest<'_>| unreachable!("no request for a malformed token"));

    let err = determine::<Sys>("header.").await.unwrap_err();
    assert!(matches!(err, DetermineProvenanceError::Provenance(ProvenanceError::MalformedIdToken)));
}

#[tokio::test]
async fn missing_public_field_yields_no_provenance() {
    github_sys!(Sys, |_: OidcRequest<'_>| Ok(OidcResponse {
        ok: true,
        status: 200,
        body: "{}".to_owned(),
    }));

    let result = determine::<Sys>(&id_token(&repository_visibility("public")))
        .await
        .unwrap();
    assert_eq!(result, None);
}

#[tokio::test]
async fn public_gitlab_project_enables_provenance() {
    ci_sys!(Sys, [("GITLAB_CI", "true"), ("SIGSTORE_ID_TOKEN", "token")], |_: OidcRequest<'_>| Ok(
        OidcResponse { ok: true, status: 200, body: r#"{"public":true}"#.to_owned() }
    ));

    let result = determine::<Sys>(&id_token(&project_visibility("public")))
        .await
        .unwrap();
    assert_eq!(result, Some(true));
}

#[tokio::test]
async fn private_gitlab_project_is_insufficient_information() {
    ci_sys!(
        Sys,
        [("GITLAB_CI", "true"), ("SIGSTORE_ID_TOKEN", "token")],
        |_: OidcRequest<'_>| unreachable!("visibility is not probed without public CI")
    );

    let err = determine::<Sys>(&id_token(&project_visibility("private"))).await.unwrap_err();
    assert!(matches!(
        err,
        DetermineProvenanceError::Provenance(ProvenanceError::InsufficientInformation)
    ));
}

#[tokio::test]
async fn gitlab_without_sigstore_id_token_is_insufficient_information() {
    ci_sys!(Sys, [("GITLAB_CI", "true")], |_: OidcRequest<'_>| unreachable!(
        "visibility is not probed without SIGSTORE_ID_TOKEN"
    ));

    let err = determine::<Sys>(&id_token(&project_visibility("public"))).await.unwrap_err();
    assert!(matches!(
        err,
        DetermineProvenanceError::Provenance(ProvenanceError::InsufficientInformation)
    ));
}

/// A GitHub repository-visibility claim does not satisfy the GitLab check.
#[tokio::test]
async fn gitlab_ignores_the_github_visibility_claim() {
    ci_sys!(
        Sys,
        [("GITLAB_CI", "true"), ("SIGSTORE_ID_TOKEN", "token")],
        |_: OidcRequest<'_>| unreachable!("visibility is not probed without public CI")
    );

    let err = determine::<Sys>(&id_token(&repository_visibility("public")))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        DetermineProvenanceError::Provenance(ProvenanceError::InsufficientInformation)
    ));
}

#[test]
fn visibility_failure_message_names_the_package_registry_and_reason() {
    let message = |body: &str, status: u16| {
        ProvenanceError::failed_to_fetch_visibility(
            body,
            status,
            "@pnpm/test-package",
            "https://registry.npmjs.org",
        )
        .to_string()
    };
    let prefix = "Failed to fetch visibility for package @pnpm/test-package from registry https://registry.npmjs.org due to";

    assert_eq!(
        message(r#"{"code":"NOT_FOUND","message":"Package not found"}"#, 404),
        format!("{prefix} NOT_FOUND: Package not found (status code 404)"),
    );
    assert_eq!(
        message(r#"{"code":"UNAUTHORIZED"}"#, 401),
        format!("{prefix} UNAUTHORIZED (status code 401)"),
    );
    assert_eq!(
        message(r#"{"message":"Internal server error"}"#, 500),
        format!("{prefix} Internal server error (status code 500)"),
    );
    assert_eq!(message("{}", 503), format!("{prefix} an unknown error (status code 503)"));
    assert_eq!(
        message("<html>Bad Gateway</html>", 502),
        format!("{prefix} an unknown error (status code 502)"),
    );
}
