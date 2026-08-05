use std::{cell::Cell, time::Duration};

use super::{
    ProvenanceGenError, SignProvenance, SignedProvenance, build_statement, fetch_sigstore_token,
    generate_provenance, github_statement, gitlab_statement, npm_purl, sign_with_retry,
    with_sign_deadline,
};
use crate::{
    capabilities::{Clock, EnvVar, OidcFetch, OidcFetchError, OidcRequest, OidcResponse},
    oidc::OidcHttpOptions,
};
use pacquet_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn purl_encodes_only_the_leading_scope_at() {
    assert_eq!(npm_purl("pkg", "1.0.0"), "pkg:npm/pkg@1.0.0");
    assert_eq!(npm_purl("@scope/pkg", "1.2.3"), "pkg:npm/%40scope/pkg@1.2.3");
}

/// A fake environment that mirrors the variables GitHub Actions sets, so the
/// statement builder can be exercised without a real CI runner.
struct GhEnv;

impl EnvVar for GhEnv {
    fn var(name: &str) -> Option<String> {
        let value = match name {
            "GITHUB_SERVER_URL" => "https://github.com",
            "GITHUB_REPOSITORY" => "pnpm/pnpm",
            "GITHUB_REPOSITORY_ID" => "123",
            "GITHUB_REPOSITORY_OWNER_ID" => "456",
            "GITHUB_WORKFLOW_REF" => "pnpm/pnpm/.github/workflows/release.yml@refs/heads/main",
            "GITHUB_EVENT_NAME" => "push",
            "GITHUB_REF" => "refs/heads/main",
            "GITHUB_SHA" => "abc123",
            "GITHUB_RUN_ID" => "42",
            "GITHUB_RUN_ATTEMPT" => "1",
            "RUNNER_ENVIRONMENT" => "github-hosted",
            _ => return None,
        };
        Some(value.to_owned())
    }
}

#[test]
fn github_statement_shapes_the_slsa_v1_predicate() {
    let subject = json!([{ "name": "pkg:npm/pkg@1.0.0", "digest": { "sha512": "deadbeef" } }]);
    let statement = github_statement::<GhEnv>(&subject);

    assert_eq!(statement["_type"], "https://in-toto.io/Statement/v1");
    assert_eq!(statement["predicateType"], "https://slsa.dev/provenance/v1");
    let workflow = &statement["predicate"]["buildDefinition"]["externalParameters"]["workflow"];
    // GITHUB_WORKFLOW_REF has the `owner/repo/` prefix stripped, then splits on `@`.
    assert_eq!(workflow["path"], ".github/workflows/release.yml");
    assert_eq!(workflow["ref"], "refs/heads/main");
    assert_eq!(workflow["repository"], "https://github.com/pnpm/pnpm");
    let resolved = &statement["predicate"]["buildDefinition"]["resolvedDependencies"][0];
    assert_eq!(resolved["uri"], "git+https://github.com/pnpm/pnpm@refs/heads/main");
    assert_eq!(resolved["digest"]["gitCommit"], "abc123");
    let run = &statement["predicate"]["runDetails"];
    assert_eq!(run["builder"]["id"], "https://github.com/actions/runner/github-hosted");
    assert_eq!(
        run["metadata"]["invocationId"],
        "https://github.com/pnpm/pnpm/actions/runs/42/attempts/1",
    );
}

/// A fake environment that mirrors the variables GitLab CI sets, so the
/// statement builder can be exercised without a real CI runner.
struct GlEnv;

impl EnvVar for GlEnv {
    fn var(name: &str) -> Option<String> {
        let value = match name {
            "CI_PROJECT_URL" => "https://gitlab.com/pnpm/pnpm",
            "CI_RUNNER_ID" => "77",
            "CI_COMMIT_SHA" => "abc123",
            "CI_JOB_NAME" => "publish",
            "CI_JOB_ID" => "555",
            "CI_PIPELINE_ID" => "999",
            "CI_CONFIG_PATH" => ".gitlab-ci.yml",
            "CI_JOB_URL" => "https://gitlab.com/pnpm/pnpm/-/jobs/555",
            _ => return None,
        };
        Some(value.to_owned())
    }
}

