use super::{
    BTreeMap, Config, Context, DEPRECATION_BODY_LIMIT, DEPRECATION_ERROR_BODY_LIMIT,
    DeprecateContext, DeprecateError, Deserialize, IntoDiagnostic, LimitedBody, Response,
    Serialize, StatusCode, ThrottledClient, encode_uri_component, parse_wanted_dependency,
    pick_registry_for_package, read_limited_body, redact_url_credentials, retry_async, sanitize,
    send_with_retry,
};

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct PackageMeta {
    #[serde(default)]
    pub(super) versions: BTreeMap<String, VersionInfo>,
    #[serde(flatten)]
    pub(super) other: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct VersionInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) deprecated: Option<String>,
    #[serde(flatten)]
    other: serde_json::Value,
}

pub(crate) async fn fetch_package_meta<Meta: serde::de::DeserializeOwned>(
    context: &DeprecateContext<'_>,
    url: &str,
    auth_header: Option<&str>,
    package_name: &str,
) -> miette::Result<Meta> {
    retry_async(url, context.retry_opts, FetchError::is_retryable, || async {
        fetch_package_meta_once(context, url, auth_header).await
    })
    .await
    .map_err(|error| map_fetch_error(error, package_name))
}

#[derive(Debug)]
enum FetchError {
    Request(reqwest::Error),
    Body(reqwest::Error),
    InvalidJson(serde_json::Error),
    BodyTooLarge,
    NotFound,
    Status { status: StatusCode },
}

impl FetchError {
    fn is_retryable(&self) -> bool {
        matches!(self, Self::Body(_) | Self::InvalidJson(_))
    }
}

async fn fetch_package_meta_once<Meta: serde::de::DeserializeOwned>(
    context: &DeprecateContext<'_>,
    url: &str,
    auth_header: Option<&str>,
) -> Result<Meta, FetchError> {
    let (_guard, response) =
        send_with_retry(&context.http_client, url, context.retry_opts, |client| {
            let mut builder = client.get(url);
            if let Some(auth_header) = auth_header {
                builder = builder.header("authorization", auth_header);
            }
            // Need full metadata for put update.
            builder
        })
        .await
        .map_err(FetchError::Request)?;

    if response.status() == StatusCode::NOT_FOUND {
        return Err(FetchError::NotFound);
    }
    if !response.status().is_success() {
        return Err(FetchError::Status { status: response.status() });
    }
    if response.content_length().is_some_and(|length| length > DEPRECATION_BODY_LIMIT as u64) {
        return Err(FetchError::BodyTooLarge);
    }
    let body =
        read_limited_body(response, DEPRECATION_BODY_LIMIT).await.map_err(FetchError::Body)?;
    if body.truncated {
        return Err(FetchError::BodyTooLarge);
    }
    serde_json::from_slice(&body.bytes).map_err(FetchError::InvalidJson)
}

fn map_fetch_error(error: FetchError, package_name: &str) -> miette::Report {
    match error {
        FetchError::Request(error) => registry_operation_error("requesting the registry", error),
        FetchError::Body(error) => registry_operation_error("reading the registry response", error),
        FetchError::InvalidJson(error) => {
            registry_operation_error("parsing the registry response", error)
        }
        FetchError::BodyTooLarge => DeprecateError::RegistryResponseTooLarge {
            resource: "package metadata",
            limit: DEPRECATION_BODY_LIMIT,
        }
        .into(),
        FetchError::NotFound => {
            DeprecateError::PackageNotFound { package_name: package_name.to_string() }.into()
        }
        FetchError::Status { status } => DeprecateError::RegistryFetchFailed {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or_default().to_string(),
        }
        .into(),
    }
}

