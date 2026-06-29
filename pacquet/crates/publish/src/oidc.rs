//! Port of `publish/oidc`: OpenID-Connect trusted publishing — fetch a CI
//! id-token, exchange it for a registry auth token, and decide whether to
//! attach provenance.
//!
//! Each step is generic over a single `Sys` type parameter carrying only the
//! capabilities it consumes ([`EnvVar`], [`CiInfo`](crate::CiInfo), [`Clock`],
//! [`OidcFetch`]), so a test drives the external-service happy path with
//! `fn`-bound unit-struct fakes instead of a live registry.

mod auth_token;
mod id_token;
mod provenance;

use pacquet_reporter::Reporter;
use serde_json::Value;
use url::Url;

use crate::{
    capabilities::{Clock, EnvVar, OidcFetch, OidcFetchError, OidcMethod, OidcRequest},
    global_log::global_info,
};

pub use auth_token::{AuthTokenError, fetch_auth_token};
pub use id_token::{GetIdTokenError, IdTokenError, get_id_token};
pub use provenance::{DetermineProvenanceError, ProvenanceError, determine_provenance};

/// Read an environment variable, treating an empty value as unset to mirror
/// JavaScript truthiness (`if (env.X)`).
pub(crate) fn truthy_env<Sys: EnvVar>(name: &str) -> Option<String> {
    Sys::var(name).filter(|value| !value.is_empty())
}

/// A failure of [`github_request_token`]. Each OIDC caller maps these onto its
/// own error type so the public, per-feature error codes stay unchanged.
pub(crate) enum GitHubRequestTokenError {
    /// `ACTIONS_ID_TOKEN_REQUEST_TOKEN` / `..._URL` are not both set.
    IncorrectPermissions,
    InvalidRequestUrl(url::ParseError),
    Fetch(OidcFetchError),
    /// The request-token endpoint returned a non-2xx response.
    NotOk,
    /// The response body was not valid JSON.
    JsonParse(String),
    /// The response JSON carried no string `value`.
    MissingValue,
}

/// Drive GitHub Actions' id-token request endpoint for `audience` and return
/// the raw `value`. Shared by [`get_id_token`] (audience `npm:<host>`) and the
/// sigstore-token fetch in [`provenance_gen`](crate::generate_provenance)
/// (audience `sigstore`); pnpm gets its sigstore token from sigstore-js, so
/// only pacquet has both call sites and this consolidation is pacquet's own.
pub(crate) async fn github_request_token<Sys, Reporter>(
    audience: &str,
    options: &OidcHttpOptions,
) -> Result<String, GitHubRequestTokenError>
where
    Sys: EnvVar + Clock + OidcFetch,
    Reporter: self::Reporter,
{
    let (Some(request_token), Some(request_url)) = (
        truthy_env::<Sys>("ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
        truthy_env::<Sys>("ACTIONS_ID_TOKEN_REQUEST_URL"),
    ) else {
        return Err(GitHubRequestTokenError::IncorrectPermissions);
    };

    let mut url = Url::parse(&request_url).map_err(GitHubRequestTokenError::InvalidRequestUrl)?;
    url.query_pairs_mut().append_pair("audience", audience);
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
    .map_err(GitHubRequestTokenError::Fetch)?;

    let elapsed = Sys::now_ms().saturating_sub(start);
    global_info::<Reporter>(&format!("GET {url} {} {elapsed}ms", response.status));

    if !response.ok {
        return Err(GitHubRequestTokenError::NotOk);
    }

    let json: Value = serde_json::from_str(&response.body)
        .map_err(|source| GitHubRequestTokenError::JsonParse(source.to_string()))?;

    json.get("value")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or(GitHubRequestTokenError::MissingValue)
}

/// npm-package-arg's `escapedName`: percent-encode the scope separator so the
/// package name is a single URL path segment (`@scope/name` → `@scope%2fname`).
pub(crate) fn escaped_package_name(name: &str) -> String {
    name.replace('/', "%2f")
}

/// The fetch-retry / timeout knobs the OIDC requests forward, sourced from the
/// publish options. Ports the `Pick<PublishPackedPkgOptions, 'fetchRetries' |
/// ...>` shared by all three OIDC steps.
#[derive(Debug, Default, Clone)]
pub struct OidcHttpOptions {
    pub fetch_retries: Option<u32>,
    pub fetch_retry_factor: Option<f64>,
    pub fetch_retry_maxtimeout: Option<u64>,
    pub fetch_retry_mintimeout: Option<u64>,
    pub fetch_timeout: Option<u64>,
}
