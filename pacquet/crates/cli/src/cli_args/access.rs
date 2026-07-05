use clap::Args;
use derive_more::{Display, Error};
use futures_util::StreamExt as _;
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_network::{
    NetworkSettings, RetryOpts, ThrottledClient, encode_uri_component, send_with_retry,
};
use reqwest::{Response, StatusCode};
use std::{collections::HashMap, time::Duration};

const ACCESS_ERROR_BODY_LIMIT: usize = 64 * 1024;

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

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum AccessError {
    #[display(
        "A subcommand is required (e.g., \"list packages\", \"get status\", \"set status=public\", \"grant\", \"revoke\")"
    )]
    #[diagnostic(code(ERR_PNPM_ACCESS_SUBCOMMAND_REQUIRED))]
    SubcommandRequired,

    #[display("Unknown subcommand: {cmd}")]
    #[diagnostic(code(ERR_PNPM_ACCESS_UNKNOWN_SUBCOMMAND))]
    UnknownSubcommand {
        #[error(not(source))]
        cmd: String,
    },

    #[display("Package name is required (e.g., pnpm access get status @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_GET_STATUS_PACKAGE_REQUIRED))]
    GetStatusPackageRequired,

    #[display("Package name is required (e.g., pnpm access list collaborators @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_LIST_COLLABORATORS_PACKAGE_REQUIRED))]
    ListCollaboratorsPackageRequired,

    #[display("Package visibility is required (e.g., pnpm access set status=public @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_STATUS_REQUIRED))]
    SetStatusRequired,

    #[display(r#"Invalid access value "{value}". Must be "public" or "private"."#)]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_STATUS_INVALID))]
    SetStatusInvalid {
        #[error(not(source))]
        value: String,
    },

    #[display("Package name is required (e.g., pnpm access set status=public @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_STATUS_PACKAGE_REQUIRED))]
    SetStatusPackageRequired,

    #[display(
        "Access settings can only be changed for scoped packages (@scope/name). Unscoped packages are always public."
    )]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_STATUS_UNSCOPED))]
    SetStatusUnscoped,

    #[display("MFA level is required (e.g., pnpm access set mfa=automation @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_MFA_REQUIRED))]
    SetMfaRequired,

    #[display(r#"Invalid MFA value "{value}". Must be "none", "publish", or "automation"."#)]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_MFA_INVALID))]
    SetMfaInvalid {
        #[error(not(source))]
        value: String,
    },

    #[display("Package name is required (e.g., pnpm access set mfa=automation @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_MFA_PACKAGE_REQUIRED))]
    SetMfaPackageRequired,

    #[display(
        "Permissions and scope:team are required (e.g., pnpm access grant read-only @scope:developers @scope/pkg)"
    )]
    #[diagnostic(code(ERR_PNPM_ACCESS_GRANT_ARGS_REQUIRED))]
    GrantArgsRequired,

    #[display(r#"Invalid permissions "{value}". Must be "read-only" or "read-write"."#)]
    #[diagnostic(code(ERR_PNPM_ACCESS_GRANT_INVALID_PERMISSIONS))]
    GrantInvalidPermissions {
        #[error(not(source))]
        value: String,
    },

    #[display(r#"Invalid team "{team}". Format must be "scope:team". "#)]
    #[diagnostic(code(ERR_PNPM_ACCESS_GRANT_INVALID_TEAM))]
    GrantInvalidTeam {
        #[error(not(source))]
        team: String,
    },

    #[display(
        "Package name is required (e.g., pnpm access grant read-only @scope:developers @scope/pkg)"
    )]
    #[diagnostic(code(ERR_PNPM_ACCESS_GRANT_PACKAGE_REQUIRED))]
    GrantPackageRequired,

    #[display(
        "scope:team and package name are required (e.g., pnpm access revoke @scope:developers @scope/pkg)"
    )]
    #[diagnostic(code(ERR_PNPM_ACCESS_REVOKE_ARGS_REQUIRED))]
    RevokeArgsRequired,

    #[display(r#"Invalid team "{team}". Format must be "scope:team". "#)]
    #[diagnostic(code(ERR_PNPM_ACCESS_REVOKE_INVALID_TEAM))]
    RevokeInvalidTeam {
        #[error(not(source))]
        team: String,
    },

    #[display("Package name is required (e.g., pnpm access revoke @scope:developers @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_REVOKE_PACKAGE_REQUIRED))]
    RevokePackageRequired,

    #[display(r#"Package "{package_name}" not found in registry"#)]
    #[diagnostic(code(ERR_PNPM_PACKAGE_NOT_FOUND))]
    PackageNotFound {
        #[error(not(source))]
        package_name: String,
    },

    #[display("You must be logged in to {action} packages. {body}")]
    #[diagnostic(code(ERR_PNPM_UNAUTHORIZED))]
    Unauthorized {
        #[error(not(source))]
        action: String,
        #[error(not(source))]
        body: String,
    },

    #[display("You do not have permission to {action} this package. {body}")]
    #[diagnostic(code(ERR_PNPM_FORBIDDEN))]
    Forbidden {
        #[error(not(source))]
        action: String,
        #[error(not(source))]
        body: String,
    },

    #[display("Invalid request: {body}")]
    #[diagnostic(code(ERR_PNPM_ACCESS_VALIDATION_ERROR))]
    ValidationError {
        #[error(not(source))]
        body: String,
    },

    #[display("Failed to {action} package: {status} {status_text}. {body}")]
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

    #[display("Failed to {action} packages: {status} {status_text}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    RegistryFetchFailed {
        #[error(not(source))]
        action: String,
        status: u16,
        #[error(not(source))]
        status_text: String,
    },
}

