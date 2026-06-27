//! Port of `oidc/authToken.ts`: exchange a CI id-token for a registry auth
//! token via the npm OIDC token-exchange endpoint.

use pacquet_diagnostics::miette::{self, Diagnostic};
use serde_json::Value;
use url::Url;

use crate::{
    capabilities::{OidcFetch, OidcMethod, OidcRequest},
    oidc::{OidcHttpOptions, escaped_package_name},
};

#[cfg(test)]
mod tests;

/// Exchange `id_token` for an `authToken` scoped to `package_name` on
/// `registry`. Ports TS `fetchAuthToken`. Every failure is an
/// [`AuthTokenError`], which the publish flow turns into a warning and skips.
pub async fn fetch_auth_token<Sys: OidcFetch>(
    id_token: &str,
    package_name: &str,
    registry: &str,
    options: &OidcHttpOptions,
) -> Result<String, AuthTokenError> {
    let path =
        format!("/-/npm/v1/oidc/token/exchange/package/{}", escaped_package_name(package_name));
    let url = Url::parse(registry)
        .and_then(|base| base.join(&path))
        .map_err(|error| AuthTokenError::Fetch {
            error_source: error.to_string(),
            package_name: package_name.to_owned(),
            registry: registry.to_owned(),
        })?
        .to_string();

    let authorization = format!("Bearer {id_token}");
    let response = Sys::fetch(OidcRequest {
        method: OidcMethod::Post,
        url: &url,
        authorization: &authorization,
        timeout_ms: options.fetch_timeout,
    })
    .await
    .map_err(|error| AuthTokenError::Fetch {
        error_source: error.reason,
        package_name: package_name.to_owned(),
        registry: registry.to_owned(),
    })?;

    if !response.ok {
        let message = serde_json::from_str::<Value>(&response.body)
            .ok()
            .and_then(|json| json.get("body")?.get("message")?.as_str().map(str::to_owned))
            .unwrap_or_else(|| "Unknown error".to_owned());
        return Err(AuthTokenError::Exchange { message, http_status: response.status });
    }

    let json: Value = serde_json::from_str(&response.body)
        .map_err(|source| AuthTokenError::JsonInterrupted { source: source.to_string() })?;

    match json.get("token").and_then(Value::as_str) {
        Some(token) => Ok(token.to_owned()),
        None => Err(AuthTokenError::MalformedJson {
            package_name: package_name.to_owned(),
            registry: registry.to_owned(),
        }),
    }
}

/// A skippable auth-token error: surfaced as a warning by the publish flow.
/// Ports the `AuthTokenError` hierarchy.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum AuthTokenError {
    #[display(
        "Failed to fetch authToken for package {package_name} from registry {registry}: {error_source}"
    )]
    #[diagnostic(code(ERR_PNPM_AUTH_TOKEN_FETCH))]
    Fetch { error_source: String, package_name: String, registry: String },

    #[display(
        "Failed token exchange request with body message: {message} (status code {http_status})"
    )]
    #[diagnostic(code(ERR_PNPM_AUTH_TOKEN_EXCHANGE))]
    Exchange { message: String, http_status: u16 },

    #[display("Fetching of authToken JSON interrupted: {source}")]
    #[diagnostic(code(ERR_PNPM_AUTH_TOKEN_JSON_INTERRUPTED))]
    JsonInterrupted {
        #[error(not(source))]
        source: String,
    },

    #[display(
        "Failed to fetch authToken for package {package_name} from registry {registry} due to malformed JSON response"
    )]
    #[diagnostic(code(ERR_PNPM_AUTH_TOKEN_MALFORMED_JSON))]
    MalformedJson { package_name: String, registry: String },
}
