//! Generate an npm-style provenance attestation bundle and sign it with
//! sigstore (Fulcio keyless certificate + Rekor transparency log) via the
//! [`sigstore_sign`] crate.
//!
//! It builds the in-toto SLSA *statement* and delegates the signing to the
//! `sigstore_sign` crate unmodified, so the emitted bundle is whatever that
//! crate produces: a **sigstore bundle v0.3** (single certificate, `dsse`
//! Rekor entry) — not the legacy v0.2 form (`x509CertificateChain`, `intoto`
//! Rekor entry), which pacquet deliberately does not reproduce.
//! The npm registry accepts the v0.3 bundle: a package published this way was
//! verified end-to-end against npmjs.com (`@pnpm.e2e/testing-provenance2`,
//! recorded in the Rekor transparency log), so the modern bundle is sufficient
//! and no legacy-compatibility path is needed.

use std::time::Duration;

use pacquet_diagnostics::miette::{self, Diagnostic};
use pacquet_network::{RetryOpts, redact_url_credentials};
use pacquet_reporter::Reporter;
use serde_json::{Value, json};
use sha2::{Digest, Sha512};
use sigstore_sign::{SigningContext, oidc::IdentityToken};

use crate::{
    capabilities::{Clock, EnvVar, Host, OidcFetch, OidcFetchError},
    global_log::global_info,
    oidc::{
        GitHubRequestTokenError, OidcHttpOptions, github_request_token, is_github_actions,
        is_gitlab, truthy_env,
    },
};

const IN_TOTO_STATEMENT_V1_TYPE: &str = "https://in-toto.io/Statement/v1";
const IN_TOTO_STATEMENT_V01_TYPE: &str = "https://in-toto.io/Statement/v0.1";
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
/// not base64).
#[derive(Debug)]
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
    Sys: EnvVar + Clock + OidcFetch + SignProvenance,
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
    let timeout = options.fetch_timeout.map(Duration::from_millis);
    let signed = Sys::sign_statement(&jwt, &statement_bytes, timeout).await?;

    global_info::<Reporter>("Signed provenance statement with source and build information");

    Ok(ProvenanceAttachment {
        bundle_name: format!("{package_name}-{package_version}.sigstore"),
        content_type: signed.media_type,
        data: signed.data,
    })
}

