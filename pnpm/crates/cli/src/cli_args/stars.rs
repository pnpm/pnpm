use crate::cli_args::registry_client::build_registry_client;
use clap::Parser;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_network::{RetryOpts, ThrottledClient, encode_uri_component, send_with_retry};
use serde_json::Value;
use std::time::Duration;

fn parse_stars_response(body: &Value) -> Option<String> {
    if let Some(arr) = body.as_array() {
        let res: Vec<String> =
            arr.iter().filter_map(|val| val.as_str().map(String::from)).collect();
        Some(res.join("\n"))
    } else if let Some(obj) = body.as_object() {
        let res: Vec<String> = obj.keys().cloned().collect();
        Some(res.join("\n"))
    } else {
        Some(String::new())
    }
}

#[derive(Debug, Parser)]
pub struct StarsArgs {
    pub username: Option<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum StarsError {
    #[display("You must be logged in to list your starred packages")]
    #[diagnostic(code(ERR_PNPM_STARS_UNAUTHORIZED))]
    Unauthorized,

    #[display("Failed to fetch stars: {status} {status_text}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    Failed { status: u16, status_text: String },

    #[display("User \"{username}\" not found")]
    #[diagnostic(code(ERR_PNPM_USER_NOT_FOUND))]
    UserNotFound { username: String },
}

impl StarsArgs {
    pub async fn run(&self, config: &Config) -> miette::Result<Option<String>> {
        let auth_header =
            config.auth_headers.for_url(&config.registry).ok_or(StarsError::Unauthorized);
        let http_client = build_registry_client(config)?;
        let retry_opts = RetryOpts {
            retries: config.fetch_retries,
            factor: config.fetch_retry_factor,
            min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
            max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
        };

        let mut user = self.username.clone();
        if user.is_none() {
            if auth_header.is_err() {
                return Err(StarsError::Unauthorized.into());
            }
            user = Some(crate::cli_args::whoami::whoami(config).await?);
        }

        let is_self = self.username.is_none();
        let username = user.unwrap();
        let auth_header_str = auth_header.unwrap_or_default();
        let auth_header_val =
            if auth_header_str.is_empty() { None } else { Some(auth_header_str.as_str()) };

        fetch_stars(&config.registry, &http_client, auth_header_val, retry_opts, &username, is_self)
            .await
    }
}

async fn fetch_stars(
    registry_url: &str,
    http_client: &ThrottledClient,
    auth_header: Option<&str>,
    retry_opts: RetryOpts,
    username: &str,
    is_self: bool,
) -> miette::Result<Option<String>> {
    if is_self {
        let star_url = format!("{registry_url}-/user/v1/star");
        let (client, response) = send_with_retry(http_client, &star_url, retry_opts, |client| {
            let mut req = client.get(&star_url);
            if let Some(auth) = auth_header {
                req = req.header("authorization", auth);
            }
            req
        })
        .await
        .into_diagnostic()
        .wrap_err("requesting the self stars endpoint")?;

        if response.status().is_success() {
            let body: Value = response.json().await.into_diagnostic()?;
            drop(client);
            return Ok(parse_stars_response(&body));
        }
        drop(client);
    }

    let encoded_username = encode_uri_component(username);

    let stars_url = format!("{registry_url}-/user/{encoded_username}/stars");

    let (client, response) = send_with_retry(http_client, &stars_url, retry_opts, |client| {
        let mut req = client.get(&stars_url);
        if let Some(auth) = auth_header {
            req = req.header("authorization", auth);
        }
        req
    })
    .await
    .into_diagnostic()
    .wrap_err("requesting the user stars endpoint")?;

    if !response.status().is_success() {
        drop(client);
        let util_stars_url = format!("{registry_url}-/util/user/{encoded_username}/stars");
        let (client2, response2) =
            send_with_retry(http_client, &util_stars_url, retry_opts, |client| {
                let mut req = client.get(&util_stars_url);
                if let Some(auth) = auth_header {
                    req = req.header("authorization", auth);
                }
                req
            })
            .await
            .into_diagnostic()
            .wrap_err("requesting the alt user stars endpoint")?;

        if !response2.status().is_success() {
            let status = response2.status();
            if status == 404 {
                return Err(StarsError::UserNotFound { username: username.to_string() }.into());
            }
            return Err(StarsError::Failed {
                status: status.as_u16(),
                status_text: status.canonical_reason().unwrap_or_default().to_string(),
            }
            .into());
        }

        let body: Value = response2.json().await.into_diagnostic()?;
        drop(client2);
        return Ok(parse_stars_response(&body));
    }

    let body: Value = response.json().await.into_diagnostic()?;
    drop(client);
    Ok(parse_stars_response(&body))
}
