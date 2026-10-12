use crate::cli_args::registry_client::{
    apply_auth_and_otp, build_registry_client, join_registry_endpoint,
};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_network::{RetryOpts, ThrottledClient, normalize_registry_url, send_with_retry};
use serde::Deserialize;

#[cfg(test)]
mod tests;

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

#[derive(Debug, Default, Args)]
pub struct WhoamiArgs {
    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,
}

impl WhoamiArgs {
    /// `pacquet whoami` — return the username the configured registry
    /// associates with the current auth token.
    ///
    /// Resolve the default registry (or `--registry` override), look up its
    /// `Authorization` header, and fail with `ERR_PNPM_WHOAMI_UNAUTHORIZED` when
    /// no credentials are configured — before any request is made.
    pub async fn run(&self, config: &Config) -> miette::Result<String> {
        let registry_url = self.registry.as_deref().unwrap_or(&config.registry);
        let normalized = normalize_registry_url(registry_url);
        let auth_header =
            config.auth_headers.for_url(&normalized).ok_or(WhoamiError::Unauthorized)?;
        let http_client = build_registry_client(config)?;
        let retry_opts = config.retry_opts();
        fetch_whoami(&normalized, &http_client, &auth_header, retry_opts).await
    }
}

/// GET `<registry>-/whoami` with the resolved `Authorization` header and
/// read `username` from the JSON body.
///
/// Errors with `ERR_PNPM_WHOAMI_FAILED` on any non-success status.
/// `registry_url` is the config registry, which always carries a trailing
/// slash, so concatenating `-/whoami` resolves it relative to the registry
/// (preserving any registry path prefix).
pub(crate) async fn fetch_whoami(
    registry_url: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    retry_opts: RetryOpts,
) -> miette::Result<String> {
    let normalized = normalize_registry_url(registry_url);
    let url = join_registry_endpoint(registry_url, "-/whoami")
        .unwrap_or_else(|_| format!("{normalized}-/whoami"));
    // Diagnostic context omits the URL: a registry configured as
    // `https://user:password@host/` carries inline credentials (accepted by
    // `AuthHeaders`), which must not reach stderr / CI logs.
    let (client, response) = send_with_retry(http_client, &url, retry_opts, |client| {
        apply_auth_and_otp(client.get(&url), Some(auth_header), None)
    })
    .await
    .into_diagnostic()
    .wrap_err("requesting the registry whoami endpoint")?;
    if !response.status().is_success() {
        let status = response.status();
        return Err(WhoamiError::Failed {
            status: status.as_u16(),
            status_text: status
                .canonical_reason()
                .unwrap_or_default()
                .to_string(),
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
