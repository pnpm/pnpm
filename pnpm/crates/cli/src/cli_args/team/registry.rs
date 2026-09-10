use super::{
    Config, Deserialize, IntoDiagnostic, RedirectGuard, Response, TeamContext, TeamError,
    ThrottledClient, encode_uri_component, pick_registry_for_package, redact_url_credentials,
    sanitize, send_with_retry,
};
use futures_util::StreamExt as _;
use miette::WrapErr;

const TEAM_BODY_LIMIT: usize = 1024 * 1024;

const TEAM_ERROR_BODY_LIMIT: usize = 64 * 1024;

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

    let body = read_limited_body(response, TEAM_BODY_LIMIT)
        .await
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

    let body = read_limited_body(response, TEAM_BODY_LIMIT)
        .await
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
    context
        .config
        .auth_headers
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

pub(super) fn build_http_client(
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
    .wrap_err("create the network client for team command")
}

pub(super) fn normalize_registry_url(registry_url: &str) -> String {
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
