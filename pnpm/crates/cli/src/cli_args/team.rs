use super::sanitize;
use clap::Args;
use derive_more::{Display, Error};
use futures_util::StreamExt as _;
use miette::{Diagnostic, IntoDiagnostic, WrapErr};
use pacquet_config::Config;
use pacquet_network::{
    NetworkSettings, RedirectGuard, RetryOpts, ThrottledClient, encode_uri_component,
    redact_url_credentials, send_with_retry,
};
use pacquet_resolving_npm_resolver::pick_registry_for_package;
use reqwest::Response;
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc, time::Duration};

const TEAM_BODY_LIMIT: usize = 1024 * 1024;
const TEAM_ERROR_BODY_LIMIT: usize = 64 * 1024;

#[derive(Debug, Args)]
pub struct TeamArgs {
    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    /// One-time password for registries that require two-factor authentication.
    #[clap(long)]
    pub otp: Option<String>,

    /// Output parseable results (tab-separated).
    #[clap(long)]
    pub parseable: bool,

    /// Output results as JSON.
    #[clap(long)]
    pub json: bool,

    /// Subcommand and arguments.
    pub params: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum TeamError {
    #[display(
        "Subcommand is required (create, destroy, add, rm, ls). Use `pnpm team ls <scope>` to list teams."
    )]
    #[diagnostic(code(ERR_PNPM_TEAM_SUBCOMMAND_REQUIRED))]
    SubcommandRequired,

    #[display(
        r#"Team spec must start with @scope, got "{spec}". Use @scope or @scope:team format."#
    )]
    #[diagnostic(code(ERR_PNPM_TEAM_INVALID_SCOPE))]
    InvalidScope {
        #[error(not(source))]
        spec: String,
    },

    #[display("Team scope is required (e.g., pnpm team create @org:newteam)")]
    #[diagnostic(code(ERR_PNPM_TEAM_CREATE_SCOPE_REQUIRED))]
    CreateScopeRequired,

    #[display("Team name is required (e.g., pnpm team create @org:newteam)")]
    #[diagnostic(code(ERR_PNPM_TEAM_CREATE_NAME_REQUIRED))]
    CreateNameRequired,

    #[display("Team scope is required (e.g., pnpm team destroy @org:newteam)")]
    #[diagnostic(code(ERR_PNPM_TEAM_DESTROY_SCOPE_REQUIRED))]
    DestroyScopeRequired,

    #[display("Team name is required (e.g., pnpm team destroy @org:newteam)")]
    #[diagnostic(code(ERR_PNPM_TEAM_DESTROY_NAME_REQUIRED))]
    DestroyNameRequired,

    #[display("Team scope and user are required (e.g., pnpm team add @org:team username)")]
    #[diagnostic(code(ERR_PNPM_TEAM_ADD_ARGS_REQUIRED))]
    AddArgsRequired,

    #[display("Team name is required (e.g., pnpm team add @org:team username)")]
    #[diagnostic(code(ERR_PNPM_TEAM_ADD_NAME_REQUIRED))]
    AddNameRequired,

    #[display("Team scope and user are required (e.g., pnpm team rm @org:team username)")]
    #[diagnostic(code(ERR_PNPM_TEAM_RM_ARGS_REQUIRED))]
    RmArgsRequired,

    #[display("Team name is required (e.g., pnpm team rm @org:team username)")]
    #[diagnostic(code(ERR_PNPM_TEAM_RM_NAME_REQUIRED))]
    RmNameRequired,

    #[display("Organization scope is required (e.g., pnpm team ls @org or pnpm team ls @org:team)")]
    #[diagnostic(code(ERR_PNPM_TEAM_LS_SCOPE_REQUIRED))]
    LsScopeRequired,

    #[display(r#"Organization "@{scope}" not found in registry"#)]
    #[diagnostic(code(ERR_PNPM_ORG_NOT_FOUND))]
    OrgNotFound {
        #[error(not(source))]
        scope: String,
    },

    #[display(r#"Team "@{scope}:{team}" not found in registry"#)]
    #[diagnostic(code(ERR_PNPM_TEAM_NOT_FOUND))]
    TeamNotFound {
        #[error(not(source))]
        scope: String,
        #[error(not(source))]
        team: String,
    },

    #[display("You must be logged in to {action}. {body}")]
    #[diagnostic(code(ERR_PNPM_UNAUTHORIZED))]
    Unauthorized {
        #[error(not(source))]
        action: String,
        #[error(not(source))]
        body: String,
    },

    #[display("You do not have permission to {action}. {body}")]
    #[diagnostic(code(ERR_PNPM_FORBIDDEN))]
    Forbidden {
        #[error(not(source))]
        action: String,
        #[error(not(source))]
        body: String,
    },

    #[display("Organization or team not found. {body}")]
    #[diagnostic(code(ERR_PNPM_NOT_FOUND))]
    NotFound {
        #[error(not(source))]
        body: String,
    },

    #[display("Team operation failed due to conflict. {body}")]
    #[diagnostic(code(ERR_PNPM_TEAM_CONFLICT))]
    Conflict {
        #[error(not(source))]
        body: String,
    },

    #[display("Failed to {action}: {status} {status_text}. {body}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    RegistryWriteFailed {
        #[error(not(source))]
        action: String,
        status: u16,
        #[error(not(source))]
        status_text: String,
        #[error(not(source))]
        body: String,
    },

    #[display("Failed to {operation}: {reason}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    RegistryOperationFailed {
        #[error(not(source))]
        operation: &'static str,
        #[error(not(source))]
        reason: String,
    },
}

