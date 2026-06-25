//! `pacquet ping` — test connectivity to the configured registry.
//!
//! Ports pnpm's
//! [`ping` command](https://github.com/pnpm/pnpm/blob/fc2f33912e/pnpm11/registry-access/commands/src/ping.ts).

use crate::cli_args::registry_client::build_registry_client;
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_network::{RetryOpts, ThrottledClient, redact_url_credentials, send_with_retry};
use serde_json::Value;
use std::time::Instant;

/// Errors from `pacquet ping`.
///
/// Both a transport failure and a non-success status map to the single
/// `PING_ERROR` code pnpm raises in `ping.ts`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PingError {
    #[display("Failed to reach registry: {message}")]
    #[diagnostic(code(ERR_PNPM_PING_ERROR))]
    Unreachable { message: String },
}

#[derive(Debug, Args)]
pub struct PingArgs {
    /// Test a specific registry URL.
    #[clap(long)]
    pub registry: Option<String>,
}

impl PingArgs {
    /// Ports `ping.ts`'s `handler`: resolve the registry (the `--registry`
    /// override or the configured default), GET `<registry>-/ping?write=true`
    /// with any resolved auth header, and render pnpm's `PING`/`PONG` report.
    pub async fn run(&self, config: &Config) -> miette::Result<String> {
        let registry_url = self.registry.as_deref().unwrap_or(&config.registry);
        // Add a trailing slash before joining so a registry with a path
        // prefix keeps it, matching `new URL('./-/ping', normalizedRegistryUrl)`.
        let normalized_registry_url = if registry_url.ends_with('/') {
            registry_url.to_owned()
        } else {
            format!("{registry_url}/")
        };
        let ping_url = format!("{normalized_registry_url}-/ping?write=true");
        let auth_header = config.auth_headers.for_url(&normalized_registry_url);
        let http_client = build_registry_client(config)?;

        // `ping` issues a single attempt with no retries, matching pnpm's
        // `retry: { retries: 0 }`.
        let (time, body) = fetch_ping(
            &ping_url,
            &http_client,
            auth_header.as_deref(),
            RetryOpts { retries: 0, ..RetryOpts::default() },
        )
        .await?;

        let mut report = format!("PING {registry_url}\nPONG {time}ms");
        if let Some(details) = format_details(&body) {
            report.push_str("\nPONG ");
            report.push_str(&details);
        }
        Ok(report)
    }
}

/// GET `ping_url` with the optional `Authorization` header, timing the
/// round trip (request plus body read) the way pnpm measures `Date.now()`.
///
/// Errors with `ERR_PNPM_PING_ERROR` on any transport failure or
/// non-success status. The transport error message is credential-redacted
/// before it reaches stderr / CI logs: a registry configured as
/// `https://user:password@host/` carries inline credentials that the
/// underlying error may echo back.
async fn fetch_ping(
    ping_url: &str,
    http_client: &ThrottledClient,
    auth_header: Option<&str>,
    retry_opts: RetryOpts,
) -> miette::Result<(u128, String)> {
    let start = Instant::now();
    let (client, response) = send_with_retry(http_client, ping_url, retry_opts, |client| {
        let request = client.get(ping_url);
        match auth_header {
            Some(value) => request.header("authorization", value),
            None => request,
        }
    })
    .await
    .map_err(|error| PingError::Unreachable {
        message: redact_url_credentials(&error.to_string()),
    })?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(PingError::Unreachable {
            message: format!(
                "{} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or_default(),
            )
            .trim_end()
            .to_owned(),
        }
        .into());
    }

    let body =
        response.text().await.into_diagnostic().wrap_err("reading the registry ping response")?;
    let time = start.elapsed().as_millis();
    drop(client);
    Ok((time, body))
}

/// Pretty-print the ping response body when it is a non-empty JSON object
/// or array, mirroring `ping.ts`'s `JSON.stringify(parsed, null, 2)` branch
/// (which fires for any `typeof === 'object'` value with at least one entry).
/// Returns `None` for an empty body, non-JSON, or an empty/primitive value.
fn format_details(body: &str) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    let value = serde_json::from_str::<Value>(body).ok()?;
    let non_empty = match &value {
        Value::Object(map) => !map.is_empty(),
        Value::Array(items) => !items.is_empty(),
        _ => false,
    };
    if non_empty { serde_json::to_string_pretty(&value).ok() } else { None }
}