struct AccessContext<'a> {
    config: &'a Config,
    http_client: ThrottledClient,
    retry_opts: RetryOpts,
    registry: String,
    json: bool,
    otp: Option<String>,
}

impl AccessArgs {
    pub async fn run(mut self, config: &Config) -> miette::Result<Option<String>> {
        let mut params = std::mem::take(&mut self.params);
        let context = build_access_context(&self, config)?;

        if params.is_empty() {
            return Err(AccessError::SubcommandRequired.into());
        }

        let first = params.remove(0);
        let second = if params.is_empty() { None } else { Some(params.remove(0)) };

        let (action, rest) = match (first.as_str(), second.as_deref()) {
            ("list", Some("packages") | None) => ("list_packages", params),
            ("ls", rest) => ("list_packages", {
                if rest == Some("packages") {
                    params
                } else {
                    let mut p: Vec<String> = Vec::new();
                    if let Some(r) = rest {
                        p.push(r.to_string());
                    }
                    p.extend(params);
                    p
                }
            }),
            ("list", Some("collaborators")) => ("list_collaborators", params),
            ("get", Some("status")) => ("get_status", params),
            ("set", Some(status_val)) if status_val.starts_with("status=") => {
                let mut p: Vec<String> = vec![format!("status={}", &status_val[7..])];
                p.extend(params);
                ("set_status", p)
            }
            ("set", Some(mfa_val)) if mfa_val.starts_with("mfa=") => {
                let mut p: Vec<String> = vec![format!("mfa={}", &mfa_val[4..])];
                p.extend(params);
                ("set_mfa", p)
            }
            ("public", _) => {
                let mut p: Vec<String> = vec!["status=public".to_string()];
                if let Some(s) = second {
                    p.push(s);
                }
                p.extend(params);
                ("set_status", p)
            }
            ("restricted", _) => {
                let mut p: Vec<String> = vec!["status=restricted".to_string()];
                if let Some(s) = second {
                    p.push(s);
                }
                p.extend(params);
                ("set_status", p)
            }
            ("grant", _) => {
                let mut p: Vec<String> = Vec::new();
                if let Some(s) = second {
                    p.push(s);
                }
                p.extend(params);
                ("grant", p)
            }
            ("revoke", _) => {
                let mut p: Vec<String> = Vec::new();
                if let Some(s) = second {
                    p.push(s);
                }
                p.extend(params);
                ("revoke", p)
            }
            _ => {
                return Err(AccessError::UnknownSubcommand {
                    cmd: match second {
                        Some(s) => format!("{first} {s}"),
                        None => first.clone(),
                    },
                }
                .into());
            }
        };

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

fn build_access_context<'a>(
    args: &AccessArgs,
    config: &'a Config,
) -> miette::Result<AccessContext<'a>> {
    let registry =
        args.registry.as_deref().map_or_else(|| config.registry.clone(), normalize_registry_url);

