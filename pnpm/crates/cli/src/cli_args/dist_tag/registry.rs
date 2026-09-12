use super::{
    AuthType, BTreeMap, Config, Context, DIST_TAG_ERROR_BODY_LIMIT, DIST_TAGS_BODY_LIMIT,
    Deserialize, DistTagContext, DistTagError, IntoDiagnostic, RequestBuilder, Response,
    StatusCode, ThrottledClient, encode_uri_component, parse_wanted_dependency,
    pick_registry_for_package, read_limited_body, redact_url_credentials, retry_async, sanitize,
    send_with_retry,
};

pub(super) struct SetDistTagRequest<'a> {
    pub(super) package_name: &'a str,
    pub(super) version: &'a str,
    pub(super) tag: &'a str,
    pub(super) registry_url: &'a str,
    pub(super) auth_header: Option<&'a str>,
    pub(super) auth_type: AuthType,
    pub(super) otp: Option<&'a str>,
}

pub(super) async fn set_dist_tag(
    context: &DistTagContext<'_>,
    request: SetDistTagRequest<'_>,
) -> miette::Result<()> {
    let url = dist_tag_url(request.package_name, request.registry_url, request.tag)?;
    let body = serde_json::to_string(request.version).expect("a string serializes");
    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let builder =
                client.put(&url).header("content-type", "application/json").body(body.clone());
            apply_dist_tag_mutation_headers(
                builder,
                request.auth_header,
                request.auth_type,
                request.otp,
            )
        })
        .await
        .map_err(|source| {
            registry_operation_error("requesting the registry dist-tag endpoint", source)
        })?;
    if response.status().is_success() {
        return Ok(());
    }
    let action = format!(r#"set dist-tag "{}" on"#, request.tag);
    write_error_from_response(response, action).await
}

pub(super) struct DeleteDistTagRequest<'a> {
    pub(super) package_name: &'a str,
    pub(super) tag: &'a str,
    pub(super) registry_url: &'a str,
    pub(super) auth_header: Option<&'a str>,
    pub(super) auth_type: AuthType,
    pub(super) otp: Option<&'a str>,
}

pub(super) async fn delete_dist_tag(
    context: &DistTagContext<'_>,
    request: DeleteDistTagRequest<'_>,
) -> miette::Result<()> {
    let url = dist_tag_url(request.package_name, request.registry_url, request.tag)?;
    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            apply_dist_tag_mutation_headers(
                client.delete(&url),
                request.auth_header,
                request.auth_type,
                request.otp,
            )
        })
        .await
        .map_err(|source| {
            registry_operation_error("requesting the registry dist-tag endpoint", source)
        })?;
    if response.status().is_success() {
        return Ok(());
    }
    let action = format!(r#"remove dist-tag "{}" from"#, request.tag);
    write_error_from_response(response, action).await
}

fn apply_dist_tag_mutation_headers(
    mut builder: RequestBuilder,
    auth_header: Option<&str>,
    auth_type: AuthType,
    otp: Option<&str>,
) -> RequestBuilder {
    builder = builder.header("npm-auth-type", auth_type.header_value());
    if let Some(auth_header) = auth_header {
        builder = builder.header("authorization", auth_header);
    }
    if let Some(otp) = otp {
        builder = builder.header("npm-otp", otp);
    }
    builder
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
            let mut builder = client.get(url);
            if let Some(auth_header) = auth_header {
                builder = builder.header("authorization", auth_header);
            }
            builder
        })
        .await
        .map_err(DistTagsFetchError::Request)?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(DistTagsFetchError::NotFound);
    }
    if !response.status().is_success() {
        return Err(DistTagsFetchError::Status { status: response.status() });
    }
    if response.content_length().is_some_and(|length| length > DIST_TAGS_BODY_LIMIT as u64) {
        return Err(DistTagsFetchError::BodyTooLarge);
    }
    let body = read_limited_body(response, DIST_TAGS_BODY_LIMIT)
        .await
        .map_err(DistTagsFetchError::Body)?;
    if body.truncated {
        return Err(DistTagsFetchError::BodyTooLarge);
    }
    serde_json::from_slice(&body.bytes).map_err(DistTagsFetchError::InvalidJson)
}