pub(super) async fn put_package_meta(
    context: &DeprecateContext<'_>,
    url: &str,
    package_meta: &PackageMeta,
    auth_header: Option<&str>,
    otp: Option<&str>,
    is_deprecate: bool,
) -> miette::Result<()> {
    let body = serde_json::to_string(package_meta).expect("a struct serializes");
    let (_guard, response) =
        send_with_retry(&context.http_client, url, context.retry_opts, |client| {
            let mut builder =
                client.put(url).header("content-type", "application/json").body(body.clone());
            if let Some(auth_header) = auth_header {
                builder = builder.header("authorization", auth_header);
            }
            if let Some(otp) = otp {
                builder = builder.header("npm-otp", otp);
            }
            builder
        })
        .await
        .map_err(|source| {
            registry_operation_error("requesting the registry put endpoint", source)
        })?;
    if response.status().is_success() {
        return Ok(());
    }

    let action = if is_deprecate { "deprecate" } else { "undeprecate" }.to_string();
    Err(registry_write_error(response, action).await.into())
}

/// The error a failed registry write maps to, once its body is read.
pub(crate) async fn registry_write_error(response: Response, action: String) -> DeprecateError {
    let status = response.status();
    match read_limited_body(response, DEPRECATION_ERROR_BODY_LIMIT).await {
        Ok(body) => write_error_for_status(status, &body, action),
        Err(source) => registry_operation_failed("reading the registry error response", source),
    }
}

/// The error a failed registry write with an already-read `body` maps to.
pub(crate) fn write_error_for_status(
    status: StatusCode,
    body: &LimitedBody,
    action: String,
) -> DeprecateError {
    let status_text = status.canonical_reason().unwrap_or_default().to_string();
    let body = sanitize::body_display_string(body);
    if status == StatusCode::UNAUTHORIZED {
        return DeprecateError::Unauthorized { action, body };
    }
    if status == StatusCode::FORBIDDEN {
        return DeprecateError::Forbidden { action, body };
    }
    DeprecateError::RegistryWriteFailed { action, status: status.as_u16(), status_text, body }
}

pub(crate) fn registry_operation_error<ErrorType>(
    operation: &'static str,
    error: ErrorType,
) -> miette::Report
where
    ErrorType: std::fmt::Display,
{
    registry_operation_failed(operation, error).into()
}

pub(crate) fn registry_operation_failed<ErrorType>(
    operation: &'static str,
    error: ErrorType,
) -> DeprecateError
where
    ErrorType: std::fmt::Display,
{
    DeprecateError::RegistryOperationFailed {
        operation,
        reason: redact_url_credentials(&error.to_string()),
    }
}

pub(crate) fn registry_for_package(context: &DeprecateContext<'_>, package_name: &str) -> String {
    pick_registry_for_package(&context.registries, package_name, None)
}

pub(crate) fn auth_header_for_registry(
    context: &DeprecateContext<'_>,
    registry_url: &str,
    package_name: &str,
) -> Option<String> {
    context.config.auth_headers.for_url_with_package(registry_url, Some(package_name))
}

pub(crate) fn build_http_client(config: &Config) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &config.network_settings(),
    )
    .into_diagnostic()
    .wrap_err("create the network client for deprecate")
}

pub(crate) fn package_url(package_name: &str, registry_url: &str) -> miette::Result<String> {
    let package_name = package_name_for_url(package_name)?;
    registry_endpoint_url(registry_url, &escaped_package_name(&package_name))
}

pub(crate) fn package_name_for_url(package_name: &str) -> Result<String, DeprecateError> {
    parse_wanted_dependency(package_name)
        .alias
        .ok_or_else(|| DeprecateError::InvalidPackageSpec { spec: package_name.to_string() })
}

pub(crate) fn registry_endpoint_url(registry_url: &str, path: &str) -> miette::Result<String> {
    reqwest::Url::parse(&normalize_registry_url(registry_url))
        .and_then(|url| url.join(path))
        .map(|url| url.to_string())
        .map_err(|source| registry_operation_error("build registry URL", source))
}

pub(crate) fn normalize_registry_url(registry_url: &str) -> String {
    if registry_url.ends_with('/') { registry_url.to_string() } else { format!("{registry_url}/") }
}

pub(crate) fn escaped_package_name(package_name: &str) -> String {
    match package_name.strip_prefix('@') {
        Some(rest) => format!("@{}", encode_uri_component(rest).replace("%2F", "%2f")),
        None => encode_uri_component(package_name),
    }
}
