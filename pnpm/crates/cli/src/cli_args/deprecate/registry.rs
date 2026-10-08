use super::{
    BTreeMap, DEPRECATION_BODY_LIMIT, DEPRECATION_ERROR_BODY_LIMIT, DeprecateContext,
    DeprecateError, Deserialize, LimitedBody, Response, Serialize, StatusCode, read_limited_body,
    redact_url_credentials, retry_async, sanitize, send_with_retry,
};
use crate::cli_args::{
    package_spec::PackageSpec,
    registry_client::{
        apply_auth_and_otp, auth_header_for_package, package_endpoint_url,
        resolve_target_registry_for_package,
    },
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
            // Need full metadata for put update.
            apply_auth_and_otp(client.get(url), auth_header, None)
        })
        .await
        .map_err(FetchError::Request)?;

    if response.status() == StatusCode::NOT_FOUND {
        return Err(FetchError::NotFound);
    }
    if !response.status().is_success() {
        return Err(FetchError::Status { status: response.status() });
    }
    if response
        .content_length()
        .is_some_and(|length| length > DEPRECATION_BODY_LIMIT as u64)
    {
        return Err(FetchError::BodyTooLarge);
    }
    let body = read_limited_body(response, DEPRECATION_BODY_LIMIT).await.map_err(FetchError::Body)?;
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
            status_text: status
                .canonical_reason()
                .unwrap_or_default()
                .to_string(),
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
            let builder = client
                .put(url)
                .header("content-type", "application/json")
                .body(body.clone());
            apply_auth_and_otp(builder, auth_header, otp)
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
    let status_text = status
        .canonical_reason()
        .unwrap_or_default()
        .to_string();
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
    resolve_target_registry_for_package(
        &context.registries,
        context.registry_override,
        package_name,
        None,
    )
}

pub(crate) fn auth_header_for_registry(
    context: &DeprecateContext<'_>,
    registry_url: &str,
    package_name: &str,
) -> Option<String> {
    auth_header_for_package(context.config, registry_url, package_name)
}

pub(crate) fn package_url(package_name: &str, registry_url: &str) -> miette::Result<String> {
    let package_name = package_name_for_url(package_name)?;
    package_endpoint_url(registry_url, &package_name)
        .map_err(|source| registry_operation_error("build registry URL", source))
}

pub(crate) fn package_name_for_url(package_name: &str) -> Result<String, DeprecateError> {
    PackageSpec::parse(package_name)
        .map(|spec| spec.name)
        .ok_or_else(|| DeprecateError::InvalidPackageSpec { spec: package_name.to_string() })
}