    Ok(AccessContext {
        config,
        http_client: build_http_client(config)?,
        retry_opts: RetryOpts {
            retries: config.fetch_retries,
            factor: config.fetch_retry_factor,
            min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
            max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
        },
        registry,
        json: args.json,
        otp: args.otp.clone(),
    })
}

async fn list_packages(context: &AccessContext<'_>, params: &[String]) -> miette::Result<String> {
    let (entity_type, entity, remaining) = if params.is_empty() {
        (None, None, &[][..])
    } else {
        let raw = &params[0];
        if raw.contains(':') {
            (Some("team"), Some(raw.clone()), &params[1..])
        } else if let Some(org_name) = raw.strip_prefix('@') {
            (Some("org"), Some(org_name.to_string()), &params[1..])
        } else {
            (Some("user"), Some(raw.clone()), &params[1..])
        }
    };

    let auth_header = context.config.auth_headers.for_url(&context.registry);

    let url = match (entity_type, entity) {
        (Some("team"), Some(team_str)) => {
            let parts: Vec<&str> = team_str.splitn(2, ':').collect();
            let scope = parts[0];
            let team = parts.get(1).unwrap_or(&"");
            let team_path = if team.is_empty() {
                String::new()
            } else {
                format!("{}/", encode_uri_component(team))
            };
            format!(
                "{}-/team/{}{}package?format=cli",
                normalize_registry_url(&context.registry),
                encode_uri_component(scope),
                team_path,
            )
        }
        (Some("org"), Some(org)) => {
            format!(
                "{}-/org/{}/package?format=cli",
                normalize_registry_url(&context.registry),
                encode_uri_component(&org),
            )
        }
        (Some("user"), Some(user)) => {
            format!(
                "{}-/user/{}/package?format=cli",
                normalize_registry_url(&context.registry),
                encode_uri_component(&user),
            )
        }
        _ => {
            if let Some(pkg) = remaining.first() {
                format!(
                    "{}-/package/{}/collaborators?format=cli",
                    normalize_registry_url(&context.registry),
                    escaped_package_name(pkg),
                )
            } else {
                format!("{}-/-/package?format=cli", normalize_registry_url(&context.registry))
            }
        }
    };

    fetch_list_response(context, &url, auth_header.as_deref()).await
}

async fn fetch_list_response(
    context: &AccessContext<'_>,
    url: &str,
    auth_header: Option<&str>,
) -> miette::Result<String> {
    let (_guard, response) =
        send_with_retry(&context.http_client, url, context.retry_opts, |client| {
            let mut builder = client.get(url);
            if let Some(auth) = auth_header {
                builder = builder.header("authorization", auth);
            }
            builder
        })
        .await
        .into_diagnostic()
        .wrap_err("requesting the registry access list endpoint")?;

    if !response.status().is_success() {
        return Err(fetch_error_from_response(response, "list packages from").await);
    }

    let data: HashMap<String, serde_json::Value> =
        response.json().await.into_diagnostic().wrap_err("parsing the access list response")?;

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

async fn list_collaborators(
    context: &AccessContext<'_>,
    params: &[String],
) -> miette::Result<String> {
    let package_name = params.first().ok_or(AccessError::ListCollaboratorsPackageRequired)?;
    let user = params.get(1);

    let auth_header =
        context.config.auth_headers.for_url_with_package(&context.registry, Some(package_name));

    let base = format!(
        "{}-/package/{}/collaborators?format=cli",
        normalize_registry_url(&context.registry),
        escaped_package_name(package_name),
    );
    let url = match user {
        Some(u) => format!("{base}&user={}", encode_uri_component(u)),
        None => base,
    };

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let mut builder = client.get(&url);
            if let Some(auth) = auth_header.as_deref() {
                builder = builder.header("authorization", auth);
            }
            builder
        })
        .await
        .into_diagnostic()
        .wrap_err("requesting the registry collaborators endpoint")?;

    if response.status() == StatusCode::NOT_FOUND {
        return Err(AccessError::PackageNotFound { package_name: package_name.clone() }.into());
    }
    if !response.status().is_success() {
        return Err(fetch_error_from_response(response, "list collaborators for").await);
    }

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

    let entries: Vec<CollaboratorEntry> =
        response.json().await.into_diagnostic().wrap_err("parsing the collaborators response")?;

    if context.json {
        let output = serde_json::to_string_pretty(&entries)
            .into_diagnostic()
            .wrap_err("serializing collaborators to JSON")?;
        return Ok(output);
    }

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
    Ok(lines.join("\n"))
}

