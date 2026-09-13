pub use errors::TeamError;

use super::sanitize;
use clap::Args;
use derive_more::{Display, Error};

use miette::{Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_network::{
    RedirectGuard, RetryOpts, ThrottledClient, encode_uri_component, redact_url_credentials,
    send_with_retry,
};
use pnpm_resolving_npm_resolver::pick_registry_for_package;
use registry::{
    TeamInfo, UserInfo, apply_auth_and_otp, auth_header_for_registry, build_http_client,
    fetch_team_members, fetch_teams, normalize_registry_url, org_team_url,
    registry_error_from_response, registry_for_scope, registry_operation_error, team_url,
    team_user_url,
};
use reqwest::Response;
use serde::Deserialize;
use std::collections::HashMap;

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
        return Err(TeamError::InvalidScope {
            spec: spec.to_string(),
        });
    }
    let inner = &spec[1..];
    if inner.is_empty() {
        return Err(TeamError::InvalidScope {
            spec: spec.to_string(),
        });
    }
    if let Some(colon) = inner.find(':') {
        let scope = &inner[..colon];
        let team = &inner[colon + 1..];
        if scope.is_empty() || team.is_empty() {
            return Err(TeamError::InvalidScope {
                spec: spec.to_string(),
            });
        }
        Ok(ScopeTeam {
            scope: scope.to_string(),
            team: Some(team.to_string()),
        })
    } else {
        Ok(ScopeTeam {
            scope: inner.to_string(),
            team: None,
        })
    }
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
        let mut registries: HashMap<String, String> = config
            .resolved_registries()
            .into_iter()
            .collect();
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
        // pnpm exactly needs a manual redirect loop in pnpm-network — a
        // follow-up that would cover `access` too.
        let redirect_guard = self.otp
            .as_ref()
            .map(|_| {
                super::registry_client::registry_redirect_guard(
                    registries.values().map(String::as_str),
                )
            });
        Ok(TeamContext {
            config,
            http_client: build_http_client(config, redirect_guard.as_ref())?,
            retry_opts: config.retry_opts(),
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
    let auth_header = auth_header_for_registry(context, &st.scope)?;
    let url = org_team_url(&registry_url, &st.scope);
    let body = serde_json::json!({ "name": team }).to_string();

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let builder = client
                .put(&url)
                .header("content-type", "application/json")
                .body(body.clone());
            apply_auth_and_otp(builder, Some(&auth_header), context.otp.as_deref())
        })
        .await
        .map_err(|source| registry_operation_error("creating team", source))?;

    if response.status().is_success() {
        return Ok(format!("+{}:{}", st.scope, team));
    }
    Err(
        registry_error_from_response(response, format!(r#"create team "{}:{}""#, st.scope, team))
            .await,
    )
}

async fn team_destroy(context: &TeamContext<'_>, params: &[String]) -> miette::Result<String> {
    let spec = params.first().ok_or(TeamError::DestroyScopeRequired)?;
    let st = parse_scope_team(spec)?;
    let team = st.team.as_deref().ok_or(TeamError::DestroyNameRequired)?;

    let registry_url = registry_for_scope(context, &st.scope);
    let auth_header = auth_header_for_registry(context, &st.scope)?;
    let url = team_url(&registry_url, &st.scope, team);

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let builder = client.delete(&url);
            apply_auth_and_otp(builder, Some(&auth_header), context.otp.as_deref())
        })
        .await
        .map_err(|source| registry_operation_error("destroying team", source))?;

    if response.status().is_success() {
        return Ok(format!("-{}:{}", st.scope, team));
    }
    Err(
        registry_error_from_response(response, format!(r#"destroy team "{}:{}""#, st.scope, team))
            .await,
    )
}

async fn team_add(context: &TeamContext<'_>, params: &[String]) -> miette::Result<String> {
    if params.len() < 2 {
        return Err(TeamError::AddArgsRequired.into());
    }
    let st = parse_scope_team(&params[0])?;
    let team = st.team.as_deref().ok_or(TeamError::AddNameRequired)?;
    let username = &params[1];

    let registry_url = registry_for_scope(context, &st.scope);
    let auth_header = auth_header_for_registry(context, &st.scope)?;
    let url = team_user_url(&registry_url, &st.scope, team);
    let body = serde_json::json!({ "user": username }).to_string();

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let builder = client
                .put(&url)
                .header("content-type", "application/json")
                .body(body.clone());
            apply_auth_and_otp(builder, Some(&auth_header), context.otp.as_deref())
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
    let auth_header = auth_header_for_registry(context, &st.scope)?;
    let url = team_user_url(&registry_url, &st.scope, team);
    let body = serde_json::json!({ "user": username }).to_string();

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let builder = client
                .delete(&url)
                .header("content-type", "application/json")
                .body(body.clone());
            apply_auth_and_otp(builder, Some(&auth_header), context.otp.as_deref())
        })
        .await
        .map_err(|source| registry_operation_error("removing user from team", source))?;

    if response.status().is_success() {
        return Ok(format!("-{username} removed from @{}:{team}", st.scope));
    }
    Err(registry_error_from_response(
        response,
        format!(
            r#"remove user "{username}" from team "{}:{team}""#,
            st.scope,
        ),
    )
    .await)
}

async fn team_ls(context: &TeamContext<'_>, params: &[String]) -> miette::Result<String> {
    let spec = params.first().ok_or(TeamError::LsScopeRequired)?;
    let st = parse_scope_team(spec)?;

    let auth_header = auth_header_for_registry(context, &st.scope)?;

    if let Some(team) = &st.team {
        let members = fetch_team_members(context, &st.scope, team, Some(&auth_header)).await?;
        render_members(&st.scope, team, &members, context.parseable, context.json)
    } else {
        let teams = fetch_teams(context, &st.scope, Some(&auth_header)).await?;
        render_teams(&st.scope, &teams, context.parseable, context.json)
    }
}

fn render_teams(
    scope: &str,
    teams: &[TeamInfo],
    parseable: bool,
    json: bool,
) -> miette::Result<String> {
    if json {
        let names: Vec<&str> = teams
            .iter()
            .map(|team| team.name.as_str())
            .collect();
        return serde_json::to_string_pretty(&names)
            .into_diagnostic()
            .map_err(|source| registry_operation_error("serializing teams as JSON", source));
    }

    if parseable {
        let lines: Vec<&str> = teams
            .iter()
            .map(|team| team.name.as_str())
            .collect();
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
        let names: Vec<&str> = members
            .iter()
            .map(|member| member.name.as_str())
            .collect();
        return serde_json::to_string_pretty(&names)
            .into_diagnostic()
            .map_err(|source| registry_operation_error("serializing members as JSON", source));
    }

    if parseable {
        let lines: Vec<&str> = members
            .iter()
            .map(|member| member.name.as_str())
            .collect();
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

#[cfg(test)]
mod tests;

mod registry;

mod errors;
