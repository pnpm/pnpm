use super::{
    AuthType, BTreeMap, DIST_TAG_ERROR_BODY_LIMIT, DIST_TAGS_BODY_LIMIT, DistTagContext,
    DistTagError, RequestBuilder, Response, StatusCode, encode_uri_component, read_limited_body,
    redact_url_credentials, retry_async, sanitize, send_with_retry,
};
use crate::cli_args::{
    package_spec::PackageSpec,
    registry_client::{
        OtpHost, apply_auth_and_otp, auth_header_for_package, join_registry_endpoint,
        resolve_target_registry_for_package, web_auth_fetch_options,
    },
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_network::escaped_package_name;
use pnpm_network_web_auth::{
    OtpChallenge, OtpError, WithOtpError, otp_challenge_from_unauthorized_body, with_otp_handling,
};
use pnpm_publish::{Host, OidcHttpOptions, fetch_token_and_provenance_by_oidc};
use pnpm_reporter::Reporter;
use reqwest::Method;

pub(super) struct SetDistTagRequest<'a> {
    pub(super) package_name: &'a str,
    pub(super) version: &'a str,
    pub(super) tag: &'a str,
    pub(super) registry_url: &'a str,
    pub(super) auth_header: Option<&'a str>,
}

pub(super) async fn set_dist_tag<Sys: OtpHost, Reporter: self::Reporter>(
    context: &DistTagContext<'_>,
    request: SetDistTagRequest<'_>,
) -> miette::Result<()> {
    let url = dist_tag_url(request.package_name, request.registry_url, request.tag)?;
    let body = serde_json::to_string(request.version).expect("a string serializes");
    let action = format!(r#"set dist-tag "{}" on"#, request.tag);
    send_dist_tag_mutation::<Sys, Reporter>(
        context,
        DistTagMutation {
            method: &Method::PUT,
            url: &url,
            json_body: Some(&body),
            auth_header: request.auth_header,
            action: &action,
        },
    )
    .await
}

pub(super) struct DeleteDistTagRequest<'a> {
    pub(super) package_name: &'a str,
    pub(super) tag: &'a str,
    pub(super) registry_url: &'a str,
    pub(super) auth_header: Option<&'a str>,
}

pub(super) async fn delete_dist_tag<Sys: OtpHost, Reporter: self::Reporter>(
    context: &DistTagContext<'_>,
    request: DeleteDistTagRequest<'_>,
) -> miette::Result<()> {
    let url = dist_tag_url(request.package_name, request.registry_url, request.tag)?;
    let action = format!(r#"remove dist-tag "{}" from"#, request.tag);
    send_dist_tag_mutation::<Sys, Reporter>(
        context,
        DistTagMutation {
            method: &Method::DELETE,
            url: &url,
            json_body: None,
            auth_header: request.auth_header,
            action: &action,
        },
    )
    .await
}

#[derive(Clone, Copy)]
struct DistTagMutation<'a> {
    method: &'a Method,
    url: &'a str,
    json_body: Option<&'a str>,
    auth_header: Option<&'a str>,
    /// Completes "Failed to {action} package" in error messages.
    action: &'a str,
}

/// A failed dist-tag write, handed to the OTP session. Only the
/// [`Otp`](Self::Otp) arm is a challenge it acts on; the rest propagate.
#[derive(Debug, Display, Error, Diagnostic)]
enum DistTagHttpError {
    #[display("the registry requested a one-time password")]
    Otp {
        #[error(not(source))]
        challenge: OtpChallenge,
    },