#[test]
fn gitlab_statement_shapes_the_slsa_v02_predicate() {
    let subject = json!([{ "name": "pkg:npm/pkg@1.0.0", "digest": { "sha512": "deadbeef" } }]);
    let statement = gitlab_statement::<GlEnv>(&subject);

    assert_eq!(statement["_type"], "https://in-toto.io/Statement/v0.1");
    assert_eq!(statement["predicateType"], "https://slsa.dev/provenance/v0.2");
    let predicate = &statement["predicate"];
    assert_eq!(predicate["builder"]["id"], "https://gitlab.com/pnpm/pnpm/-/runners/77");
    let config_source = &predicate["invocation"]["configSource"];
    assert_eq!(config_source["uri"], "git+https://gitlab.com/pnpm/pnpm");
    assert_eq!(config_source["digest"]["sha1"], "abc123");
    assert_eq!(config_source["entryPoint"], "publish");
    assert_eq!(
        predicate["metadata"]["buildInvocationId"],
        "https://gitlab.com/pnpm/pnpm/-/jobs/555",
    );
    assert_eq!(predicate["materials"][0]["uri"], "git+https://gitlab.com/pnpm/pnpm");
    assert_eq!(predicate["materials"][0]["digest"]["sha1"], "abc123");
}

/// A GitHub-Actions provider that reuses [`GhEnv`]'s variable bodies.
struct GhProvider;

impl EnvVar for GhProvider {
    fn var(name: &str) -> Option<String> {
        match name {
            "GITHUB_ACTIONS" => Some("true".to_owned()),
            _ => GhEnv::var(name),
        }
    }
}

/// A GitLab-CI provider that reuses [`GlEnv`]'s variable bodies.
struct GlProvider;

impl EnvVar for GlProvider {
    fn var(name: &str) -> Option<String> {
        match name {
            "GITLAB_CI" => Some("true".to_owned()),
            _ => GlEnv::var(name),
        }
    }
}

/// A provider that is neither GitHub Actions nor GitLab CI.
struct NoProvider;

impl EnvVar for NoProvider {
    fn var(_: &str) -> Option<String> {
        None
    }
}

#[test]
fn build_statement_dispatches_to_github_on_github_actions() {
    let subject = json!([{ "name": "pkg:npm/pkg@1.0.0", "digest": { "sha512": "deadbeef" } }]);
    let statement = build_statement::<GhProvider>(&subject).unwrap();
    assert_eq!(statement["_type"], "https://in-toto.io/Statement/v1");
}

#[test]
fn build_statement_dispatches_to_gitlab_on_gitlab_ci() {
    let subject = json!([{ "name": "pkg:npm/pkg@1.0.0", "digest": { "sha512": "deadbeef" } }]);
    let statement = build_statement::<GlProvider>(&subject).unwrap();
    assert_eq!(statement["_type"], "https://in-toto.io/Statement/v0.1");
}

#[test]
fn build_statement_rejects_unsupported_provider() {
    let subject = json!([{ "name": "pkg:npm/pkg@1.0.0", "digest": { "sha512": "deadbeef" } }]);
    let err = build_statement::<NoProvider>(&subject).unwrap_err();
    assert!(matches!(err, ProvenanceGenError::UnsupportedProvider));
}

/// GitHub-Actions sigstore-token fake: request-token env plus a `fetch` body.
macro_rules! github_sigstore_sys {
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
async fn fetch_sigstore_token_uses_github_request_token() {
    github_sigstore_sys!(Sys, |request: OidcRequest<'_>| {
        // The sigstore audience drives the request-token query parameter.
        assert!(request.url.contains("audience=sigstore"));
        Ok(OidcResponse {
            ok: true,
            status: 200,
            body: r#"{"value":"gh-sigstore-token"}"#.to_owned(),
        })
    });

    let token =
        fetch_sigstore_token::<Sys, SilentReporter>(&OidcHttpOptions::default()).await.unwrap();
    assert_eq!(token, "gh-sigstore-token");
}

#[tokio::test]
async fn fetch_sigstore_token_reads_gitlab_env_token() {
    struct Sys;
    impl Clock for Sys {
        fn now_ms() -> u64 {
            unreachable!("no request when GitLab forwards the token via env")
        }
    }
    impl EnvVar for Sys {
        fn var(name: &str) -> Option<String> {
            match name {
                "GITLAB_CI" => Some("true".to_owned()),
                "SIGSTORE_ID_TOKEN" => Some("gl-sigstore-token".to_owned()),
                _ => None,
            }
        }
    }
    impl OidcFetch for Sys {
        async fn fetch(_: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
            unreachable!("no request when GitLab forwards the token via env")
        }
    }

    let token =
        fetch_sigstore_token::<Sys, SilentReporter>(&OidcHttpOptions::default()).await.unwrap();
    assert_eq!(token, "gl-sigstore-token");
}