struct TeamContext<'a> {
    config: &'a Config,
    http_client: ThrottledClient,
    retry_opts: RetryOpts,
    registries: HashMap<String, String>,
    otp: Option<String>,
    parseable: bool,
    json: bool,
}

#[derive(Debug)]
struct ScopeTeam {
    scope: String,
    team: Option<String>,
}

fn parse_scope_team(spec: &str) -> Result<ScopeTeam, TeamError> {
    if !spec.starts_with('@') {
        return Err(TeamError::InvalidScope { spec: spec.to_string() });
    }
    let inner = &spec[1..];
    if inner.is_empty() {
        return Err(TeamError::InvalidScope { spec: spec.to_string() });
    }
    if let Some(colon) = inner.find(':') {
        let scope = &inner[..colon];
        let team = &inner[colon + 1..];
        if scope.is_empty() || team.is_empty() {
            return Err(TeamError::InvalidScope { spec: spec.to_string() });
        }
        Ok(ScopeTeam { scope: scope.to_string(), team: Some(team.to_string()) })
    } else {
        Ok(ScopeTeam { scope: inner.to_string(), team: None })
    }
}

fn team_url(registry_url: &str, scope: &str, team: &str) -> String {
    format!(
        "{}-/team/{}/{}",
        normalize_registry_url(registry_url),
        encode_uri_component(scope),
        encode_uri_component(team),
    )
}

fn team_user_url(registry_url: &str, scope: &str, team: &str) -> String {
    format!(
        "{}-/team/{}/{}/user",
        normalize_registry_url(registry_url),
        encode_uri_component(scope),
        encode_uri_component(team),
    )
}

fn org_team_url(registry_url: &str, scope: &str) -> String {
    format!("{}-/org/{}/team", normalize_registry_url(registry_url), encode_uri_component(scope))
}

#[derive(Deserialize)]
struct TeamInfo {
    name: String,
}

#[derive(Deserialize)]
struct UserInfo {
    name: String,
}

