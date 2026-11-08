//! The [`Host`] implementation of [`SignProvenance`]: the keyless Sigstore
//! exchange, bounded by a per-attempt deadline and retried.

use std::time::Duration;

#[cfg(not(target_family = "wasm"))]
use pnpm_network::{RetryOpts, redact_url_credentials};
#[cfg(not(target_family = "wasm"))]
use sigstore_sign::{
    SigningContext, SigningServices,
    oidc::IdentityToken,
    rekor::RekorApiVersion,
    trust_root::{SigningConfig, TufConfig},
};

use super::{ProvenanceGenError, SignProvenance, SignedProvenance};
use crate::capabilities::Host;

/// Loaded once per process, so a recursive publish fetches the signing
/// configuration through TUF once rather than once per package.
#[cfg(not(target_family = "wasm"))]
static SIGNING_CONTEXT: tokio::sync::OnceCell<SigningContext> = tokio::sync::OnceCell::const_new();

#[cfg(not(target_family = "wasm"))]
impl SignProvenance for Host {
    async fn sign_statement(
        jwt: &str,
        statement: &[u8],
        timeout: Option<Duration>,
    ) -> Result<SignedProvenance, ProvenanceGenError> {
        let token = IdentityToken::from_jwt(jwt)
            .map_err(|source| ProvenanceGenError::IdentityToken { source: source.to_string() })?;
        let deadline = timeout.unwrap_or(DEFAULT_SIGN_TIMEOUT);
        let context = SIGNING_CONTEXT.get_or_try_init(|| load_signing_context(deadline)).await?;
        sign_with_retry(SIGN_RETRY_OPTS, || {
            with_sign_deadline(deadline, async {
                let bundle = context
                    .signer(token.clone())
                    .sign_raw_statement(statement)
                    .await
                    .map_err(|source| ProvenanceGenError::Sign { source: source.to_string() })?;
                let data = serde_json::to_string(&bundle).expect("serialize sigstore bundle");
                Ok(SignedProvenance { media_type: bundle.media_type.as_str().to_owned(), data })
            })
        })
        .await
    }
}

/// Only the TUF fetch is retried. Selecting the services fails the same way on
/// every attempt.
#[cfg(not(target_family = "wasm"))]
async fn load_signing_context(deadline: Duration) -> Result<SigningContext, ProvenanceGenError> {
    let config = setup_signing_context(deadline, SIGN_RETRY_OPTS, || {
        SigningConfig::from_tuf(TufConfig::production())
    })
    .await?;
    let services = signing_services(&config)
        .map_err(|source| ProvenanceGenError::Sign { source: source.to_string() })?;
    Ok(SigningContext::new(services))
}

/// Selects the services from the TUF signing configuration, with Rekor pinned
/// to API v1. npm's own client writes Rekor v1 entries only, so the pin keeps
/// a Rekor v2 endpoint added to the configuration from changing the bundle the
/// npm registry receives.
#[cfg(not(target_family = "wasm"))]
pub(super) fn signing_services(
    config: &SigningConfig,
) -> Result<SigningServices, sigstore_sign::Error> {
    SigningServices::from_signing_config_with_rekor_version(config, Some(RekorApiVersion::V1))
}

#[cfg(target_family = "wasm")]
impl SignProvenance for Host {
    async fn sign_statement(
        _jwt: &str,
        _statement: &[u8],
        _timeout: Option<Duration>,
    ) -> Result<SignedProvenance, ProvenanceGenError> {
        Err(ProvenanceGenError::UnsupportedRuntime)
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
#[cfg(not(target_family = "wasm"))]
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
#[cfg(not(target_family = "wasm"))]
const DEFAULT_SIGN_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(not(target_family = "wasm"))]
pub(super) async fn with_sign_deadline<Success, Fut>(
    deadline: Duration,
    attempt: Fut,
) -> Result<Success, ProvenanceGenError>
where
    Fut: Future<Output = Result<Success, ProvenanceGenError>>,
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

#[cfg(not(target_family = "wasm"))]
pub(super) async fn setup_signing_context<Context, SetupError, Fut>(
    deadline: Duration,
    retry_opts: RetryOpts,
    mut setup: impl FnMut() -> Fut,
) -> Result<Context, ProvenanceGenError>
where
    SetupError: std::fmt::Display,
    Fut: Future<Output = Result<Context, SetupError>>,
{
    sign_with_retry(retry_opts, || {
        let setup = setup();
        with_sign_deadline(deadline, async move {
            setup.await.map_err(|source| ProvenanceGenError::Sign { source: source.to_string() })
        })
    })
    .await
}

#[cfg(not(target_family = "wasm"))]
pub(super) async fn sign_with_retry<Success, Fut>(
    retry_opts: RetryOpts,
    mut attempt_fn: impl FnMut() -> Fut,
) -> Result<Success, ProvenanceGenError>
where
    Fut: Future<Output = Result<Success, ProvenanceGenError>>,
{
    let mut attempt = 0;
    loop {
        match attempt_fn().await {
            Ok(signed) => return Ok(signed),
            Err(error) if attempt < retry_opts.retries => {
                let delay = retry_opts.delay_for(attempt);
                tracing::warn!(
                    target: "pnpm_publish::provenance",
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