    #[diagnostic(transparent)]
    Registry(#[error(not(source))] DistTagError),
}

impl OtpError for DistTagHttpError {
    fn as_otp_challenge(&self) -> Option<OtpChallenge> {
        match self {
            DistTagHttpError::Otp { challenge } => Some(challenge.clone()),
            DistTagHttpError::Registry(_) => None,
        }
    }
}

/// Send one dist-tag write. A `--otp` code goes out with the first attempt.
/// An OTP challenge runs the browser or prompt flow, and the write is retried
/// with the password it yields.
async fn send_dist_tag_mutation<Sys: OtpHost, Reporter: self::Reporter>(
    context: &DistTagContext<'_>,
    mutation: DistTagMutation<'_>,
) -> miette::Result<()> {
    let auth_type = if context.otp.is_some() { AuthType::Legacy } else { AuthType::Web };
    with_otp_handling::<Sys, Reporter, (), DistTagHttpError, _, _>(
        web_auth_fetch_options(context.config),
        // A plain `FnMut` returning an `async move` block keeps the future
        // `Send`. See `with_otp_handling`'s `Operation` bound.
        move |challenge_otp: Option<String>| {
            let otp = challenge_otp.or_else(|| context.otp.clone());
            async move { send_dist_tag_mutation_once(context, mutation, auth_type, otp).await }
        },
    )
    .await
    .map_err(|error| match error {
        WithOtpError::Operation(DistTagHttpError::Registry(registry_error)) => {
            miette::Report::new(registry_error)
        }
        other => miette::Report::new(other),
    })
}

async fn send_dist_tag_mutation_once(
    context: &DistTagContext<'_>,
    mutation: DistTagMutation<'_>,
    auth_type: AuthType,
    otp: Option<String>,
) -> Result<(), DistTagHttpError> {
    let (_guard, response) =
        send_with_retry(&context.http_client, mutation.url, context.retry_opts, |client| {
            let mut builder = client.request(mutation.method.clone(), mutation.url);
            if let Some(json_body) = mutation.json_body {
                builder =
                    builder.header("content-type", "application/json").body(json_body.to_owned());
            }
            apply_dist_tag_mutation_headers(
                builder,
                mutation.auth_header,
                auth_type,
                otp.as_deref(),
            )
        })
        .await
        .map_err(|source| {
            DistTagHttpError::Registry(registry_operation_failure(
                "requesting the registry dist-tag endpoint",
                source,
            ))
        })?;
    if response.status().is_success() {
        return Ok(());
    }
    Err(write_error_from_response(response, mutation.action.to_string()).await)
}

fn apply_dist_tag_mutation_headers(
    builder: RequestBuilder,
    auth_header: Option<&str>,
    auth_type: AuthType,
    otp: Option<&str>,
) -> RequestBuilder {
    let builder =
        builder.header("npm-auth-type", auth_type.header_value()).header("npm-command", "dist-tag");
    apply_auth_and_otp(builder, auth_header, otp)
}

pub(super) async fn fetch_dist_tags(
    context: &DistTagContext<'_>,
    package_name: &str,
    registry_url: &str,
    auth_header: Option<&str>,
) -> miette::Result<BTreeMap<String, String>> {
    let url = dist_tags_url(package_name, registry_url)?;
    retry_async(&url, context.retry_opts, DistTagsFetchError::is_retryable, || async {
        fetch_dist_tags_once(context, &url, auth_header).await
    })
    .await
    .map_err(|error| map_dist_tags_fetch_error(error, package_name))
}

async fn fetch_dist_tags_once(
    context: &DistTagContext<'_>,
    url: &str,
    auth_header: Option<&str>,
) -> Result<BTreeMap<String, String>, DistTagsFetchError> {
    let (_guard, response) =
        send_with_retry(&context.http_client, url, context.retry_opts, |client| {
            apply_auth_and_otp(client.get(url), auth_header, None)
        })
        .await
        .map_err(DistTagsFetchError::Request)?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(DistTagsFetchError::NotFound);
    }
    if !response.status().is_success() {
        return Err(DistTagsFetchError::Status { status: response.status() });
    }
    if response
        .content_length()
        .is_some_and(|length| length > DIST_TAGS_BODY_LIMIT as u64)
    {
        return Err(DistTagsFetchError::BodyTooLarge);
    }
    let body =
        read_limited_body(response, DIST_TAGS_BODY_LIMIT).await.map_err(DistTagsFetchError::Body)?;
    if body.truncated {
        return Err(DistTagsFetchError::BodyTooLarge);
    }
    serde_json::from_slice(&body.bytes).map_err(DistTagsFetchError::InvalidJson)
}

async fn write_error_from_response(response: Response, action: String) -> DistTagHttpError {
    let status = response.status();
    let status_text = status
        .canonical_reason()
        .unwrap_or_default()
        .to_string();
    let body = match read_limited_body(response, DIST_TAG_ERROR_BODY_LIMIT).await {
        Ok(body) => body,
        Err(source) => {
            return DistTagHttpError::Registry(registry_operation_failure(
                "reading the registry dist-tag error response",
                source,
            ));
        }
    };
    if status == StatusCode::UNAUTHORIZED
        && !body.truncated
        && let Some(challenge) = otp_challenge_from_unauthorized_body(&body.bytes)
    {
        return DistTagHttpError::Otp { challenge };
    }
    let body = sanitize::body_display_string(&body);
    DistTagHttpError::Registry(match status {
        StatusCode::UNAUTHORIZED => DistTagError::Unauthorized { action, body },
        StatusCode::FORBIDDEN => DistTagError::Forbidden { action, body },
        _ => {
            DistTagError::RegistryWriteFailed { action, status: status.as_u16(), status_text, body }
        }
    })
}