impl TeamArgs {
    pub async fn run(self, config: &Config) -> miette::Result<Option<String>> {
        let Some(subcommand) = self.params.first().map(String::as_str) else {
            return Err(TeamError::SubcommandRequired.into());
        };
        let context = self.context(config)?;
        match subcommand {
            "create" => team_create(&context, &self.params[1..]).await.map(Some),
            "destroy" => team_destroy(&context, &self.params[1..]).await.map(Some),
            "add" => team_add(&context, &self.params[1..]).await.map(Some),
            "rm" => team_rm(&context, &self.params[1..]).await.map(Some),
            "ls" | "list" => team_ls(&context, &self.params[1..]).await.map(Some),
            _ => {
                // When no subcommand is given, assume the first arg is a scope:team
                // and list members, or a scope and list teams.
                if self.params[0].starts_with('@') || self.params[0].starts_with(':') {
                    team_ls(&context, &self.params).await.map(Some)
                } else {
                    Err(TeamError::SubcommandRequired.into())
                }
            }
        }
    }

    fn context<'a>(&self, config: &'a Config) -> miette::Result<TeamContext<'a>> {
        let mut registries: HashMap<String, String> =
            config.resolved_registries().into_iter().collect();
        if let Some(registry) = &self.registry {
            registries.insert("default".to_string(), normalize_registry_url(registry));
        }
        // When an OTP is in play, restrict redirects to the configured
        // registry origins so a redirect cannot forward the `npm-otp` header
        // to another host (reqwest only strips standard auth headers on
        // cross-host redirects). Mirrors the `access` command's guard.
        //
        // Deliberate divergence from pnpm: the TypeScript fetch layer
        // follows a cross-host redirect after stripping `authorization` and
        // `npm-otp`, so the request proceeds without credentials and fails
        // at the target; here it fails at the redirect hop instead. reqwest
        // redirect policies cannot strip custom headers per hop, so matching
        // pnpm exactly needs a manual redirect loop in pacquet-network — a
        // follow-up that would cover `access` too.
        let redirect_guard = self.otp.as_ref().map(|_| {
            let origins: Vec<(String, String, Option<u16>)> = registries
                .values()
                .filter_map(|registry| {
                    reqwest::Url::parse(registry).ok().and_then(|url| {
                        url.host_str()
                            .map(|host| (url.scheme().to_string(), host.to_string(), url.port()))
                    })
                })
                .collect();
            let guard: RedirectGuard = Arc::new(move |target: &reqwest::Url| -> bool {
                origins.iter().any(|(scheme, host, port)| {
                    target.scheme() == scheme
                        && target.host_str() == Some(host.as_str())
                        && target.port() == *port
                })
            });
            guard
        });
        Ok(TeamContext {
            config,
            http_client: build_http_client(config, redirect_guard.as_ref())?,
            retry_opts: RetryOpts {
                retries: config.fetch_retries,
                factor: config.fetch_retry_factor,
                min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
                max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
            },
            registries,
            otp: self.otp.clone(),
            parseable: self.parseable,
            json: self.json,
        })
    }
}