async fn get_status(context: &AccessContext<'_>, params: &[String]) -> miette::Result<String> {
    let package_name = params.first().ok_or(AccessError::GetStatusPackageRequired)?;

    let auth_header =
        context.config.auth_headers.for_url_with_package(&context.registry, Some(package_name));

    let url = format!(
        "{}-/package/{}/access",
        normalize_registry_url(&context.registry),
        escaped_package_name(package_name),
    );

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let mut builder = client.get(&url);
            if let Some(auth) = auth_header.as_deref() {
                builder = builder.header("authorization", auth);
            }
            builder
        })
        .await
        .into_diagnostic()
        .wrap_err("requesting the registry access status endpoint")?;

    if response.status() == StatusCode::NOT_FOUND {
        return Err(AccessError::PackageNotFound { package_name: package_name.clone() }.into());
    }
    if !response.status().is_success() {
        return Err(fetch_error_from_response(response, "get status of").await);
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    struct AccessStatus {
        access: Option<String>,
        #[serde(rename = "publish_requires_tfa")]
        publish_requires_tfa: Option<serde_json::Value>,
    }

    let status: AccessStatus =
        response.json().await.into_diagnostic().wrap_err("parsing the access status response")?;

    if context.json {
        let output = serde_json::to_string_pretty(&status)
            .into_diagnostic()
            .wrap_err("serializing access status to JSON")?;
        return Ok(output);
    }

    let access = status.access.as_deref().unwrap_or("public");
    Ok(format!("package: {package_name}\naccess: {access}"))
}

async fn set_status(context: &AccessContext<'_>, params: &[String]) -> miette::Result<String> {
    let status_val = params
        .first()
        .ok_or(AccessError::SetStatusRequired)?
        .strip_prefix("status=")
        .ok_or(AccessError::SetStatusRequired)?;

    let access_value = match status_val {
        "public" => "public",
        "private" | "restricted" => "restricted",
        other => return Err(AccessError::SetStatusInvalid { value: other.to_string() }.into()),
    };

    let package_name = params.get(1).ok_or(AccessError::SetStatusPackageRequired)?;

    if !package_name.starts_with('@') {
        return Err(AccessError::SetStatusUnscoped.into());
    }

    let auth_header =
        context.config.auth_headers.for_url_with_package(&context.registry, Some(package_name));

    let url = format!(
        "{}-/package/{}/access",
        normalize_registry_url(&context.registry),
        escaped_package_name(package_name),
    );

    let body = serde_json::json!({ "access": access_value });
    let body_bytes = serde_json::to_vec(&body).expect("a serializable object");

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let mut builder = client
                .post(&url)
                .header("content-type", "application/json")
                .body(body_bytes.clone());
            if let Some(auth) = auth_header.as_deref() {
                builder = builder.header("authorization", auth);
            }
            if let Some(otp) = &context.otp {
                builder = builder.header("npm-otp", otp);
            }
            builder
        })
        .await
        .into_diagnostic()
        .wrap_err("requesting the registry access set endpoint")?;

    if !response.status().is_success() {
        return Err(write_error_from_response(
            response,
            format!("set access to \"{access_value}\" for"),
        )
        .await);
    }

    let display_access = if access_value == "restricted" { "restricted" } else { "public" };
    Ok(format!("{package_name}: {display_access}"))
}

