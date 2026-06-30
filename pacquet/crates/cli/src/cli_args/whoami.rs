use crate::cli_args::registry_client::build_registry_client;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_network::{RetryOpts, ThrottledClient, send_with_retry};
use serde::Deserialize;
use std::time::Duration;

/// Errors from `pacquet whoami`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum WhoamiError {
    #[display("You must be logged in to use whoami")]
    #[diagnostic(code(ERR_PNPM_WHOAMI_UNAUTHORIZED))]
    Unauthorized,

    #[display("Failed to find the current user: {status} {status_text}")]
    #[diagnostic(code(ERR_PNPM_WHOAMI_FAILED))]
    Failed { status: u16, status_text: String },
}

/// The `GET /-/whoami` response body. The registry returns other fields
/// too; only `username` is read.
#[derive(Debug, Deserialize)]
struct WhoamiResponse {
    username: String,
}

/// `pacquet whoami` — return the username the configured registry
/// associates with the current auth token.
///
/// Resolve the default registry, look up its `Authorization` header, and
/// fail with `ERR_PNPM_WHOAMI_UNAUTHORIZED` when no credentials are
/// configured — before any request is made.
pub async fn whoami(config: &Config) -> miette::Result<String> {
    let auth_header =
        config.auth_headers.for_url(&config.registry).ok_or(WhoamiError::Unauthorized)?;
    let http_client = build_registry_client(config)?;
    let retry_opts = RetryOpts {
        retries: config.fetch_retries,
        factor: config.fetch_retry_factor,
        min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
        max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
    };
    fetch_whoami(&config.registry, &http_client, &auth_header, retry_opts).await
}

/// GET `<registry>-/whoami` with the resolved `Authorization` header and
/// read `username` from the JSON body.
///
/// Errors with `ERR_PNPM_WHOAMI_FAILED` on any non-success status.
/// `registry_url` is the config registry, which always carries a trailing
/// slash, so concatenating `-/whoami` resolves it relative to the registry
/// (preserving any registry path prefix).
async fn fetch_whoami(
    registry_url: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    retry_opts: RetryOpts,
) -> miette::Result<String> {
    let url = format!("{registry_url}-/whoami");
    // Diagnostic context omits the URL: a registry configured as
    // `https://user:password@host/` carries inline credentials (accepted by
    // `AuthHeaders`), which must not reach stderr / CI logs.
    let (client, response) = send_with_retry(http_client, &url, retry_opts, |client| {
        client.get(&url).header("authorization", auth_header)
    })
    .await
    .into_diagnostic()
    .wrap_err("requesting the registry whoami endpoint")?;
    if !response.status().is_success() {
        let status = response.status();
        return Err(WhoamiError::Failed {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or_default().to_string(),
        }
        .into());
    }
    let body = response
        .json::<WhoamiResponse>()
        .await
        .into_diagnostic()
        .wrap_err("parsing the whoami response")?;
    drop(client);
    Ok(body.username)
}