async fn team_create(context: &TeamContext<'_>, params: &[String]) -> miette::Result<String> {
    let spec = params.first().ok_or(TeamError::CreateScopeRequired)?;
    let st = parse_scope_team(spec)?;
    let team = st.team.as_deref().ok_or(TeamError::CreateNameRequired)?;

    let registry_url = registry_for_scope(context, &st.scope);
    let auth_header = auth_header_for_registry(context, &st.scope);
    let url = org_team_url(&registry_url, &st.scope);
    let body = serde_json::json!({ "name": team }).to_string();

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let builder =
                client.put(&url).header("content-type", "application/json").body(body.clone());
            apply_auth_and_otp(builder, auth_header.as_deref(), context.otp.as_deref())
        })
        .await
        .map_err(|source| registry_operation_error("creating team", source))?;

    if response.status().is_success() {
        return Ok(format!("+{}:{}", st.scope, team));
    }
    Err(registry_error_from_response(response, format!(r#"create team "{}:{}""#, st.scope, team))
        .await)
}

async fn team_destroy(context: &TeamContext<'_>, params: &[String]) -> miette::Result<String> {
    let spec = params.first().ok_or(TeamError::DestroyScopeRequired)?;
    let st = parse_scope_team(spec)?;
    let team = st.team.as_deref().ok_or(TeamError::DestroyNameRequired)?;

    let registry_url = registry_for_scope(context, &st.scope);
    let auth_header = auth_header_for_registry(context, &st.scope);
    let url = team_url(&registry_url, &st.scope, team);

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let builder = client.delete(&url);
            apply_auth_and_otp(builder, auth_header.as_deref(), context.otp.as_deref())
        })
        .await
        .map_err(|source| registry_operation_error("destroying team", source))?;

    if response.status().is_success() {
        return Ok(format!("-{}:{}", st.scope, team));
    }
    Err(registry_error_from_response(response, format!(r#"destroy team "{}:{}""#, st.scope, team))
        .await)
}

async fn team_add(context: &TeamContext<'_>, params: &[String]) -> miette::Result<String> {
    if params.len() < 2 {
        return Err(TeamError::AddArgsRequired.into());
    }
    let st = parse_scope_team(&params[0])?;
    let team = st.team.as_deref().ok_or(TeamError::AddNameRequired)?;
    let username = &params[1];

    let registry_url = registry_for_scope(context, &st.scope);
    let auth_header = auth_header_for_registry(context, &st.scope);
    let url = team_user_url(&registry_url, &st.scope, team);
    let body = serde_json::json!({ "user": username }).to_string();

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let builder =
                client.put(&url).header("content-type", "application/json").body(body.clone());
            apply_auth_and_otp(builder, auth_header.as_deref(), context.otp.as_deref())
        })
        .await
        .map_err(|source| registry_operation_error("adding user to team", source))?;

    if response.status().is_success() {
        return Ok(format!("+{username} added to @{}:{team}", st.scope));
    }
    Err(registry_error_from_response(
        response,
        format!(r#"add user "{username}" to team "{}:{team}""#, st.scope),
    )
    .await)
}

async fn team_rm(context: &TeamContext<'_>, params: &[String]) -> miette::Result<String> {
    if params.len() < 2 {
        return Err(TeamError::RmArgsRequired.into());
    }
    let st = parse_scope_team(&params[0])?;
    let team = st.team.as_deref().ok_or(TeamError::RmNameRequired)?;
    let username = &params[1];

    let registry_url = registry_for_scope(context, &st.scope);
    let auth_header = auth_header_for_registry(context, &st.scope);
    let url = team_user_url(&registry_url, &st.scope, team);
    let body = serde_json::json!({ "user": username }).to_string();

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let builder =
                client.delete(&url).header("content-type", "application/json").body(body.clone());
            apply_auth_and_otp(builder, auth_header.as_deref(), context.otp.as_deref())
        })
        .await
        .map_err(|source| registry_operation_error("removing user from team", source))?;

    if response.status().is_success() {
        return Ok(format!("-{username} removed from @{}:{team}", st.scope));
    }
    Err(registry_error_from_response(
        response,
        format!(r#"remove user "{username}" from team "{}:{team}""#, st.scope),
    )
    .await)
}

async fn team_ls(context: &TeamContext<'_>, params: &[String]) -> miette::Result<String> {
    let spec = params.first().ok_or(TeamError::LsScopeRequired)?;
    let st = parse_scope_team(spec)?;

    let auth_header = auth_header_for_registry(context, &st.scope);

    if let Some(team) = &st.team {
        let members = fetch_team_members(context, &st.scope, team, auth_header.as_deref()).await?;
        render_members(&st.scope, team, &members, context.parseable, context.json)
    } else {
        let teams = fetch_teams(context, &st.scope, auth_header.as_deref()).await?;
        render_teams(&st.scope, &teams, context.parseable, context.json)
    }
}

