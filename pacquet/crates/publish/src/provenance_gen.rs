//! Generate an npm-style provenance attestation bundle and sign it with
//! sigstore (Fulcio keyless certificate + Rekor transparency log) via the
//! [`sigstore_sign`] crate.
//!
//! This ports the in-toto SLSA *statement* construction from
//! [`generateProvenance`](https://github.com/pnpm/pnpm/blob/54c5c0e028/pnpm11/releasing/commands/src/publish/oidc/provenance.ts)
//! /
//! [`libnpmpublish`'s provenance.js](https://github.com/npm/cli/blob/latest/node_modules/libnpmpublish/lib/provenance.js)
//! and the `_attachments[*.sigstore]` wiring from `libnpmpublish`'s
//! `publish.js`. The signing itself is delegated to `sigstore_sign`
//! unmodified, so the emitted bundle is whatever that crate produces (a v0.3
//! DSSE bundle with a single certificate and a `dsse` Rekor entry) — which is
//! *not* byte-identical to npm's legacy `legacyCompatibility: true` bundle. The
//! compiled output is meant to be validated against a real registry.

use pacquet_diagnostics::miette::{self, Diagnostic};
use pacquet_reporter::Reporter;
use serde_json::{Value, json};
use sha2::{Digest, Sha512};
use sigstore_sign::{SigningContext, oidc::IdentityToken};
use url::Url;

use crate::{
    capabilities::{CiInfo, Clock, EnvVar, OidcFetch, OidcFetchError, OidcMethod, OidcRequest},
    global_log::global_info,
    oidc::OidcHttpOptions,
};

const INTOTO_STATEMENT_V1_TYPE: &str = "https://in-toto.io/Statement/v1";
const INTOTO_STATEMENT_V01_TYPE: &str = "https://in-toto.io/Statement/v0.1";
const SLSA_PREDICATE_V1_TYPE: &str = "https://slsa.dev/provenance/v1";
const SLSA_PREDICATE_V02_TYPE: &str = "https://slsa.dev/provenance/v0.2";
const GITHUB_BUILDER_ID_PREFIX: &str = "https://github.com/actions/runner";
const GITHUB_BUILD_TYPE: &str =
    "https://slsa-framework.github.io/github-actions-buildtypes/workflow/v1";
const GITLAB_BUILD_TYPE_PREFIX: &str = "https://github.com/npm/cli/gitlab";
const GITLAB_BUILD_TYPE_VERSION: &str = "v0alpha1";
const SIGSTORE_AUDIENCE: &str = "sigstore";

/// The `_attachments["<name>-<version>.sigstore"]` entry to splice into the
/// publish document. `data` is the serialized bundle JSON (stored verbatim,
/// not base64, matching `libnpmpublish`).
pub struct ProvenanceAttachment {
    pub bundle_name: String,
    pub content_type: String,
    pub data: String,
}

/// Build the SLSA provenance statement for the package, fetch a `sigstore`
/// audience OIDC token from the CI environment, and sign the statement with
/// sigstore. Returns the attachment to add to the publish document.
pub async fn generate_provenance<Sys, Reporter>(
    package_name: &str,
    package_version: &str,
    tarball_data: &[u8],
    options: &OidcHttpOptions,
) -> Result<ProvenanceAttachment, ProvenanceGenError>
where
    Sys: EnvVar + CiInfo + Clock + OidcFetch,
    Reporter: self::Reporter,
{
    let sha512_hex = format!("{:x}", Sha512::digest(tarball_data));
    let subject = json!([{
        "name": npm_purl(package_name, package_version),
        "digest": { "sha512": sha512_hex },
    }]);

    let statement = build_statement::<Sys>(&subject)?;
    let statement_bytes = serde_json::to_vec(&statement).expect("serialize provenance statement");

    let jwt = fetch_sigstore_token::<Sys, Reporter>(options).await?;
    let token = IdentityToken::from_jwt(&jwt)
        .map_err(|source| ProvenanceGenError::IdentityToken { source: source.to_string() })?;

    let bundle = SigningContext::production()
        .signer(token)
        .sign_raw_statement(&statement_bytes)
        .await
        .map_err(|source| ProvenanceGenError::Sign { source: source.to_string() })?;

    global_info::<Reporter>("Signed provenance statement with source and build information");

    let data = serde_json::to_string(&bundle).expect("serialize sigstore bundle");
    Ok(ProvenanceAttachment {
        bundle_name: format!("{package_name}-{package_version}.sigstore"),
        content_type: bundle.media_type,
        data,
    })
}

