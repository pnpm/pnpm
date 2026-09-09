use crate::cli_args::registry_client::build_registry_client;
use clap::Parser;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_network::{
    RetryOpts, ThrottledClient, ThrottledClientGuard, encode_uri_component, send_with_retry,
};
use reqwest::Response;
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

        let request = StarsRequest {
            registry_url: &config.registry,
            http_client: &http_client,
            auth_header: auth_header_val,
            retry_opts,
        };
        fetch_stars(&request, &username, is_self).await
    }
}

/// The registry endpoints that answer "what has this user starred?", and
/// what a request to them needs to carry.
struct StarsRequest<'a> {
    registry_url: &'a str,
    http_client: &'a ThrottledClient,
    auth_header: Option<&'a str>,
    retry_opts: RetryOpts,
}

impl StarsRequest<'_> {
    /// The stars of the authenticated user, from the endpoint that needs no
    /// username. Not every registry serves it, and one that does may answer
    /// with something other than the list, so a `None` here means the
    /// per-user endpoint still has to be asked.
    async fn own_stars(&self) -> miette::Result<Option<Value>> {
        let star_url = format!("{}-/user/v1/star", self.registry_url);
        let (client, response) = self.get(&star_url, "requesting the self stars endpoint").await?;
        if !response.status().is_success() {
            drop(client);
            return Ok(None);
        }
        let body: Value = response.json().await.into_diagnostic()?;
        drop(client);
        Ok((body.is_array() || body.is_object()).then_some(body))
    }

    async fn get(
        &self,
        url: &str,
        context: &'static str,
    ) -> miette::Result<(ThrottledClientGuard<'_>, Response)> {
        send_with_retry(self.http_client, url, self.retry_opts, |client| {
            let mut req = client.get(url);
            if let Some(auth) = self.auth_header {
                req = req.header("authorization", auth);
            }
            req
        })
        .await
        .into_diagnostic()
        .wrap_err(context)
    }

    /// The stars of `username`. Registries that do not serve
    /// `-/user/<name>/stars` serve the same document under `-/util/`.
    async fn user_stars(&self, username: &str) -> miette::Result<Value> {
        let encoded_username = encode_uri_component(username);
        let stars_url = format!("{}-/user/{encoded_username}/stars", self.registry_url);
        let (client, response) = self.get(&stars_url, "requesting the user stars endpoint").await?;
        if response.status().is_success() {
            let body = response.json().await.into_diagnostic()?;
            drop(client);
            return Ok(body);
        }
        drop(client);

        let util_stars_url = format!("{}-/util/user/{encoded_username}/stars", self.registry_url);
        let (client, response) =
            self.get(&util_stars_url, "requesting the alt user stars endpoint").await?;
        if !response.status().is_success() {
            let status = response.status();
            if status == 404 {
                return Err(StarsError::UserNotFound { username: username.to_string() }.into());
            }
            return Err(StarsError::Failed {
                status: status.as_u16(),
                status_text: status.canonical_reason().unwrap_or_default().to_string(),
            }
            .into());
        }
        let body = response.json().await.into_diagnostic()?;
        drop(client);
        Ok(body)
    }
}

async fn fetch_stars(
    request: &StarsRequest<'_>,
    username: &str,
    is_self: bool,
) -> miette::Result<Option<String>> {
    if is_self && let Some(body) = request.own_stars().await? {
        return Ok(parse_stars_response(&body));
    }
    let body = request.user_stars(username).await?;
    Ok(parse_stars_response(&body))
}