async fn write_error_from_response(response: Response, action: String) -> miette::Result<()> {
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();
    let body = read_limited_body(response, DIST_TAG_ERROR_BODY_LIMIT).await.map_err(|source| {
        registry_operation_error("reading the registry dist-tag error response", source)
    })?;
    let web_otp_challenge =
        if body.truncated { None } else { parse_web_otp_challenge(&body.bytes) };
    let body = sanitize::body_display_string(&body);
    if status == StatusCode::UNAUTHORIZED {
        if let Some(challenge) = web_otp_challenge {
            return Err(DistTagError::WebOtpRequired {
                action,
                auth_url: sanitize::sanitize(&challenge.auth_url).into_owned(),
                done_url: sanitize::sanitize(&challenge.done_url).into_owned(),
            }
            .into());
        }
        return Err(DistTagError::Unauthorized { action, body }.into());
    }
    if status == StatusCode::FORBIDDEN {
        return Err(DistTagError::Forbidden { action, body }.into());
    }
    Err(DistTagError::RegistryWriteFailed { action, status: status.as_u16(), status_text, body }
        .into())
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
            status_text: status.canonical_reason().unwrap_or_default().to_string(),
        }
        .into(),
    }
}

fn registry_operation_error<ErrorType>(operation: &'static str, error: ErrorType) -> miette::Report
where
    ErrorType: std::fmt::Display,
{
    DistTagError::RegistryOperationFailed {
        operation,
        reason: redact_url_credentials(&error.to_string()),
    }
    .into()
}

#[derive(Deserialize)]
struct WebOtpChallenge {
    #[serde(rename = "authUrl")]
    auth_url: String,
    #[serde(rename = "doneUrl")]
    done_url: String,
}

fn parse_web_otp_challenge(body: &[u8]) -> Option<WebOtpChallenge> {
    let challenge: WebOtpChallenge = serde_json::from_slice(body).ok()?;
    Some(WebOtpChallenge {
        auth_url: display_safe_web_otp_url(&challenge.auth_url)?,
        done_url: display_safe_web_otp_url(&challenge.done_url)?,
    })
}

fn display_safe_web_otp_url(value: &str) -> Option<String> {
    if value.chars().any(char::is_control) {
        return None;
    }
    let url = reqwest::Url::parse(value).ok()?;
    match url.scheme() {
        "http" | "https" => Some(redact_url_credentials(url.as_str())),
        _ => None,
    }
}

pub(super) fn registry_for_package(context: &DistTagContext<'_>, package_name: &str) -> String {
    pick_registry_for_package(&context.registries, package_name, None)
}

pub(super) fn auth_header_for_registry(
    context: &DistTagContext<'_>,
    registry_url: &str,
    package_name: &str,
) -> Option<String> {
    context.config.auth_headers.for_url_with_package(registry_url, Some(package_name))
}

pub(super) fn build_http_client(config: &Config) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &config.network_settings(),
    )
    .into_diagnostic()
    .wrap_err("create the network client for dist-tag")
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
    parse_wanted_dependency(package_name)
        .alias
        .ok_or_else(|| DistTagError::InvalidPackageSpec { spec: package_name.to_string() })
}

fn registry_endpoint_url(registry_url: &str, path: &str) -> miette::Result<String> {
    reqwest::Url::parse(&normalize_registry_url(registry_url))
        .and_then(|url| url.join(path))
        .map(|url| url.to_string())
        .map_err(|source| registry_operation_error("build registry dist-tag URL", source))
}

pub(super) fn normalize_registry_url(registry_url: &str) -> String {
    if registry_url.ends_with('/') { registry_url.to_string() } else { format!("{registry_url}/") }
}

fn escaped_package_name(package_name: &str) -> String {
    match package_name.strip_prefix('@') {
        Some(rest) => format!("@{}", encode_uri_component(rest).replace("%2F", "%2f")),
        None => encode_uri_component(package_name),
    }
}
