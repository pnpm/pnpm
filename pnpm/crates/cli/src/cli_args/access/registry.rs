use super::{AccessArgs, AccessError, Config};
use crate::cli_args::{
    registry_client::{
        apply_auth_and_otp, auth_header_for_package, build_registry_client_with_otp_guard,
        join_registry_endpoint, resolve_registries_with_override,
        resolve_target_registry_for_package,
    },
    sanitize::{DEFAULT_ERROR_BODY_LIMIT, read_sanitized_error_body},
};
use pnpm_network::{
    RetryOpts, ThrottledClient, ThrottledClientGuard, encode_uri_component, escaped_package_name,
    send_with_retry,
};
use reqwest::{Method, Response, StatusCode};
use std::collections::HashMap;

pub(super) struct AccessContext<'a> {
    pub(super) config: &'a Config,
    pub(super) http_client: ThrottledClient,
    pub(super) retry_opts: RetryOpts,
    pub(super) registries: HashMap<String, String>,
    pub(super) registry_override: Option<&'a str>,
    pub(super) json: bool,
    pub(super) otp: Option<String>,
}

pub(super) fn build_access_context<'a>(
    args: &'a AccessArgs,
    config: &'a Config,
) -> miette::Result<AccessContext<'a>> {
    let registries = resolve_registries_with_override(config, args.registry.as_deref());
    let http_client = build_registry_client_with_otp_guard(
        config,
        args.otp.as_deref(),
        registries.values().map(String::as_str),
    )?;

    Ok(AccessContext {
        config,
        http_client,
        retry_opts: config.retry_opts(),
        registries,
        registry_override: args.registry.as_deref(),
        json: args.json,
        otp: args.otp.clone(),
    })
}

pub(super) fn registry_for_package(context: &AccessContext<'_>, package_name: &str) -> String {
    resolve_target_registry_for_package(
        &context.registries,
        context.registry_override,
        package_name,
        None,
    )
}

pub(super) fn registry_for_scope(context: &AccessContext<'_>, scope: &str) -> String {
    let pkg_name = format!("@{scope}/_");
    resolve_target_registry_for_package(
        &context.registries,
        context.registry_override,
        &pkg_name,
        None,
    )
}

pub(super) fn registry_for_list(context: &AccessContext<'_>, params: &[String]) -> String {
    match params.first() {
        Some(raw) => {
            let entity = raw
                .split(':')
                .next()
                .unwrap_or(raw.as_str());
            if let Some(scope) = entity.strip_prefix('@') {
                registry_for_scope(context, scope)
            } else if raw.contains(':') {
                registry_for_scope(context, entity)
            } else {
                registry_for_package(context, "")
            }
        }
        None => registry_for_package(context, ""),
    }
}

pub(super) fn auth_header_for_list(
    context: &AccessContext<'_>,
    params: &[String],
    registry: &str,
) -> Option<String> {
    let scope = params
        .first()
        .and_then(|raw| {
            let entity = raw
                .split(':')
                .next()
                .unwrap_or(raw.as_str());
            if let Some(scope) = entity.strip_prefix('@') {
                Some(scope)
            } else if raw.contains(':') {
                Some(entity)
            } else {
                None
            }
        });
    match scope {
        Some(scope_name) => {
            let pkg_name = format!("@{scope_name}/_");
            auth_header_for_package(context.config, registry, &pkg_name)
        }
        None => context.config.auth_headers.for_url(registry),
    }
}

/// GET `url`, carrying the registry's authorization header when there is
/// one.
pub(super) async fn send_get<'client>(
    context: &'client AccessContext<'_>,
    url: &str,
    auth_header: Option<&str>,
) -> Result<(ThrottledClientGuard<'client>, Response), reqwest::Error> {
    send_with_retry(&context.http_client, url, context.retry_opts, |client| {
        apply_auth_and_otp(client.get(url), auth_header, None)
    })
    .await
}

/// Send `body` as JSON, carrying the registry's authorization header and
/// the one-time password when the command was given one.
pub(super) async fn send_json<'client>(
    context: &'client AccessContext<'_>,
    method: Method,
    url: &str,
    auth_header: Option<&str>,
    body: &serde_json::Value,
) -> Result<(ThrottledClientGuard<'client>, Response), reqwest::Error> {
    let body_bytes = serde_json::to_vec(body).expect("a serializable object");
    send_with_retry(&context.http_client, url, context.retry_opts, |client| {
        let builder = client
            .request(method.clone(), url)
            .header("content-type", "application/json")
            .body(body_bytes.clone());
        apply_auth_and_otp(builder, auth_header, context.otp.as_deref())
    })
    .await
}

pub(super) async fn fetch_error_from_response(response: Response, action: &str) -> miette::Report {
    let status = response.status();
    AccessError::RegistryFetchFailed {
        action: action.to_string(),
        status: status.as_u16(),
        status_text: status
            .canonical_reason()
            .unwrap_or_default()
            .to_string(),
    }
    .into()
}

pub(super) async fn write_error_from_response(
    response: Response,
    action: String,
    package_name: &str,
) -> miette::Report {
    let (status, status_text, body) =
        read_sanitized_error_body(response, DEFAULT_ERROR_BODY_LIMIT).await;

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

pub(super) fn package_access_url(registry: &str, package_name: &str) -> String {
    let path = format!("-/package/{}/access", escaped_package_name(package_name));
    join_registry_endpoint(registry, &path).unwrap_or_else(|_| format!("{registry}{path}"))
}

pub(super) fn team_package_url(registry: &str, scope: &str, team: &str) -> String {
    let path =
        format!("-/team/{}/{}/package", encode_uri_component(scope), encode_uri_component(team));
    join_registry_endpoint(registry, &path).unwrap_or_else(|_| format!("{registry}{path}"))
}

pub(super) fn package_collaborators_url(
    registry: &str,
    package_name: &str,
    user: Option<&str>,
) -> String {
    let base_path =
        format!("-/package/{}/collaborators?format=cli", escaped_package_name(package_name));
    let path = match user {
        Some(user_name) => format!("{base_path}&user={}", encode_uri_component(user_name)),
        None => base_path,
    };
    join_registry_endpoint(registry, &path).unwrap_or_else(|_| format!("{registry}{path}"))
}

pub(crate) fn list_packages_url(registry: &str, params: &[String]) -> String {
    let path = match params.first() {
        None => "-/-/package?format=cli".to_string(),
        Some(raw) if !raw.contains(':') => match raw.strip_prefix('@') {
            Some(org_name) => {
                format!("-/org/{}/package?format=cli", encode_uri_component(org_name))
            }
            None => format!("-/user/{}/package?format=cli", encode_uri_component(raw)),
        },
        Some(raw) => {
            let parts: Vec<&str> = raw.splitn(2, ':').collect();
            let team = parts.get(1).unwrap_or(&"");
            let team_path = if team.is_empty() {
                String::new()
            } else {
                format!("{}/", encode_uri_component(team))
            };
            let scope = parts[0]
                .strip_prefix('@')
                .unwrap_or(parts[0]);
            format!("-/team/{}/{team_path}package?format=cli", encode_uri_component(scope))
        }
    };
    join_registry_endpoint(registry, &path).unwrap_or_else(|_| format!("{registry}{path}"))
}