/// Format the npm package coordinate as a PURL, mirroring `npm-package-arg`'s
/// `toPurl`: `pkg:npm/<name>@<version>` with only a leading scope `@`
/// percent-encoded to `%40` (the `/` is left intact).
fn npm_purl(name: &str, version: &str) -> String {
    let encoded =
        name.strip_prefix('@').map_or_else(|| name.to_owned(), |rest| format!("%40{rest}"));
    format!("pkg:npm/{encoded}@{version}")
}

/// Build the in-toto SLSA statement from the CI environment. GitHub Actions
/// emits a Statement v1 + SLSA predicate v1; GitLab CI emits a Statement v0.1 +
/// SLSA predicate v0.2. Ports `generateProvenance`.
fn build_statement<Sys: EnvVar + CiInfo>(subject: &Value) -> Result<Value, ProvenanceGenError> {
    if Sys::github_actions() {
        return Ok(github_statement::<Sys>(subject));
    }
    if Sys::gitlab() {
        return Ok(gitlab_statement::<Sys>(subject));
    }
    Err(ProvenanceGenError::UnsupportedProvider)
}

fn github_statement<Sys: EnvVar>(subject: &Value) -> Value {
    let server_url = env::<Sys>("GITHUB_SERVER_URL");
    let repository = env::<Sys>("GITHUB_REPOSITORY");
    let workflow_ref = env::<Sys>("GITHUB_WORKFLOW_REF");
    // GITHUB_WORKFLOW_REF is `owner/repo/path@ref`; strip the `owner/repo/`
    // prefix, then split the remainder on `@` into path and ref.
    let relative_ref =
        workflow_ref.strip_prefix(&format!("{repository}/")).unwrap_or(&workflow_ref);
    let (workflow_path, workflow_ref_only) =
        relative_ref.split_once('@').unwrap_or((relative_ref, ""));

    json!({
        "_type": INTOTO_STATEMENT_V1_TYPE,
        "subject": subject,
        "predicateType": SLSA_PREDICATE_V1_TYPE,
        "predicate": {
            "buildDefinition": {
                "buildType": GITHUB_BUILD_TYPE,
                "externalParameters": {
                    "workflow": {
                        "ref": workflow_ref_only,
                        "repository": format!("{server_url}/{repository}"),
                        "path": workflow_path,
                    },
                },
                "internalParameters": {
                    "github": {
                        "event_name": env::<Sys>("GITHUB_EVENT_NAME"),
                        "repository_id": env::<Sys>("GITHUB_REPOSITORY_ID"),
                        "repository_owner_id": env::<Sys>("GITHUB_REPOSITORY_OWNER_ID"),
                    },
                },
                "resolvedDependencies": [{
                    "uri": format!("git+{server_url}/{repository}@{}", env::<Sys>("GITHUB_REF")),
                    "digest": { "gitCommit": env::<Sys>("GITHUB_SHA") },
                }],
            },
            "runDetails": {
                "builder": {
                    "id": format!("{GITHUB_BUILDER_ID_PREFIX}/{}", env::<Sys>("RUNNER_ENVIRONMENT")),
                },
                "metadata": {
                    "invocationId": format!(
                        "{server_url}/{repository}/actions/runs/{}/attempts/{}",
                        env::<Sys>("GITHUB_RUN_ID"),
                        env::<Sys>("GITHUB_RUN_ATTEMPT"),
                    ),
                },
            },
        },
    })
}

fn gitlab_statement<Sys: EnvVar>(subject: &Value) -> Value {
    let project_url = env::<Sys>("CI_PROJECT_URL");
    json!({
        "_type": INTOTO_STATEMENT_V01_TYPE,
        "subject": subject,
        "predicateType": SLSA_PREDICATE_V02_TYPE,
        "predicate": {
            "buildType": format!("{GITLAB_BUILD_TYPE_PREFIX}/{GITLAB_BUILD_TYPE_VERSION}"),
            "builder": {
                "id": format!("{project_url}/-/runners/{}", env::<Sys>("CI_RUNNER_ID")),
            },
            "invocation": {
                "configSource": {
                    "uri": format!("git+{project_url}"),
                    "digest": { "sha1": env::<Sys>("CI_COMMIT_SHA") },
                    "entryPoint": env::<Sys>("CI_JOB_NAME"),
                },
                "environment": {
                    "name": env::<Sys>("CI_RUNNER_DESCRIPTION"),
                    "architecture": env::<Sys>("CI_RUNNER_EXECUTABLE_ARCH"),
                    "server": env::<Sys>("CI_SERVER_URL"),
                    "project": env::<Sys>("CI_PROJECT_PATH"),
                    "job": { "id": env::<Sys>("CI_JOB_ID") },
                    "pipeline": {
                        "id": env::<Sys>("CI_PIPELINE_ID"),
                        "ref": env::<Sys>("CI_CONFIG_PATH"),
                    },
                },
            },
            "metadata": {
                "buildInvocationId": env::<Sys>("CI_JOB_URL"),
                "completeness": { "parameters": true, "environment": true, "materials": false },
                "reproducible": false,
            },
            "materials": [{
                "uri": format!("git+{project_url}"),
                "digest": { "sha1": env::<Sys>("CI_COMMIT_SHA") },
            }],
        },
    })
}

