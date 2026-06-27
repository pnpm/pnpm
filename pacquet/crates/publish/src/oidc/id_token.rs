//! Port of [`oidc/idToken.ts`](https://github.com/pnpm/pnpm/blob/54c5c0e028/pnpm11/releasing/commands/src/publish/oidc/idToken.ts): retrieve an OIDC id-token from the CI
//! environment.

use pacquet_diagnostics::miette::{self, Diagnostic};
use pacquet_reporter::Reporter;
use serde_json::Value;
use url::Url;

use crate::{
    capabilities::{CiInfo, Clock, EnvVar, OidcFetch, OidcFetchError, OidcMethod, OidcRequest},
    global_log::global_info,
    oidc::OidcHttpOptions,
};

#[cfg(test)]
mod tests;

/// Retrieve an `idToken` from the CI environment, or `None` when OIDC is not
/// applicable (outside CI, or a CI without a forwarded token). Ports TS
/// `getIdToken`.
///
/// `NPM_ID_TOKEN` is honored first as the CI-agnostic injection point; failing
/// that, only GitHub Actions' request-token endpoint can be driven directly.
pub async fn get_id_token<Sys, Reporter>(
    registry: &str,
    options: &OidcHttpOptions,
) -> Result<Option<String>, GetIdTokenError>
where
    Sys: EnvVar + CiInfo + Clock + OidcFetch,
    Reporter: self::Reporter,
{
    if let Some(token) = truthy_env::<Sys>("NPM_ID_TOKEN") {
        return Ok(Some(token));
    }

    if !Sys::github_actions() {
        return Ok(None);
    }

    let (Some(request_token), Some(request_url)) = (
        truthy_env::<Sys>("ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
        truthy_env::<Sys>("ACTIONS_ID_TOKEN_REQUEST_URL"),
    ) else {
        return Err(IdTokenError::GitHubWorkflowIncorrectPermissions.into());
    };

    let parsed_registry = Url::parse(registry).map_err(GetIdTokenError::InvalidRegistry)?;
    let audience = format!("npm:{}", parsed_registry.host_str().unwrap_or_default());
    let mut url = Url::parse(&request_url).map_err(GetIdTokenError::InvalidRequestUrl)?;
    url.query_pairs_mut().append_pair("audience", &audience);
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
    .map_err(GetIdTokenError::Fetch)?;

    let elapsed = Sys::now_ms().saturating_sub(start);
    global_info::<Reporter>(&format!("GET {url} {} {elapsed}ms", response.status));

    if !response.ok {
        return Err(IdTokenError::GitHubInvalidResponse.into());
    }

    let json: Value = serde_json::from_str(&response.body)
        .map_err(|source| IdTokenError::GitHubJsonInterrupted { source: source.to_string() })?;

    match json.get("value").and_then(Value::as_str) {
        Some(value) => Ok(Some(value.to_owned())),
        None => Err(IdTokenError::GitHubJsonInvalidValue.into()),
    }
}

/// Read an environment variable, treating an empty value as unset to mirror
/// JavaScript truthiness (`if (env.X)`).
fn truthy_env<Sys: EnvVar>(name: &str) -> Option<String> {
    Sys::var(name).filter(|value| !value.is_empty())
}

/// A skippable id-token error: surfaced as a warning by the publish flow,
/// which then falls back to static credentials. Ports the `IdTokenError`
/// hierarchy.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum IdTokenError {
    #[display("Incorrect permissions for idToken within GitHub Workflows")]
    #[diagnostic(code(ERR_PNPM_ID_TOKEN_GITHUB_WORKFLOW_INCORRECT_PERMISSIONS))]
    GitHubWorkflowIncorrectPermissions,

    #[display("Failed to fetch idToken from GitHub: received an invalid response")]
    #[diagnostic(code(ERR_PNPM_ID_TOKEN_GITHUB_INVALID_RESPONSE))]
    GitHubInvalidResponse,

    #[display("Fetching of idToken JSON interrupted: {source}")]
    #[diagnostic(code(ERR_PNPM_ID_TOKEN_GITHUB_JSON_INTERRUPTED_ERROR))]
    GitHubJsonInterrupted {
        #[error(not(source))]
        source: String,
    },

    #[display("Failed to fetch idToken from GitHub: missing or invalid value")]
    #[diagnostic(code(ERR_PNPM_ID_TOKEN_GITHUB_JSON_INVALID_VALUE))]
    GitHubJsonInvalidValue,
}

/// The error surface of [`get_id_token`]. Only the [`IdToken`](Self::IdToken)
/// arm is skippable; the rest are hard transport/parse failures that the TS
/// code lets propagate past the `error instanceof IdTokenError` guard.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum GetIdTokenError {
    #[display("{_0}")]
    #[diagnostic(transparent)]
    IdToken(IdTokenError),

    #[display("{_0}")]
    Fetch(OidcFetchError),

    #[display("invalid registry URL: {_0}")]
    InvalidRegistry(url::ParseError),

    #[display("invalid id-token request URL: {_0}")]
    InvalidRequestUrl(url::ParseError),
}

impl From<IdTokenError> for GetIdTokenError {
    fn from(error: IdTokenError) -> Self {
        GetIdTokenError::IdToken(error)
    }
}
