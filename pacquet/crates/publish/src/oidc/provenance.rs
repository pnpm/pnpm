//! Port of [`oidc/provenance.ts`](https://github.com/pnpm/pnpm/blob/54c5c0e028/pnpm11/releasing/commands/src/publish/oidc/provenance.ts): decide whether to attach provenance based on
//! the CI context and the package's registry visibility.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pacquet_diagnostics::miette::{self, Diagnostic};
use pipe_trait::Pipe;
use serde_json::Value;
use url::Url;

use crate::{
    capabilities::{CiInfo, EnvVar, OidcFetch, OidcFetchError, OidcMethod, OidcRequest},
    oidc::{OidcHttpOptions, escaped_package_name},
};

#[cfg(test)]
mod tests;

/// Determine the `provenance` flag for a package from the CI context and the
/// package's visibility. Returns `Some(true)` when provenance should be
/// attached, `None` when it should not. Ports TS `determineProvenance`.
///
/// A [`ProvenanceError`] is skippable (the publish proceeds without
/// provenance); a malformed id-token payload or a failed visibility request is
/// a hard error, matching the TS code that lets those propagate.
pub async fn determine_provenance<Sys>(
    auth_token: &str,
    id_token: &str,
    package_name: &str,
    registry: &str,
    options: &OidcHttpOptions,
) -> Result<Option<bool>, DetermineProvenanceError>
where
    Sys: CiInfo + EnvVar + OidcFetch,
{
    let mut parts = id_token.split('.');
    let (Some(header_b64), Some(payload_b64)) = (parts.next(), parts.next()) else {
        return Err(ProvenanceError::MalformedIdToken.into());
    };
    if header_b64.is_empty() || payload_b64.is_empty() {
        return Err(ProvenanceError::MalformedIdToken.into());
    }

    let payload = decode_jwt_payload(payload_b64)?;
    let repository_visibility = payload.get("repository_visibility").and_then(Value::as_str);
    let project_visibility = payload.get("project_visibility").and_then(Value::as_str);

    let github_public = Sys::github_actions() && repository_visibility == Some("public");
    let gitlab_public = Sys::gitlab()
        && project_visibility == Some("public")
        && Sys::var("SIGSTORE_ID_TOKEN").is_some_and(|token| !token.is_empty());
    if !github_public && !gitlab_public {
        return Err(ProvenanceError::InsufficientInformation.into());
    }

    let path = format!("/-/package/{}/visibility", escaped_package_name(package_name));
    let visibility_url = Url::parse(registry)
        .and_then(|base| base.join(&path))
        .map_err(DetermineProvenanceError::InvalidUrl)?
        .to_string();

    let authorization = format!("Bearer {auth_token}");
    let response = Sys::fetch(OidcRequest {
        method: OidcMethod::Get,
        url: &visibility_url,
        authorization: &authorization,
        timeout_ms: options.fetch_timeout,
    })
    .await
    .map_err(DetermineProvenanceError::Fetch)?;

    if !response.ok {
        return Err(ProvenanceError::failed_to_fetch_visibility(
            &response.body,
            response.status,
            package_name,
            registry,
        )
        .into());
    }

    let public = response
        .body
        .pipe_as_ref(serde_json::from_str::<Value>)
        .ok()
        .and_then(|json| json.get("public").and_then(Value::as_bool))
        .unwrap_or(false);
    Ok(public.then_some(true))
}

/// Decode the base64url JWT payload into JSON. A decode or parse failure is a
/// hard error, matching the TS `JSON.parse(...)` that runs unguarded.
fn decode_jwt_payload(payload_b64: &str) -> Result<Value, DetermineProvenanceError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(payload_b64.trim_end_matches('='))
        .map_err(|error| DetermineProvenanceError::PayloadParse(error.to_string()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| DetermineProvenanceError::PayloadParse(error.to_string()))
}

/// A skippable provenance error: the publish proceeds without provenance.
/// Ports the `ProvenanceError` hierarchy.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum ProvenanceError {
    #[display("The received idToken is not a valid JWT")]
    #[diagnostic(code(ERR_PNPM_PROVENANCE_MALFORMED_ID_TOKEN))]
    MalformedIdToken,

    #[display("The environment does not provide enough information to determine visibility")]
    #[diagnostic(code(ERR_PNPM_PROVENANCE_INSUFFICIENT_INFORMATION))]
    InsufficientInformation,

    #[display(
        "Failed to fetch visibility for package {package_name} from registry {registry} due to {message} (status code {status})"
    )]
    #[diagnostic(code(ERR_PNPM_PROVENANCE_FAILED_TO_FETCH_VISIBILITY))]
    FailedToFetchVisibility { message: String, status: u16, package_name: String, registry: String },
}

impl ProvenanceError {
    /// Build a [`FailedToFetchVisibility`](Self::FailedToFetchVisibility) from
    /// the rejected response body, mirroring the TS `code`/`message` precedence.
    fn failed_to_fetch_visibility(
        body: &str,
        status: u16,
        package_name: &str,
        registry: &str,
    ) -> Self {
        let parsed = serde_json::from_str::<Value>(body).ok();
        let code = parsed.as_ref().and_then(|json| json.get("code")?.as_str().map(str::to_owned));
        let detail =
            parsed.as_ref().and_then(|json| json.get("message")?.as_str().map(str::to_owned));
        let message = match (code, detail) {
            (Some(code), Some(detail)) => format!("{code}: {detail}"),
            (Some(code), None) => code,
            (None, Some(detail)) => detail,
            (None, None) => "an unknown error".to_owned(),
        };
        ProvenanceError::FailedToFetchVisibility {
            message,
            status,
            package_name: package_name.to_owned(),
            registry: registry.to_owned(),
        }
    }
}

/// The error surface of [`determine_provenance`]. Only the
/// [`Provenance`](Self::Provenance) arm is skippable.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum DetermineProvenanceError {
    #[display("{_0}")]
    #[diagnostic(transparent)]
    Provenance(ProvenanceError),

    #[display("failed to parse the idToken payload: {_0}")]
    PayloadParse(#[error(not(source))] String),

    #[display("invalid visibility URL: {_0}")]
    InvalidUrl(url::ParseError),

    #[display("{_0}")]
    Fetch(OidcFetchError),
}

impl From<ProvenanceError> for DetermineProvenanceError {
    fn from(error: ProvenanceError) -> Self {
        DetermineProvenanceError::Provenance(error)
    }
}