async fn set_mfa(context: &AccessContext<'_>, params: &[String]) -> miette::Result<String> {
    let mfa_val = params
        .first()
        .ok_or(AccessError::SetMfaRequired)?
        .strip_prefix("mfa=")
        .ok_or(AccessError::SetMfaRequired)?;

    let publish_requires_tfa = match mfa_val {
        "none" => false,
        "publish" | "automation" => true,
        other => return Err(AccessError::SetMfaInvalid { value: other.to_string() }.into()),
    };

    let package_name = params.get(1).ok_or(AccessError::SetMfaPackageRequired)?;

    let auth_header =
        context.config.auth_headers.for_url_with_package(&context.registry, Some(package_name));

    let url = format!(
        "{}-/package/{}/access",
        normalize_registry_url(&context.registry),
        escaped_package_name(package_name),
    );

    let body = serde_json::json!({ "publish_requires_tfa": publish_requires_tfa });
    let body_bytes = serde_json::to_vec(&body).expect("a serializable object");

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let mut builder = client
                .post(&url)
                .header("content-type", "application/json")
                .body(body_bytes.clone());
            if let Some(auth) = auth_header.as_deref() {
                builder = builder.header("authorization", auth);
            }
            if let Some(otp) = &context.otp {
                builder = builder.header("npm-otp", otp);
            }
            builder
        })
        .await
        .into_diagnostic()
        .wrap_err("requesting the registry MFA set endpoint")?;

    if !response.status().is_success() {
        return Err(write_error_from_response(response, "set MFA for".to_string()).await);
    }

    Ok(format!("{package_name}: mfa={mfa_val}"))
}

async fn grant_access(context: &AccessContext<'_>, params: &[String]) -> miette::Result<String> {
    if params.len() < 2 {
        return Err(AccessError::GrantArgsRequired.into());
    }

    let permissions = &params[0];
    if permissions != "read-only" && permissions != "read-write" {
        return Err(AccessError::GrantInvalidPermissions { value: permissions.clone() }.into());
    }

    let scope_team = &params[1];
    if !scope_team.contains(':') {
        return Err(AccessError::GrantInvalidTeam { team: scope_team.clone() }.into());
    }

    let package_name = params.get(2).ok_or(AccessError::GrantPackageRequired)?;

    let parts: Vec<&str> = scope_team.splitn(2, ':').collect();
    let scope = parts[0];
    let team = parts[1];

    let auth_header =
        context.config.auth_headers.for_url_with_package(&context.registry, Some(package_name));

    let url = format!(
        "{}-/team/{}/{}/package",
        normalize_registry_url(&context.registry),
        encode_uri_component(scope),
        encode_uri_component(team),
    );

    let body = serde_json::json!({
        "package": package_name,
        "permissions": permissions,
    });
    let body_bytes = serde_json::to_vec(&body).expect("a serializable object");

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let mut builder = client
                .put(&url)
                .header("content-type", "application/json")
                .body(body_bytes.clone());
            if let Some(auth) = auth_header.as_deref() {
                builder = builder.header("authorization", auth);
            }
            if let Some(otp) = &context.otp {
                builder = builder.header("npm-otp", otp);
            }
            builder
        })
        .await
        .into_diagnostic()
        .wrap_err("requesting the registry grant access endpoint")?;

    if !response.status().is_success() {
        return Err(write_error_from_response(
            response,
            format!("grant {permissions} access for {scope_team} on"),
        )
        .await);
    }

    Ok(format!("+{scope_team} ({permissions}): {package_name}"))
}