async fn fetch_teams(
    context: &TeamContext<'_>,
    scope: &str,
    auth_header: Option<&str>,
) -> miette::Result<Vec<TeamInfo>> {
    let registry_url = registry_for_scope(context, scope);
    let url = org_team_url(&registry_url, scope);
    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let mut builder = client.get(&url);
            if let Some(auth) = auth_header {
                builder = builder.header("authorization", auth);
            }
            builder
        })
        .await
        .map_err(|source| registry_operation_error("fetching teams", source))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(TeamError::OrgNotFound { scope: scope.to_string() }.into());
    }
    if !response.status().is_success() {
        return Err(registry_error_from_response(
            response,
            format!(r#"fetch teams for "@{scope}""#),
        )
        .await);
    }

    let body = read_limited_body(response, TEAM_BODY_LIMIT)
        .await
        .map_err(|source| registry_operation_error("reading teams response", source))?;
    serde_json::from_slice(&body.bytes)
        .into_diagnostic()
        .map_err(|source| registry_operation_error("parsing teams response", source))
}

async fn fetch_team_members(
    context: &TeamContext<'_>,
    scope: &str,
    team: &str,
    auth_header: Option<&str>,
) -> miette::Result<Vec<UserInfo>> {
    let registry_url = registry_for_scope(context, scope);
    let url = team_user_url(&registry_url, scope, team);
    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let mut builder = client.get(&url);
            if let Some(auth) = auth_header {
                builder = builder.header("authorization", auth);
            }
            builder
        })
        .await
        .map_err(|source| registry_operation_error("fetching team members", source))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(
            TeamError::TeamNotFound { scope: scope.to_string(), team: team.to_string() }.into()
        );
    }
    if !response.status().is_success() {
        return Err(registry_error_from_response(
            response,
            format!(r#"fetch team members for "@{scope}:{team}""#),
        )
        .await);
    }

    let body = read_limited_body(response, TEAM_BODY_LIMIT)
        .await
        .map_err(|source| registry_operation_error("reading team members response", source))?;
    serde_json::from_slice(&body.bytes)
        .into_diagnostic()
        .map_err(|source| registry_operation_error("parsing team members response", source))
}

fn render_teams(
    scope: &str,
    teams: &[TeamInfo],
    parseable: bool,
    json: bool,
) -> miette::Result<String> {
    if json {
        let names: Vec<&str> = teams.iter().map(|team| team.name.as_str()).collect();
        return serde_json::to_string_pretty(&names)
            .into_diagnostic()
            .map_err(|source| registry_operation_error("serializing teams as JSON", source));
    }

    if parseable {
        let lines: Vec<&str> = teams.iter().map(|team| team.name.as_str()).collect();
        return Ok(lines.join("\n"));
    }

    if teams.is_empty() {
        return Ok(format!("@{scope} has no teams"));
    }

    let mut lines = vec![format!("@{scope} has the following teams:")];
    for team in teams {
        lines.push(format!("  @{scope}:{}", team.name));
    }
    Ok(lines.join("\n"))
}

fn render_members(
    scope: &str,
    team: &str,
    members: &[UserInfo],
    parseable: bool,
    json: bool,
) -> miette::Result<String> {
    if json {
        let names: Vec<&str> = members.iter().map(|member| member.name.as_str()).collect();
        return serde_json::to_string_pretty(&names)
            .into_diagnostic()
            .map_err(|source| registry_operation_error("serializing members as JSON", source));
    }

    if parseable {
        let lines: Vec<&str> = members.iter().map(|member| member.name.as_str()).collect();
        return Ok(lines.join("\n"));
    }

    if members.is_empty() {
        return Ok(format!("@{scope}:{team} has no members"));
    }

    let mut lines = vec![format!("@{scope}:{team} has the following members:")];
    for member in members {
        lines.push(format!("  {}", member.name));
    }
    Ok(lines.join("\n"))
}

