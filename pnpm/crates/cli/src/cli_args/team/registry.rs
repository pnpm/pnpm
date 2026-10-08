use super::{
    Deserialize, IntoDiagnostic, Response, TeamContext, TeamError, encode_uri_component,
    pick_registry_for_package, redact_url_credentials, send_with_retry,
};
use crate::cli_args::sanitize::{DEFAULT_ERROR_BODY_LIMIT, read_sanitized_error_body};
use pnpm_network::{normalize_registry_url, read_limited_body};

const TEAM_BODY_LIMIT: usize = 1024 * 1024;

pub(super) fn team_url(registry_url: &str, scope: &str, team: &str) -> String {
    format!(
        "{}-/team/{}/{}",
        normalize_registry_url(registry_url),
        encode_uri_component(scope),
        encode_uri_component(team),
    )
}

pub(super) fn team_user_url(registry_url: &str, scope: &str, team: &str) -> String {
    format!(
        "{}-/team/{}/{}/user",
        normalize_registry_url(registry_url),
        encode_uri_component(scope),
        encode_uri_component(team),
    )
}

pub(super) fn org_team_url(registry_url: &str, scope: &str) -> String {
    format!("{}-/org/{}/team", normalize_registry_url(registry_url), encode_uri_component(scope))
}

#[derive(Deserialize)]
pub(super) struct TeamInfo {
    pub(super) name: String,
}

#[derive(Deserialize)]
pub(super) struct UserInfo {
    pub(super) name: String,
}

pub(super) async fn fetch_teams(
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

    let body = read_limited_body(response, TEAM_BODY_LIMIT).await
        .map_err(|source| registry_operation_error("reading teams response", source))?;
    serde_json::from_slice(&body.bytes)
        .into_diagnostic()
        .map_err(|source| registry_operation_error("parsing teams response", source))
}

pub(super) async fn fetch_team_members(
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

    let body = read_limited_body(response, TEAM_BODY_LIMIT).await
        .map_err(|source| registry_operation_error("reading team members response", source))?;
    serde_json::from_slice(&body.bytes)
        .into_diagnostic()
        .map_err(|source| registry_operation_error("parsing team members response", source))
}

pub(super) fn registry_for_scope(context: &TeamContext<'_>, scope: &str) -> String {
    let pkg_name = format!("@{scope}/_");
    pick_registry_for_package(&context.registries, &pkg_name, None)
}

pub(super) fn auth_header_for_registry(
    context: &TeamContext<'_>,
    scope: &str,
) -> miette::Result<String> {
    let registry_url = registry_for_scope(context, scope);
    let pkg_name = format!("@{scope}/_");
    context.config.auth_headers
        .for_url_with_package(&registry_url, Some(&pkg_name))
        .ok_or_else(|| TeamError::MissingAuthToken.into())
}

pub(super) fn apply_auth_and_otp(
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

pub(super) fn registry_operation_error<ErrorType>(
    operation: &'static str,
    error: ErrorType,
) -> miette::Report
where
    ErrorType: std::fmt::Display,
{
    TeamError::RegistryOperationFailed {
        operation,
        reason: redact_url_credentials(&error.to_string()),
    }
    .into()
}

pub(super) async fn registry_error_from_response(
    response: Response,
    action: String,
) -> miette::Report {
    let (status, status_text, body) =
        read_sanitized_error_body(response, DEFAULT_ERROR_BODY_LIMIT).await;

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