async fn revoke_access(context: &AccessContext<'_>, params: &[String]) -> miette::Result<String> {
    if params.is_empty() {
        return Err(AccessError::RevokeArgsRequired.into());
    }

    let scope_team = &params[0];
    if !scope_team.contains(':') {
        return Err(AccessError::RevokeInvalidTeam { team: scope_team.clone() }.into());
    }

    let package_name = params.get(1).ok_or(AccessError::RevokePackageRequired)?;

    let parts: Vec<&str> = scope_team.splitn(2, ':').collect();
    let scope = parts[0];
    let team = parts[1];

    let auth_header =
        context.config.auth_headers.for_url_with_package(&context.registry, Some(package_name));

    let url = format!(
        "{}-/team/{}/{}/package",
        normalize_registry_url(&context.registry),
        encode_uri_component(scope),
        encode_uri_component(team),
    );

    let body = serde_json::json!({ "package": package_name });
    let body_bytes = serde_json::to_vec(&body).expect("a serializable object");

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let mut builder = client
                .delete(&url)
                .header("content-type", "application/json")
                .body(body_bytes.clone());
            if let Some(auth) = auth_header.as_deref() {
                builder = builder.header("authorization", auth);
            }
            if let Some(otp) = &context.otp {
                builder = builder.header("npm-otp", otp);
            }
            builder
        })
        .await
        .into_diagnostic()
        .wrap_err("requesting the registry revoke access endpoint")?;

    if !response.status().is_success() {
        return Err(write_error_from_response(
            response,
            format!("revoke {scope_team}'s access to"),
        )
        .await);
    }

    Ok(format!("-{scope_team}: {package_name}"))
}

fn build_http_client(config: &Config) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &NetworkSettings {
            network_concurrency: config.network_concurrency,
            fetch_timeout: Duration::from_millis(config.fetch_timeout),
            user_agent: config.user_agent.clone(),
        },
    )
    .into_diagnostic()
    .wrap_err("create the network client for access command")
}

fn normalize_registry_url(registry_url: &str) -> String {
    if registry_url.ends_with('/') { registry_url.to_string() } else { format!("{registry_url}/") }
}

fn escaped_package_name(package_name: &str) -> String {
    match package_name.strip_prefix('@') {
        Some(rest) => format!("@{}", encode_uri_component(rest).replace("%2F", "%2f")),
        None => encode_uri_component(package_name),
    }
}

async fn fetch_error_from_response(response: Response, action: &str) -> miette::Report {
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();
    if status == StatusCode::NOT_FOUND {
        return AccessError::RegistryFetchFailed {
            action: action.to_string(),
            status: status.as_u16(),
            status_text,
        }
        .into();
    }
    AccessError::RegistryFetchFailed {
        action: action.to_string(),
        status: status.as_u16(),
        status_text,
    }
    .into()
}

async fn write_error_from_response(response: Response, action: String) -> miette::Report {
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();
    let body = read_error_body(response).await;

    match status {
        StatusCode::UNAUTHORIZED => AccessError::Unauthorized { action, body }.into(),
        StatusCode::FORBIDDEN => AccessError::Forbidden { action, body }.into(),
        StatusCode::NOT_FOUND => {
            AccessError::PackageNotFound { package_name: "unknown".to_string() }.into()
        }
        StatusCode::UNPROCESSABLE_ENTITY => AccessError::ValidationError { body }.into(),
        _ => {
            AccessError::RegistryWriteFailed { action, status: status.as_u16(), status_text, body }
                .into()
        }
    }
}

async fn read_error_body(response: Response) -> String {
    let limit = ACCESS_ERROR_BODY_LIMIT;
    let header_exceeds_limit =
        response.content_length().is_some_and(|length| length > limit as u64);
    let mut bytes = Vec::new();
    let mut truncated = header_exceeds_limit;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { break };
        let remaining = limit.saturating_sub(bytes.len());
        if chunk.len() > remaining {
            bytes.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    let mut body = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        if !body.is_empty() && !body.chars().next_back().is_some_and(char::is_whitespace) {
            body.push(' ');
        }
        body.push_str("(response body truncated)");
    }
    body
}

#[cfg(test)]
mod tests;
