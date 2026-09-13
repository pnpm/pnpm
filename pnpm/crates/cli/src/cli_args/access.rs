pub use errors::AccessError;

use clap::Args;
use derive_more::{Display, Error};

use miette::{Context, Diagnostic, IntoDiagnostic};
use permissions::{get_status, grant_access, revoke_access, set_mfa, set_status};
use pnpm_config::Config;
use pnpm_network::{
    RedirectGuard, RetryOpts, ThrottledClient, ThrottledClientGuard, encode_uri_component,
    redact_and_sanitize, send_with_retry,
};
use registry::{
    AccessContext, build_access_context, escaped_package_name, fetch_error_from_response,
    normalize_registry_url, send_get, send_json, write_error_from_response,
};
use reqwest::{Method, Response, StatusCode};
use std::collections::HashMap;

#[derive(Debug, Args)]
pub struct AccessArgs {
    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    /// Output results in JSON format.
    #[clap(long)]
    pub json: bool,

    /// One-time password for registries that require two-factor authentication.
    #[clap(long)]
    pub otp: Option<String>,

    /// Subcommand and arguments.
    pub params: Vec<String>,
}

impl AccessArgs {
    pub async fn run(mut self, config: &Config) -> miette::Result<Option<String>> {
        let mut params = std::mem::take(&mut self.params);
        let context = build_access_context(&self, config)?;

        if params.is_empty() {
            return Err(AccessError::SubcommandRequired.into());
        }

        let first = params.remove(0);
        let second = if params.is_empty() {
            None
        } else {
            Some(params.remove(0))
        };

        let (action, rest) = parse_access_action(&first, second, params)?;

        match action {
            "list_packages" => list_packages(&context, &rest).await.map(Some),
            "list_collaborators" => list_collaborators(&context, &rest).await.map(Some),
            "get_status" => get_status(&context, &rest).await.map(Some),
            "set_status" => set_status(&context, &rest).await.map(Some),
            "set_mfa" => set_mfa(&context, &rest).await.map(Some),
            "grant" => grant_access(&context, &rest).await.map(Some),
            "revoke" => revoke_access(&context, &rest).await.map(Some),
            _ => unreachable!(),
        }
    }
}

/// The subcommand `params` name, and the arguments to pass it.
///
/// `pnpm access` takes its subcommand as one or two leading params,
/// with the rest — plus, for the shorthands, a value derived from the
/// subcommand itself — forming its arguments.
fn parse_access_action(
    first: &str,
    second: Option<String>,
    params: Vec<String>,
) -> Result<(&'static str, Vec<String>), AccessError> {
    let action = match (first, second.as_deref()) {
        ("list", Some("packages")) | ("ls", None | Some("packages")) => {
            ("list_packages", access_args(None, None, params))
        }
        ("list", Some("collaborators")) => ("list_collaborators", access_args(None, None, params)),
        ("get", Some("status")) => ("get_status", access_args(None, None, params)),
        ("set", Some(status_val)) if status_val.starts_with("status=") => {
            let status = format!("status={}", &status_val["status=".len()..]);
            ("set_status", access_args(Some(status), None, params))
        }
        ("set", Some(mfa_val)) if mfa_val.starts_with("mfa=") => {
            let mfa = format!("mfa={}", &mfa_val["mfa=".len()..]);
            ("set_mfa", access_args(Some(mfa), None, params))
        }
        ("public", _) => (
            "set_status",
            access_args(Some("status=public".to_string()), second, params),
        ),
        ("restricted", _) => (
            "set_status",
            access_args(Some("status=restricted".to_string()), second, params),
        ),
        ("grant", _) => ("grant", access_args(None, second, params)),
        ("revoke", _) => ("revoke", access_args(None, second, params)),
        _ => {
            let parts = access_args(Some(first.to_owned()), second, params);
            return Err(AccessError::UnknownSubcommand {
                cmd: parts.join(" "),
            });
        }
    };
    Ok(action)
}

fn access_args(lead: Option<String>, second: Option<String>, params: Vec<String>) -> Vec<String> {
    lead
        .into_iter()
        .chain(second)
        .chain(params)
        .collect()
}

async fn list_packages(context: &AccessContext<'_>, params: &[String]) -> miette::Result<String> {
    let auth_header = context.config.auth_headers.for_url(&context.registry);
    let url = list_packages_url(&context.registry, params);
    fetch_list_response(context, &url, auth_header.as_deref()).await
}

