use crate::cli_args::registry_client::build_registry_client;
use clap::Parser;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_network::{RetryOpts, ThrottledClient, send_with_retry};
use serde_json::json;
use std::time::Duration;

#[derive(Debug, Parser)]
pub struct StarArgs {
    pub package_name: String,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum StarError {
    #[display("You must be logged in to star packages")]
    #[diagnostic(code(ERR_PNPM_STAR_UNAUTHORIZED))]
    Unauthorized,

    #[display("Failed to star package: {status} {status_text}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    Failed { status: u16, status_text: String },
}

impl StarArgs {
    pub async fn run(&self, config: &Config) -> miette::Result<()> {
        let auth_header =
            config.auth_headers.for_url(&config.registry).ok_or(StarError::Unauthorized)?;
        let http_client = build_registry_client(config)?;
        let retry_opts = RetryOpts {
            retries: config.fetch_retries,
            factor: config.fetch_retry_factor,
            min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
            max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
        };
        fetch_star(
            &config.registry,
            &http_client,
            &auth_header,
            retry_opts,
            &self.package_name,
            true,
        )
        .await
    }
}

pub(crate) async fn fetch_star(
    registry_url: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    retry_opts: RetryOpts,
    package_name: &str,
    is_star: bool,
) -> miette::Result<()> {
    let method = if is_star { reqwest::Method::PUT } else { reqwest::Method::DELETE };
    let star_url = format!("{registry_url}-/user/v1/star");
    let body = json!({ "name": package_name, "package": package_name }).to_string();

    let (client, response) = send_with_retry(http_client, &star_url, retry_opts, |client| {
        client
            .request(method.clone(), &star_url)
            .header("authorization", auth_header)
            .header("content-type", "application/json")
            .body(body.clone())
    })
    .await
    .into_diagnostic()
    .wrap_err("requesting the registry star endpoint")?;

    if !response.status().is_success() {
        drop(client);
        let escaped_name = package_name
            .chars()
            .map(|ch| match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' | '@' => ch.to_string(),
                _ => format!("%{:02X}", ch as u8),
            })
            .collect::<String>();
        let alt_star_url = format!("{registry_url}-/user/package/{escaped_name}/star");

        let (client2, response2) =
            send_with_retry(http_client, &alt_star_url, retry_opts, |client| {
                client
                    .request(method.clone(), &alt_star_url)
                    .header("authorization", auth_header)
                    .header("content-type", "application/json")
            })
            .await
            .into_diagnostic()
            .wrap_err("requesting the alt registry star endpoint")?;

        if !response2.status().is_success() {
            let status = response2.status();
            // Not doing the legacy packument fallback for now unless tests need it
            return Err(StarError::Failed {
                status: status.as_u16(),
                status_text: status.canonical_reason().unwrap_or_default().to_string(),
            }
            .into());
        }
        drop(client2);
        return Ok(());
    }

    drop(client);
    Ok(())
}