/// Fetch an OIDC token with the `sigstore` audience for Fulcio. GitHub Actions
/// is driven through its request-token endpoint; GitLab forwards the token via
/// `SIGSTORE_ID_TOKEN`. Mirrors sigstore-js's `CIContextProvider('sigstore')`.
async fn fetch_sigstore_token<Sys, Reporter>(
    options: &OidcHttpOptions,
) -> Result<String, ProvenanceGenError>
where
    Sys: EnvVar + CiInfo + Clock + OidcFetch,
    Reporter: self::Reporter,
{
    if Sys::github_actions() {
        let (Some(request_token), Some(request_url)) = (
            truthy_env::<Sys>("ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
            truthy_env::<Sys>("ACTIONS_ID_TOKEN_REQUEST_URL"),
        ) else {
            return Err(ProvenanceGenError::GitHubIncorrectPermissions);
        };
        let mut url = Url::parse(&request_url).map_err(ProvenanceGenError::InvalidRequestUrl)?;
        url.query_pairs_mut().append_pair("audience", SIGSTORE_AUDIENCE);
        let url = url.to_string();
        let authorization = format!("Bearer {request_token}");
        let start = Sys::now_ms();
        let response = Sys::fetch(OidcRequest {
            method: OidcMethod::Get,
            url: &url,
            authorization: &authorization,
            timeout_ms: options.fetch_timeout,
        })
        .await
        .map_err(ProvenanceGenError::Fetch)?;
        let elapsed = Sys::now_ms().saturating_sub(start);
        global_info::<Reporter>(&format!("GET {url} {} {elapsed}ms", response.status));
        if !response.ok {
            return Err(ProvenanceGenError::GitHubInvalidResponse);
        }
        let json: Value = serde_json::from_str(&response.body)
            .map_err(|source| ProvenanceGenError::TokenJson { source: source.to_string() })?;
        return json
            .get("value")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or(ProvenanceGenError::GitHubInvalidResponse);
    }

    if Sys::gitlab() {
        return truthy_env::<Sys>("SIGSTORE_ID_TOKEN")
            .ok_or(ProvenanceGenError::GitLabMissingToken);
    }

    Err(ProvenanceGenError::UnsupportedProvider)
}

fn env<Sys: EnvVar>(name: &str) -> String {
    Sys::var(name).unwrap_or_default()
}

fn truthy_env<Sys: EnvVar>(name: &str) -> Option<String> {
    Sys::var(name).filter(|value| !value.is_empty())
}

/// Failure surface of [`generate_provenance`].
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum ProvenanceGenError {
    #[display("Automatic provenance generation is not supported for this CI provider")]
    #[diagnostic(code(ERR_PNPM_PROVENANCE_UNSUPPORTED_PROVIDER))]
    UnsupportedProvider,

    #[display("Incorrect permissions for idToken within GitHub Workflows")]
    #[diagnostic(code(ERR_PNPM_ID_TOKEN_GITHUB_WORKFLOW_INCORRECT_PERMISSIONS))]
    GitHubIncorrectPermissions,

    #[display("Failed to fetch sigstore idToken from GitHub: received an invalid response")]
    #[diagnostic(code(ERR_PNPM_ID_TOKEN_GITHUB_INVALID_RESPONSE))]
    GitHubInvalidResponse,

    #[display(
        r#"Provenance generation in GitLab CI requires "SIGSTORE_ID_TOKEN" with "sigstore" audience"#
    )]
    #[diagnostic(code(ERR_PNPM_PROVENANCE_GITLAB_MISSING_TOKEN))]
    GitLabMissingToken,

    #[display("Failed to parse the sigstore idToken response: {source}")]
    #[diagnostic(code(ERR_PNPM_PROVENANCE_TOKEN_JSON))]
    TokenJson {
        #[error(not(source))]
        source: String,
    },

    #[display("invalid id-token request URL: {_0}")]
    InvalidRequestUrl(url::ParseError),

    #[display("{_0}")]
    Fetch(OidcFetchError),

    #[display("invalid sigstore identity token: {source}")]
    #[diagnostic(code(ERR_PNPM_PROVENANCE_IDENTITY_TOKEN))]
    IdentityToken {
        #[error(not(source))]
        source: String,
    },

    #[display("failed to sign the provenance statement with sigstore: {source}")]
    #[diagnostic(code(ERR_PNPM_PROVENANCE_SIGN))]
    Sign {
        #[error(not(source))]
        source: String,
    },
}

#[cfg(test)]
mod tests;