/// The listing endpoint for whichever entity the params name: a
/// `<scope>:<team>` team, an `@<org>`, a user, or — with no params — the
/// packages the credentials themselves reach.
fn list_packages_url(registry: &str, params: &[String]) -> String {
    let Some(raw) = params.first() else {
        return format!("{}-/-/package?format=cli", normalize_registry_url(registry));
    };
    // A team is `<scope>:<team>` and the scope may carry its `@`, so the
    // separator decides before the prefix does.
    if !raw.contains(':') {
        return match raw.strip_prefix('@') {
            Some(org_name) => format!(
                "{}-/org/{}/package?format=cli",
                normalize_registry_url(registry),
                encode_uri_component(org_name),
            ),
            None => format!(
                "{}-/user/{}/package?format=cli",
                normalize_registry_url(registry),
                encode_uri_component(raw),
            ),
        };
    }
    let parts: Vec<&str> = raw.splitn(2, ':').collect();
    let team = parts.get(1).unwrap_or(&"");
    let team_path = if team.is_empty() {
        String::new()
    } else {
        format!("{}/", encode_uri_component(team))
    };
    format!(
        "{}-/team/{}/{}package?format=cli",
        normalize_registry_url(registry),
        encode_uri_component(
            parts[0]
                .strip_prefix('@')
                .unwrap_or(parts[0])
        ),
        team_path,
    )
}

async fn fetch_list_response(
    context: &AccessContext<'_>,
    url: &str,
    auth_header: Option<&str>,
) -> miette::Result<String> {
    let (_guard, response) = send_get(context, url, auth_header).await
        .map_err(reqwest::Error::without_url)
        .into_diagnostic()
        .wrap_err("requesting the registry access list endpoint")?;

    if !response.status().is_success() {
        return Err(fetch_error_from_response(response, "list packages from").await);
    }

    let data: HashMap<String, serde_json::Value> = response
        .json()
        .await
        .into_diagnostic()
        .wrap_err("parsing the access list response")?;

    if context.json {
        let output = serde_json::to_string_pretty(&data)
            .into_diagnostic()
            .wrap_err("serializing access list to JSON")?;
        return Ok(output);
    }

    let mut lines: Vec<String> = data
        .into_iter()
        .map(|(pkg, access)| {
            if let Some(access_str) = access.as_str() {
                format!("{pkg}: {access_str}")
            } else {
                pkg
            }
        })
        .collect();
    lines.sort();
    Ok(lines.join("\n"))
}

/// One entry of the registry's collaborators listing.
#[derive(serde::Serialize, serde::Deserialize)]
struct CollaboratorEntry {
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    permissions: Option<String>,
}

async fn list_collaborators(
    context: &AccessContext<'_>,
    params: &[String],
) -> miette::Result<String> {
    let package_name = params.first().ok_or(AccessError::ListCollaboratorsPackageRequired)?;
    let auth_header =
        context.config.auth_headers.for_url_with_package(&context.registry, Some(package_name));
    let url = collaborators_url(
        &context.registry,
        package_name,
        params.get(1).map(String::as_str),
    );

    let (_guard, response) = send_get(context, &url, auth_header.as_deref()).await
        .map_err(reqwest::Error::without_url)
        .into_diagnostic()
        .wrap_err("requesting the registry collaborators endpoint")?;

    if response.status() == StatusCode::NOT_FOUND {
        return Err(AccessError::PackageNotFound {
            package_name: package_name.clone(),
        }
        .into());
    }
    if !response.status().is_success() {
        return Err(fetch_error_from_response(response, "list collaborators for").await);
    }

    let entries: Vec<CollaboratorEntry> = response
        .json()
        .await
        .into_diagnostic()
        .wrap_err("parsing the collaborators response")?;

    if context.json {
        let output = serde_json::to_string_pretty(&entries)
            .into_diagnostic()
            .wrap_err("serializing collaborators to JSON")?;
        return Ok(output);
    }

    Ok(render_collaborators(entries))
}

/// One `user <email>: permissions` line per collaborator, sorted.
fn render_collaborators(entries: Vec<CollaboratorEntry>) -> String {
    let mut lines: Vec<String> = entries
        .into_iter()
        .map(|entry| {
            let user = entry.user
                .or(entry.username)
                .unwrap_or_else(|| "unknown".to_string());
            let email = entry.email.unwrap_or_default();
            let permissions = entry.permissions.unwrap_or_else(|| "read-only".to_string());
            if email.is_empty() {
                format!("{user}: {permissions}")
            } else {
                format!("{user} <{email}>: {permissions}")
            }
        })
        .collect();
    lines.sort();
    lines.join("\n")
}

#[cfg(test)]
mod tests;

mod registry;

mod permissions;

mod errors;

fn collaborators_url(registry: &str, package_name: &str, user: Option<&str>) -> String {
    let base = format!(
        "{}-/package/{}/collaborators?format=cli",
        normalize_registry_url(registry),
        escaped_package_name(package_name),
    );
    match user {
        Some(u) => format!("{base}&user={}", encode_uri_component(u)),
        None => base,
    }
}
