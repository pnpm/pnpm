use crate::cli_args::registry_client::{
    apply_auth_and_otp, build_registry_client, join_registry_endpoint,
    resolve_registries_with_override,
};
use clap::Parser;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_network::{
    RetryOpts, ThrottledClient, ThrottledClientGuard, encode_uri_component, normalize_registry_url,
    send_with_retry,
};
use reqwest::Response;
use serde_json::Value;

fn parse_stars_response(body: &Value) -> Option<String> {
    if let Some(arr) = body.as_array() {
        let res: Vec<String> = arr
            .iter()
            .filter_map(|val| val.as_str().map(String::from))
            .collect();
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
    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

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
        let registries = resolve_registries_with_override(config, self.registry.as_deref());
        let default_registry =
            registries.get("default").map_or(config.registry.as_str(), String::as_str);
        let auth_header = config.auth_headers.for_url(default_registry);
        let http_client = build_registry_client(config)?;
        let retry_opts = config.retry_opts();

        let (username, is_self) = if let Some(user) = &self.username {
            (user.clone(), false)
        } else {
            let auth = auth_header.as_deref().ok_or(StarsError::Unauthorized)?;
            let whoami = crate::cli_args::whoami::fetch_whoami(
                default_registry,
                &http_client,
                auth,
                retry_opts,
            )
            .await?;
            (whoami, true)
        };

        let request = StarsRequest {
            registry_url: default_registry,
            http_client: &http_client,
            auth_header: auth_header.as_deref(),
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
        let normalized = normalize_registry_url(self.registry_url);
        let star_url = join_registry_endpoint(self.registry_url, "-/user/v1/star")
            .unwrap_or_else(|_| format!("{normalized}-/user/v1/star"));
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
            apply_auth_and_otp(client.get(url), self.auth_header, None)
        })
        .await
        .into_diagnostic()
        .wrap_err(context)
    }

    /// The stars of `username`. Registries that do not serve
    /// `-/user/<name>/stars` serve the same document under `-/util/`.
    async fn user_stars(&self, username: &str) -> miette::Result<Value> {
        let encoded_username = encode_uri_component(username);
        let endpoint = format!("-/user/{encoded_username}/stars");
        let normalized = normalize_registry_url(self.registry_url);
        let stars_url = join_registry_endpoint(self.registry_url, &endpoint)
            .unwrap_or_else(|_| format!("{normalized}{endpoint}"));
        let (client, response) = self.get(&stars_url, "requesting the user stars endpoint").await?;
        if response.status().is_success() {
            let body = response.json().await.into_diagnostic()?;
            drop(client);
            return Ok(body);
        }
        drop(client);

        let util_endpoint = format!("-/util/user/{encoded_username}/stars");
        let util_stars_url = join_registry_endpoint(self.registry_url, &util_endpoint)
            .unwrap_or_else(|_| format!("{normalized}{util_endpoint}"));
        let (client, response) =
            self.get(&util_stars_url, "requesting the alt user stars endpoint").await?;
        if !response.status().is_success() {
            let status = response.status();
            if status == 404 {
                return Err(StarsError::UserNotFound { username: username.to_string() }.into());
            }
            return Err(StarsError::Failed {
                status: status.as_u16(),
                status_text: status
                    .canonical_reason()
                    .unwrap_or_default()
                    .to_string(),
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
