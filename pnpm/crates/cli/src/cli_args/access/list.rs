use super::{
    AccessError,
    registry::{
        AccessContext, auth_header_for_list, fetch_error_from_response, list_packages_url,
        package_collaborators_url, registry_for_list, registry_for_package, send_get,
    },
};
use crate::cli_args::registry_client::auth_header_for_package;
use miette::{Context, IntoDiagnostic};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub(super) async fn list_packages(
    context: &AccessContext<'_>,
    params: &[String],
) -> miette::Result<String> {
    let registry = registry_for_list(context, params);
    let auth_header = auth_header_for_list(context, params, &registry);
    let url = list_packages_url(&registry, params);
    fetch_list_response(context, &url, auth_header.as_deref()).await
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
#[derive(Serialize, Deserialize)]
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

pub(super) async fn list_collaborators(
    context: &AccessContext<'_>,
    params: &[String],
) -> miette::Result<String> {
    let package_name = params.first().ok_or(AccessError::ListCollaboratorsPackageRequired)?;
    let user = params.get(1).map(String::as_str);

    let registry = registry_for_package(context, package_name);
    let auth_header = auth_header_for_package(context.config, &registry, package_name);
    let url = package_collaborators_url(&registry, package_name, user);

    let (_guard, response) = send_get(context, &url, auth_header.as_deref())
        .await
        .map_err(reqwest::Error::without_url)
        .into_diagnostic()
        .wrap_err("requesting the registry collaborators endpoint")?;

    if response.status() == StatusCode::NOT_FOUND {
        return Err(AccessError::PackageNotFound { package_name: package_name.clone() }.into());
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
            let user = entry.user.or(entry.username).unwrap_or_else(|| "unknown".to_string());
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