fn registry_for_scope(context: &TeamContext<'_>, scope: &str) -> String {
    let pkg_name = format!("@{scope}/_");
    pick_registry_for_package(&context.registries, &pkg_name, None)
}

fn auth_header_for_registry(context: &TeamContext<'_>, scope: &str) -> Option<String> {
    let registry_url = registry_for_scope(context, scope);
    let pkg_name = format!("@{scope}/_");
    context.config.auth_headers.for_url_with_package(&registry_url, Some(&pkg_name))
}

fn apply_auth_and_otp(
    mut builder: reqwest::RequestBuilder,
    auth_header: Option<&str>,
    otp: Option<&str>,
) -> reqwest::RequestBuilder {
    if let Some(auth) = auth_header {
        builder = builder.header("authorization", auth);
    }
    if let Some(otp) = otp {
        builder = builder.header("npm-otp", otp);
    }
    builder
}

fn build_http_client(
    config: &Config,
    redirect_guard: Option<&RedirectGuard>,
) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs_with_guard(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &NetworkSettings {
            network_concurrency: config.network_concurrency,
            fetch_timeout: Duration::from_millis(config.fetch_timeout),
            user_agent: config.user_agent.clone(),
        },
        redirect_guard,
    )
    .into_diagnostic()
    .wrap_err("create the network client for team command")
}

fn normalize_registry_url(registry_url: &str) -> String {
    if registry_url.ends_with('/') { registry_url.to_string() } else { format!("{registry_url}/") }
}

struct LimitedBody {
    bytes: Vec<u8>,
}

impl LimitedBody {
    /// Renders the body for embedding in a one-line error message: control
    /// characters (including newlines) are stripped and the result is capped
    /// at 500 characters, matching the TypeScript implementation.
    fn into_display_string(self) -> String {
        String::from_utf8_lossy(&self.bytes)
            .chars()
            .filter(|ch| !ch.is_control())
            .take(500)
            .collect()
    }
}

async fn read_limited_body(
    response: Response,
    limit: usize,
) -> Result<LimitedBody, reqwest::Error> {
    let header_exceeds_limit =
        response.content_length().is_some_and(|length| length > limit as u64);
    let mut bytes = Vec::new();
    let mut truncated = header_exceeds_limit;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let remaining = limit.saturating_sub(bytes.len());
        if chunk.len() > remaining {
            bytes.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    if truncated {
        let body = String::from_utf8_lossy(&bytes);
        let mut body = sanitize::sanitize(&body).into_owned();
        if !body.is_empty() && !body.chars().next_back().is_some_and(char::is_whitespace) {
            body.push(' ');
        }
        body.push_str("(response body truncated)");
        Ok(LimitedBody { bytes: body.into_bytes() })
    } else {
        Ok(LimitedBody { bytes })
    }
}

fn registry_operation_error<ErrorType>(operation: &'static str, error: ErrorType) -> miette::Report
where
    ErrorType: std::fmt::Display,
{
    TeamError::RegistryOperationFailed {
        operation,
        reason: redact_url_credentials(&error.to_string()),
    }
    .into()
}

async fn registry_error_from_response(response: Response, action: String) -> miette::Report {
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();
    let body = match read_limited_body(response, TEAM_ERROR_BODY_LIMIT).await {
        Ok(body) => body.into_display_string(),
        Err(_) => String::new(),
    };

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return TeamError::Unauthorized { action, body }.into();
    }
    if status == reqwest::StatusCode::FORBIDDEN {
        return TeamError::Forbidden { action, body }.into();
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return TeamError::NotFound { body }.into();
    }
    if status == reqwest::StatusCode::CONFLICT {
        return TeamError::Conflict { body }.into();
    }
    TeamError::RegistryWriteFailed { action, status: status.as_u16(), status_text, body }.into()
}

#[cfg(test)]
mod tests;
