use clap::Args;
use derive_more::{Display, Error};
use futures_util::StreamExt as _;
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_network::{
    RedirectGuard, RetryOpts, ThrottledClient, ThrottledClientGuard, encode_uri_component,
    redact_and_sanitize, send_with_retry,
};
use reqwest::{Method, Response, StatusCode};
use std::{collections::HashMap, sync::Arc, time::Duration};

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
    #[display(r#"A subcommand is required (e.g., "list packages", "get status", "set status=public", "grant", "revoke")"#)]
    #[diagnostic(code(ERR_PNPM_ACCESS_SUBCOMMAND_REQUIRED))]
    SubcommandRequired,

    #[display(r#"Unknown subcommand: {cmd}. Run "pnpm help access" for available subcommands."#)]
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
        ("public", _) => {
            ("set_status", access_args(Some("status=public".to_string()), second, params))
        }
        ("restricted", _) => {
            ("set_status", access_args(Some("status=restricted".to_string()), second, params))
        }
        ("grant", _) => ("grant", access_args(None, second, params)),
        ("revoke", _) => ("revoke", access_args(None, second, params)),
        _ => {
            let parts = access_args(Some(first.to_owned()), second, params);
            return Err(AccessError::UnknownSubcommand { cmd: parts.join(" ") });
        }
    };
    Ok(action)
}

fn access_args(lead: Option<String>, second: Option<String>, params: Vec<String>) -> Vec<String> {
    lead.into_iter().chain(second).chain(params).collect()
}

fn build_access_context<'a>(
    args: &AccessArgs,
    config: &'a Config,
) -> miette::Result<AccessContext<'a>> {
    let registry =
        args.registry.as_deref().map_or_else(|| config.registry.clone(), normalize_registry_url);

    let redirect_guard = args.otp.as_ref().map(|_| {
        let registry_origin: Option<(String, String, Option<u16>)> =
            reqwest::Url::parse(&registry).ok().and_then(|url| {
                url.host_str().map(|host| (url.scheme().to_string(), host.to_string(), url.port()))
            });
        let guard: RedirectGuard = Arc::new(move |target: &reqwest::Url| -> bool {
            registry_origin.as_ref().is_some_and(|(scheme, host, port)| {
                target.scheme() == scheme
                    && target.host_str() == Some(host.as_str())
                    && target.port() == *port
            })
        });
        guard
    });

    Ok(AccessContext {
        config,
        http_client: build_http_client(config, redirect_guard.as_ref())?,
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

/// GET `url`, carrying the registry's authorization header when there is
/// one.
async fn send_get<'client>(
    context: &'client AccessContext<'_>,
    url: &str,
    auth_header: Option<&str>,
) -> Result<(ThrottledClientGuard<'client>, Response), reqwest::Error> {
    send_with_retry(&context.http_client, url, context.retry_opts, |client| {
        let mut builder = client.get(url);
        if let Some(auth) = auth_header {
            builder = builder.header("authorization", auth);
        }
        builder
    })
    .await
}

/// Send `body` as JSON, carrying the registry's authorization header and
/// the one-time password when the command was given one.
async fn send_json<'client>(
    context: &'client AccessContext<'_>,
    method: Method,
    url: &str,
    auth_header: Option<&str>,
    body: &serde_json::Value,
) -> Result<(ThrottledClientGuard<'client>, Response), reqwest::Error> {
    let body_bytes = serde_json::to_vec(body).expect("a serializable object");
    send_with_retry(&context.http_client, url, context.retry_opts, |client| {
        let mut builder = client
            .request(method.clone(), url)
            .header("content-type", "application/json")
            .body(body_bytes.clone());
        if let Some(auth) = auth_header {
            builder = builder.header("authorization", auth);
        }
        if let Some(otp) = &context.otp {
            builder = builder.header("npm-otp", otp);
        }
        builder
    })
    .await
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
    if let Some(org_name) = raw.strip_prefix('@') {
        return format!(
            "{}-/org/{}/package?format=cli",
            normalize_registry_url(registry),
            encode_uri_component(org_name),
        );
    }
    if !raw.contains(':') {
        return format!(
            "{}-/user/{}/package?format=cli",
            normalize_registry_url(registry),
            encode_uri_component(raw),
        );
    }
    let parts: Vec<&str> = raw.splitn(2, ':').collect();
    let team = parts.get(1).unwrap_or(&"");
    let team_path =
        if team.is_empty() { String::new() } else { format!("{}/", encode_uri_component(team)) };
    format!(
        "{}-/team/{}/{}package?format=cli",
        normalize_registry_url(registry),
        encode_uri_component(parts[0].strip_prefix('@').unwrap_or(parts[0])),
        team_path,
    )
}

