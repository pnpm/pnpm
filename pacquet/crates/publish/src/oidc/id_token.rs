//! Port of [`oidc/idToken.ts`](https://github.com/pnpm/pnpm/blob/54c5c0e028/pnpm11/releasing/commands/src/publish/oidc/idToken.ts): retrieve an OIDC id-token from the CI
//! environment.

use pacquet_diagnostics::miette::{self, Diagnostic};
use pacquet_reporter::Reporter;
use url::Url;

use crate::{
    capabilities::{CiInfo, Clock, EnvVar, OidcFetch, OidcFetchError},
    oidc::{GitHubRequestTokenError, OidcHttpOptions, github_request_token, truthy_env},
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

    let parsed_registry = Url::parse(registry).map_err(GetIdTokenError::InvalidRegistry)?;
    let audience = format!("npm:{}", parsed_registry.host_str().unwrap_or_default());

    github_request_token::<Sys, Reporter>(&audience, options).await.map(Some).map_err(Into::into)
}

/// A skippable id-token error: surfaced as a warning by the publish flow,
/// which then falls back to static credentials. Ports the
/// [`IdTokenError`][ts-IdTokenError] hierarchy.
///
/// [ts-IdTokenError]: https://github.com/pnpm/pnpm/blob/54c5c0e028/pnpm11/releasing/commands/src/publish/oidc/idToken.ts#L143
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

impl From<GitHubRequestTokenError> for GetIdTokenError {
    fn from(error: GitHubRequestTokenError) -> Self {
        match error {
            GitHubRequestTokenError::IncorrectPermissions => {
                IdTokenError::GitHubWorkflowIncorrectPermissions.into()
            }
            GitHubRequestTokenError::InvalidRequestUrl(error) => {
                GetIdTokenError::InvalidRequestUrl(error)
            }
            GitHubRequestTokenError::Fetch(error) => GetIdTokenError::Fetch(error),
            GitHubRequestTokenError::NotOk => IdTokenError::GitHubInvalidResponse.into(),
            GitHubRequestTokenError::JsonParse(source) => {
                IdTokenError::GitHubJsonInterrupted { source }.into()
            }
            GitHubRequestTokenError::MissingValue => IdTokenError::GitHubJsonInvalidValue.into(),
        }
    }
}