/// Sign the in-toto SLSA statement into a sigstore bundle. The production
/// [`Host`] impl performs the real keyless sigstore exchange (Fulcio
/// certificate + Rekor transparency log) over the network; tests inject a fake
/// so [`generate_provenance`] runs offline and deterministically.
pub trait SignProvenance {
    /// Sign `statement` (the serialized in-toto statement) using `jwt`, the
    /// OIDC token minted for the `sigstore` audience. `timeout` caps each
    /// signing attempt (`fetch-timeout`); `None` falls back to the
    /// implementation's default, mirroring sigstore-js's `options.timeout`.
    fn sign_statement(
        jwt: &str,
        statement: &[u8],
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<SignedProvenance, ProvenanceGenError>>;
}

/// A signed sigstore bundle: its media type and the serialized bundle JSON
/// (stored verbatim in the publish document, not base64-encoded).
#[derive(Debug)]
pub struct SignedProvenance {
    pub media_type: String,
    pub data: String,
}

impl SignProvenance for Host {
    async fn sign_statement(
        jwt: &str,
        statement: &[u8],
        timeout: Option<Duration>,
    ) -> Result<SignedProvenance, ProvenanceGenError> {
        let token = IdentityToken::from_jwt(jwt)
            .map_err(|source| ProvenanceGenError::IdentityToken { source: source.to_string() })?;
        let context = SigningContext::production();
        let deadline = timeout.unwrap_or(DEFAULT_SIGN_TIMEOUT);
        sign_with_retry(SIGN_RETRY_OPTS, || {
            with_sign_deadline(deadline, async {
                let bundle =
                    context.signer(token.clone()).sign_raw_statement(statement).await.map_err(
                        |source| ProvenanceGenError::Sign { source: source.to_string() },
                    )?;
                let data = serde_json::to_string(&bundle).expect("serialize sigstore bundle");
                Ok(SignedProvenance { media_type: bundle.media_type, data })
            })
        })
        .await
    }
}

/// The TypeScript CLI signs through sigstore-js, which wraps every Fulcio /
/// TSA / Rekor request in `make-fetch-happen` with its default retry policy
/// (2 retries, factor 2, 1 s floor), so a transient sigstore outage does not
/// abort the publish. The `sigstore_sign` crate issues each request exactly
/// once, so pacquet retries at the boundary it owns instead: the whole
/// signing exchange. Every step is idempotent (a fresh ephemeral key,
/// certificate, timestamp, and transparency-log entry per attempt), so
/// re-running it is safe.
const SIGN_RETRY_OPTS: RetryOpts = RetryOpts {
    retries: 2,
    factor: 2,
    min_timeout: Duration::from_secs(1),
    max_timeout: Duration::from_mins(1),
};

/// sigstore-js's `DEFAULT_TIMEOUT`, used only when the caller supplies no
/// `fetch-timeout` — the sigstore-rust clients set no request timeout of
/// their own, so without a deadline a hung connection stalls the publish
/// until the OS gives up on the socket.
const DEFAULT_SIGN_TIMEOUT: Duration = Duration::from_secs(5);

/// Cap one signing attempt at `deadline`, converting an elapsed timer into a
/// retryable [`ProvenanceGenError::Sign`].
async fn with_sign_deadline<Fut>(
    deadline: Duration,
    attempt: Fut,
) -> Result<SignedProvenance, ProvenanceGenError>
where
    Fut: Future<Output = Result<SignedProvenance, ProvenanceGenError>>,
{
    match tokio::time::timeout(deadline, attempt).await {
        Ok(result) => result,
        Err(_) => Err(ProvenanceGenError::Sign {
            source: format!(
                "no response from the sigstore signing exchange within {} ms",
                deadline.as_millis(),
            ),
        }),
    }
}

/// Run `attempt_fn` — one full signing exchange — retrying under
/// `retry_opts`'s exponential backoff until it succeeds or the retries are
/// exhausted.
async fn sign_with_retry<Fut>(
    retry_opts: RetryOpts,
    mut attempt_fn: impl FnMut() -> Fut,
) -> Result<SignedProvenance, ProvenanceGenError>
where
    Fut: Future<Output = Result<SignedProvenance, ProvenanceGenError>>,
{
    let mut attempt = 0;
    loop {
        match attempt_fn().await {
            Ok(signed) => return Ok(signed),
            Err(error) if attempt < retry_opts.retries => {
                let delay = retry_opts.delay_for(attempt);
                tracing::warn!(
                    target: "pacquet_publish::provenance",
                    error = %redact_url_credentials(&error.to_string()),
                    attempt = attempt + 1,
                    max_attempts = retry_opts.retries + 1,
                    ?delay,
                    "Signing the provenance statement failed; retrying after backoff",
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Format the npm package coordinate as a PURL
/// (`pkg:npm/<name>@<version>`), with only a leading scope `@`
/// percent-encoded to `%40` (the `/` is left intact).
fn npm_purl(name: &str, version: &str) -> String {
    let encoded =
        name.strip_prefix('@').map_or_else(|| name.to_owned(), |rest| format!("%40{rest}"));
    format!("pkg:npm/{encoded}@{version}")
}

/// Build the in-toto SLSA statement from the CI environment. GitHub Actions
/// emits a Statement v1 + SLSA predicate v1; GitLab CI emits a Statement v0.1 +
/// SLSA predicate v0.2.
fn build_statement<Sys: EnvVar>(subject: &Value) -> Result<Value, ProvenanceGenError> {
    if is_github_actions::<Sys>() {
        return Ok(github_statement::<Sys>(subject));
    }
    if is_gitlab::<Sys>() {
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
        "_type": IN_TOTO_STATEMENT_V1_TYPE,
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
        "_type": IN_TOTO_STATEMENT_V01_TYPE,
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
/// `SIGSTORE_ID_TOKEN`.
async fn fetch_sigstore_token<Sys, Reporter>(
    options: &OidcHttpOptions,
) -> Result<String, ProvenanceGenError>
where
    Sys: EnvVar + Clock + OidcFetch,
    Reporter: self::Reporter,
{
    if is_github_actions::<Sys>() {
        return github_request_token::<Sys, Reporter>(SIGSTORE_AUDIENCE, options)
            .await
            .map_err(Into::into);
    }

    if is_gitlab::<Sys>() {
        return truthy_env::<Sys>("SIGSTORE_ID_TOKEN")
            .ok_or(ProvenanceGenError::GitLabMissingToken);
    }

    Err(ProvenanceGenError::UnsupportedProvider)
}

fn env<Sys: EnvVar>(name: &str) -> String {
    Sys::var(name).unwrap_or_default()
}

impl From<GitHubRequestTokenError> for ProvenanceGenError {
    fn from(error: GitHubRequestTokenError) -> Self {
        match error {
            GitHubRequestTokenError::IncorrectPermissions => {
                ProvenanceGenError::GitHubIncorrectPermissions
            }
            GitHubRequestTokenError::InvalidRequestUrl(error) => {
                ProvenanceGenError::InvalidRequestUrl(error)
            }
            GitHubRequestTokenError::Fetch(error) => ProvenanceGenError::Fetch(error),
            // A non-2xx response and a missing `value` both surface as the same
            // "invalid response" error the inline fetch raised for either case.
            GitHubRequestTokenError::NotOk | GitHubRequestTokenError::MissingValue => {
                ProvenanceGenError::GitHubInvalidResponse
            }
            GitHubRequestTokenError::JsonParse(source) => ProvenanceGenError::TokenJson { source },
        }
    }
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