async fn fetch_list_response(
    context: &AccessContext<'_>,
    url: &str,
    auth_header: Option<&str>,
) -> miette::Result<String> {
    let (_guard, response) = send_get(context, url, auth_header)
        .await
        .map_err(reqwest::Error::without_url)
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

    let entries: Vec<CollaboratorEntry> =
        response.json().await.into_diagnostic().wrap_err("parsing the collaborators response")?;

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

async fn get_status(context: &AccessContext<'_>, params: &[String]) -> miette::Result<String> {
    let package_name = params.first().ok_or(AccessError::GetStatusPackageRequired)?;

    let auth_header =
        context.config.auth_headers.for_url_with_package(&context.registry, Some(package_name));

    let url = format!(
        "{}-/package/{}/access",
        normalize_registry_url(&context.registry),
        escaped_package_name(package_name),
    );

    let (_guard, response) = send_get(context, &url, auth_header.as_deref())
        .await
        .map_err(reqwest::Error::without_url)
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

    let (_guard, response) = send_json(context, Method::POST, &url, auth_header.as_deref(), &body)
        .await
        .map_err(reqwest::Error::without_url)
        .into_diagnostic()
        .wrap_err("requesting the registry access set endpoint")?;

    if !response.status().is_success() {
        return Err(write_error_from_response(
            response,
            format!(r#"set access to "{access_value}" for"#),
            package_name,
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

    let (_guard, response) = send_json(context, Method::POST, &url, auth_header.as_deref(), &body)
        .await
        .map_err(reqwest::Error::without_url)
        .into_diagnostic()
        .wrap_err("requesting the registry MFA set endpoint")?;

    if !response.status().is_success() {
        return Err(
            write_error_from_response(response, "set MFA for".to_string(), package_name).await
        );
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
    let scope = parts[0].strip_prefix('@').unwrap_or(parts[0]);
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

    let (_guard, response) = send_json(context, Method::PUT, &url, auth_header.as_deref(), &body)
        .await
        .map_err(reqwest::Error::without_url)
        .into_diagnostic()
        .wrap_err("requesting the registry grant access endpoint")?;

    if !response.status().is_success() {
        return Err(write_error_from_response(
            response,
            format!("grant {permissions} access for {scope_team} on"),
            package_name,
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
    let scope = parts[0].strip_prefix('@').unwrap_or(parts[0]);
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

    let (_guard, response) =
        send_json(context, Method::DELETE, &url, auth_header.as_deref(), &body)
            .await
            .map_err(reqwest::Error::without_url)
            .into_diagnostic()
            .wrap_err("requesting the registry revoke access endpoint")?;

    if !response.status().is_success() {
        return Err(write_error_from_response(
            response,
            format!("revoke {scope_team}'s access to"),
            package_name,
        )
        .await);
    }

    Ok(format!("-{scope_team}: {package_name}"))
}

fn build_http_client(
    config: &Config,
    redirect_guard: Option<&RedirectGuard>,
) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs_with_guard(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &config.network_settings(),
        redirect_guard,
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
    AccessError::RegistryFetchFailed {
        action: action.to_string(),
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or_default().to_string(),
    }
    .into()
}

async fn write_error_from_response(
    response: Response,
    action: String,
    package_name: &str,
) -> miette::Report {
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();
    let body = redact_and_sanitize(&read_error_body(response).await);

    match status {
        StatusCode::UNAUTHORIZED => AccessError::Unauthorized { action, body }.into(),
        StatusCode::FORBIDDEN => AccessError::Forbidden { action, body }.into(),
        StatusCode::NOT_FOUND => {
            AccessError::PackageNotFound { package_name: package_name.to_string() }.into()
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