#[tokio::test]
async fn fetch_sigstore_token_errors_when_gitlab_token_missing() {
    struct Sys;
    impl Clock for Sys {
        fn now_ms() -> u64 {
            unreachable!()
        }
    }
    impl EnvVar for Sys {
        fn var(name: &str) -> Option<String> {
            match name {
                "GITLAB_CI" => Some("true".to_owned()),
                _ => None,
            }
        }
    }
    impl OidcFetch for Sys {
        async fn fetch(_: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
            unreachable!()
        }
    }

    let err =
        fetch_sigstore_token::<Sys, SilentReporter>(&OidcHttpOptions::default()).await.unwrap_err();
    assert!(matches!(err, ProvenanceGenError::GitLabMissingToken));
}

/// A GitHub-Actions host wired for the whole `generate_provenance` path: the
/// build-statement env ([`GhEnv`]), the id-token request env, a `fetch` that
/// answers the `sigstore`-audience token request, and a fake signer that
/// returns a canned bundle so no real Fulcio / Rekor call is made.
struct GhSignSys;
impl Clock for GhSignSys {
    fn now_ms() -> u64 {
        0
    }
}
impl EnvVar for GhSignSys {
    fn var(name: &str) -> Option<String> {
        match name {
            "GITHUB_ACTIONS" => Some("true".to_owned()),
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN" => Some("request-token".to_owned()),
            "ACTIONS_ID_TOKEN_REQUEST_URL" => Some("https://github.example/token".to_owned()),
            _ => GhEnv::var(name),
        }
    }
}
impl OidcFetch for GhSignSys {
    async fn fetch(request: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
        assert!(request.url.contains("audience=sigstore"), "unexpected request: {}", request.url);
        Ok(OidcResponse { ok: true, status: 200, body: r#"{"value":"sigstore-token"}"#.to_owned() })
    }
}
impl SignProvenance for GhSignSys {
    async fn sign_statement(
        jwt: &str,
        statement: &[u8],
        timeout: Option<Duration>,
    ) -> Result<SignedProvenance, ProvenanceGenError> {
        assert_eq!(jwt, "sigstore-token", "the sigstore-audience token is passed to the signer");
        assert!(!statement.is_empty(), "a serialized statement is signed");
        assert_eq!(timeout, None, "no fetch-timeout was configured");
        Ok(SignedProvenance {
            media_type: "application/vnd.dev.sigstore.bundle.v0.3+json".to_owned(),
            data: r#"{"mediaType":"application/vnd.dev.sigstore.bundle.v0.3+json"}"#.to_owned(),
        })
    }
}

/// `generate_provenance` builds the statement, fetches the sigstore token, and
/// hands both to the signer, wrapping the result as the `.sigstore` attachment.
#[tokio::test]
async fn generate_provenance_builds_the_signed_attachment() {
    let attachment = generate_provenance::<GhSignSys, SilentReporter>(
        "@scope/pkg",
        "1.2.3",
        b"tarball-bytes",
        &OidcHttpOptions::default(),
    )
    .await
    .expect("provenance generation succeeds");

    assert_eq!(attachment.bundle_name, "@scope/pkg-1.2.3.sigstore");
    assert_eq!(attachment.content_type, "application/vnd.dev.sigstore.bundle.v0.3+json");
    assert_eq!(attachment.data, r#"{"mediaType":"application/vnd.dev.sigstore.bundle.v0.3+json"}"#);
}

/// The configured `fetch-timeout` (milliseconds) reaches the signer as its
/// per-attempt deadline.
#[tokio::test]
async fn generate_provenance_forwards_fetch_timeout_to_the_signer() {
    struct TimeoutSys;
    impl Clock for TimeoutSys {
        fn now_ms() -> u64 {
            0
        }
    }
    impl EnvVar for TimeoutSys {
        fn var(name: &str) -> Option<String> {
            GhSignSys::var(name)
        }
    }
    impl OidcFetch for TimeoutSys {
        async fn fetch(request: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
            GhSignSys::fetch(request).await
        }
    }
    impl SignProvenance for TimeoutSys {
        async fn sign_statement(
            _: &str,
            _: &[u8],
            timeout: Option<Duration>,
        ) -> Result<SignedProvenance, ProvenanceGenError> {
            assert_eq!(timeout, Some(Duration::from_millis(1234)));
            Ok(SignedProvenance { media_type: "bundle".to_owned(), data: "{}".to_owned() })
        }
    }

    let options = OidcHttpOptions { fetch_timeout: Some(1234), ..OidcHttpOptions::default() };
    generate_provenance::<TimeoutSys, SilentReporter>("pkg", "1.0.0", b"tarball", &options)
        .await
        .expect("provenance generation succeeds");
}

/// A signer failure propagates as the publish-facing provenance error.
#[tokio::test]
async fn generate_provenance_surfaces_a_signer_failure() {
    struct FailingSigner;
    impl Clock for FailingSigner {
        fn now_ms() -> u64 {
            0
        }
    }
    impl EnvVar for FailingSigner {
        fn var(name: &str) -> Option<String> {
            GhSignSys::var(name)
        }
    }
    impl OidcFetch for FailingSigner {
        async fn fetch(request: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
            GhSignSys::fetch(request).await
        }
    }
    impl SignProvenance for FailingSigner {
        async fn sign_statement(
            _: &str,
            _: &[u8],
            _: Option<Duration>,
        ) -> Result<SignedProvenance, ProvenanceGenError> {
            Err(ProvenanceGenError::Sign { source: "fulcio rejected the request".to_owned() })
        }
    }

    let err = generate_provenance::<FailingSigner, SilentReporter>(
        "pkg",
        "1.0.0",
        b"tarball",
        &OidcHttpOptions::default(),
    )
    .await
    .expect_err("a signer failure aborts provenance generation");
    assert!(matches!(err, ProvenanceGenError::Sign { .. }), "got {err:?}");
}

/// A zero-delay policy so the retry tests don't sleep.
const INSTANT_RETRIES: pacquet_network::RetryOpts = pacquet_network::RetryOpts {
    retries: 2,
    factor: 2,
    min_timeout: Duration::ZERO,
    max_timeout: Duration::ZERO,
};

#[tokio::test]
async fn with_sign_deadline_times_out_a_hung_attempt() {
    let err = with_sign_deadline(Duration::ZERO, std::future::pending())
        .await
        .expect_err("a hung exchange hits the deadline");
    assert!(matches!(err, ProvenanceGenError::Sign { .. }), "got {err:?}");
}

#[tokio::test]
async fn with_sign_deadline_passes_a_completed_attempt_through() {
    let signed = with_sign_deadline(Duration::from_secs(5), async {
        Ok(SignedProvenance { media_type: "bundle".to_owned(), data: "{}".to_owned() })
    })
    .await
    .expect("a completed attempt is returned unchanged");
    assert_eq!(signed.data, "{}");
}

#[tokio::test]
async fn sign_with_retry_recovers_from_transient_failures() {
    let attempts = Cell::new(0u32);
    let signed = sign_with_retry(INSTANT_RETRIES, || {
        let attempt = attempts.get();
        attempts.set(attempt + 1);
        async move {
            if attempt < 2 {
                return Err(ProvenanceGenError::Sign {
                    source: "Failed to get timestamp".to_owned(),
                });
            }
            Ok(SignedProvenance { media_type: "bundle".to_owned(), data: "{}".to_owned() })
        }
    })
    .await
    .expect("the third attempt succeeds");
    assert_eq!(signed.data, "{}");
    assert_eq!(attempts.get(), 3);
}

#[tokio::test]
async fn sign_with_retry_surfaces_the_final_failure_once_retries_are_exhausted() {
    let attempts = Cell::new(0u32);
    let err = sign_with_retry(INSTANT_RETRIES, || {
        attempts.set(attempts.get() + 1);
        async { Err(ProvenanceGenError::Sign { source: "Failed to get timestamp".to_owned() }) }
    })
    .await
    .expect_err("every attempt fails");
    assert!(matches!(err, ProvenanceGenError::Sign { .. }), "got {err:?}");
    assert_eq!(attempts.get(), INSTANT_RETRIES.retries + 1);
}

#[tokio::test]
async fn fetch_sigstore_token_rejects_unsupported_provider() {
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
            unreachable!()
        }
    }

    let err =
        fetch_sigstore_token::<Sys, SilentReporter>(&OidcHttpOptions::default()).await.unwrap_err();
    assert!(matches!(err, ProvenanceGenError::UnsupportedProvider));
}