#[derive(Debug)]
enum DistTagsFetchError {
    Request(reqwest::Error),
    Body(reqwest::Error),
    InvalidJson(serde_json::Error),
    BodyTooLarge,
    NotFound,
    Status { status: StatusCode },
}

impl DistTagsFetchError {
    fn is_retryable(&self) -> bool {
        matches!(self, Self::Body(_) | Self::InvalidJson(_))
    }
}

fn map_dist_tags_fetch_error(error: DistTagsFetchError, package_name: &str) -> miette::Report {
    match error {
        DistTagsFetchError::Request(error) => {
            registry_operation_error("requesting the registry dist-tags endpoint", error)
        }
        DistTagsFetchError::Body(error) => {
            registry_operation_error("reading the dist-tags response", error)
        }
        DistTagsFetchError::InvalidJson(error) => {
            registry_operation_error("parsing the dist-tags response", error)
        }
        DistTagsFetchError::BodyTooLarge => DistTagError::RegistryResponseTooLarge {
            resource: "dist-tags",
            limit: DIST_TAGS_BODY_LIMIT,
        }
        .into(),
        DistTagsFetchError::NotFound => {
            DistTagError::PackageNotFound { package_name: package_name.to_string() }.into()
        }
        DistTagsFetchError::Status { status } => DistTagError::RegistryFetchFailed {
            status: status.as_u16(),
            status_text: status
                .canonical_reason()
                .unwrap_or_default()
                .to_string(),
        }
        .into(),
    }
}

fn registry_operation_error<ErrorType>(operation: &'static str, error: ErrorType) -> miette::Report
where
    ErrorType: std::fmt::Display,
{
    registry_operation_failure(operation, error).into()
}

fn registry_operation_failure<ErrorType>(operation: &'static str, error: ErrorType) -> DistTagError
where
    ErrorType: std::fmt::Display,
{
    DistTagError::RegistryOperationFailed {
        operation,
        reason: redact_url_credentials(&error.to_string()),
    }
}

pub(super) fn registry_for_package(context: &DistTagContext<'_>, package_name: &str) -> String {
    resolve_target_registry_for_package(
        &context.registries,
        context.registry_override,
        package_name,
        None,
    )
}

pub(super) async fn auth_header_for_registry<Reporter: self::Reporter>(
    context: &DistTagContext<'_>,
    registry_url: &str,
    package_name: &str,
) -> miette::Result<Option<String>> {
    let config = context.config;
    let http = OidcHttpOptions {
        fetch_retries: Some(config.fetch_retries),
        fetch_retry_factor: Some(f64::from(config.fetch_retry_factor)),
        fetch_retry_maxtimeout: Some(config.fetch_retry_maxtimeout),
        fetch_retry_mintimeout: Some(config.fetch_retry_mintimeout),
        fetch_timeout: Some(config.fetch_timeout),
    };
    // Dist-tag requests need only the token, so skip the publish provenance probe.
    let oidc = fetch_token_and_provenance_by_oidc::<Host, Reporter>(
        package_name,
        registry_url,
        Some(false),
        &http,
    )
    .await?;
    Ok(oidc
        .map(|token| format!("Bearer {}", token.auth_token))
        .or_else(|| auth_header_for_package(config, registry_url, package_name)))
}

fn dist_tags_url(package_name: &str, registry_url: &str) -> miette::Result<String> {
    let package_name = package_name_for_url(package_name)?;
    registry_endpoint_url(
        registry_url,
        &format!("-/package/{}/dist-tags", escaped_package_name(&package_name)),
    )
}

fn dist_tag_url(package_name: &str, registry_url: &str, tag: &str) -> miette::Result<String> {
    let package_name = package_name_for_url(package_name)?;
    registry_endpoint_url(
        registry_url,
        &format!(
            "-/package/{}/dist-tags/{}",
            escaped_package_name(&package_name),
            encode_uri_component(tag),
        ),
    )
}

pub(super) fn package_name_for_url(package_name: &str) -> Result<String, DistTagError> {
    PackageSpec::parse(package_name)
        .map(|spec| spec.name)
        .ok_or_else(|| DistTagError::InvalidPackageSpec { spec: package_name.to_string() })
}

fn registry_endpoint_url(registry_url: &str, path: &str) -> miette::Result<String> {
    join_registry_endpoint(registry_url, path)
        .map_err(|source| registry_operation_error("build registry dist-tag URL", source))
}
