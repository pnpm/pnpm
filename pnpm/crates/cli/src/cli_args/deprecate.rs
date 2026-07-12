use super::sanitize;
use clap::Args;
use derive_more::{Display, Error};
use futures_util::StreamExt as _;
use miette::{Context, Diagnostic, IntoDiagnostic};
use node_semver::Range;
use pacquet_config::Config;
use pacquet_network::{
    NetworkSettings, RetryOpts, ThrottledClient, encode_uri_component, redact_url_credentials,
    retry_async, send_with_retry,
};
use pacquet_resolving_npm_resolver::pick_registry_for_package;
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;
use reqwest::{Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

const DEPRECATION_BODY_LIMIT: usize = 10 * 1024 * 1024;
const DEPRECATION_ERROR_BODY_LIMIT: usize = 64 * 1024;

#[derive(Debug, Args)]
pub struct DeprecateArgs {
    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    /// One-time password for registries that require two-factor authentication.
    #[clap(long)]
    pub otp: Option<String>,

    /// The package name and the deprecation message.
    pub params: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum DeprecateError {
    #[display("Package name is required")]
    #[diagnostic(code(ERR_PNPM_DEPRECATE_REQUIRED))]
    PackageRequired,

    #[display("Deprecation message is required. To un-deprecate, use the undeprecate command.")]
    #[diagnostic(code(ERR_PNPM_DEPRECATE_MESSAGE_REQUIRED))]
    MessageRequired,

    #[display("Package name is required")]
    #[diagnostic(code(ERR_PNPM_UNDEPRECATE_REQUIRED))]
    UndeprecateRequired,

    #[display("The undeprecate command does not accept a message.")]
    #[diagnostic(code(ERR_PNPM_UNDEPRECATE_NO_MESSAGE))]
    UndeprecateNoMessage,

    #[display(r#"Package "{package_name}" not found in registry"#)]
    #[diagnostic(code(ERR_PNPM_PACKAGE_NOT_FOUND))]
    PackageNotFound {
        #[error(not(source))]
        package_name: String,
    },

    #[display(r#"Package "{package_name}" has no versions"#)]
    #[diagnostic(code(ERR_PNPM_NO_VERSIONS))]
    NoVersions {
        #[error(not(source))]
        package_name: String,
    },

    #[display(r#"No versions match "{version_range}""#)]
    #[diagnostic(code(ERR_PNPM_NO_MATCHING_VERSIONS))]
    NoMatchingVersions {
        #[error(not(source))]
        version_range: String,
    },

    #[display("No deprecated versions found in \"{package_name}\"{version_range_suffix}")]
    #[diagnostic(code(ERR_PNPM_NOT_DEPRECATED))]
    NotDeprecated {
        #[error(not(source))]
        package_name: String,
        #[error(not(source))]
        version_range_suffix: String,
    },

    #[display("Invalid package spec: {spec}")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_SPEC))]
    InvalidPackageSpec {
        #[error(not(source))]
        spec: String,
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

    #[display("Failed to fetch package info: {status} {status_text}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    RegistryFetchFailed {
        status: u16,
        #[error(not(source))]
        status_text: String,
    },

    #[display("Failed to {operation}: {reason}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    RegistryOperationFailed {
        #[error(not(source))]
        operation: &'static str,
        #[error(not(source))]
        reason: String,
    },

    #[display("Registry response for {resource} exceeded {limit} bytes")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_RESPONSE_TOO_LARGE))]
    RegistryResponseTooLarge {
        #[error(not(source))]
        resource: &'static str,
        limit: usize,
    },
}

pub(crate) struct DeprecateContext<'a> {
    pub(crate) config: &'a Config,
    pub(crate) http_client: ThrottledClient,
    pub(crate) retry_opts: RetryOpts,
    pub(crate) registries: HashMap<String, String>,
    pub(crate) otp: Option<String>,
}

impl DeprecateContext<'_> {
    pub(crate) fn new<'a>(
        config: &'a Config,
        registry: Option<&String>,
        otp: Option<String>,
    ) -> miette::Result<DeprecateContext<'a>> {
        let mut registries: HashMap<String, String> =
            config.resolved_registries().into_iter().collect();
        if let Some(registry) = registry {
            registries.insert("default".to_string(), normalize_registry_url(registry));
        }
        Ok(DeprecateContext {
            config,
            http_client: build_http_client(config)?,
            retry_opts: RetryOpts {
                retries: config.fetch_retries,
                factor: config.fetch_retry_factor,
                min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
                max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
            },
            registries,
            otp,
        })
    }
}

pub(crate) struct PackageSpec {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
}

impl DeprecateArgs {
    pub async fn run(self, config: &Config) -> miette::Result<Option<String>> {
        let context = DeprecateContext::new(config, self.registry.as_ref(), self.otp)?;

        let spec = self.params.first().ok_or(DeprecateError::PackageRequired)?;
        let PackageSpec { name: package_name, version } = parse_package_spec(spec)?;

        let message = self
            .params
            .get(1..)
            .map(|parts| parts.join(" "))
            .filter(|msg| !msg.is_empty())
            .ok_or(DeprecateError::MessageRequired)?;

        let output =
            update_deprecation(&context, Some(&message), &package_name, version.as_deref()).await?;
        Ok(Some(output))
    }
}

pub(crate) async fn update_deprecation(
    context: &DeprecateContext<'_>,
    deprecated_message: Option<&str>,
    package_name: &str,
    version_range: Option<&str>,
) -> miette::Result<String> {
    let registry_url = registry_for_package(context, package_name);
    let auth_header = auth_header_for_registry(context, &registry_url, package_name);

    let package_url = package_url(package_name, &registry_url)?;

    let mut package_meta =
        fetch_package_meta(context, &package_url, auth_header.as_deref(), package_name).await?;

    if package_meta.versions.is_empty() {
        return Err(DeprecateError::NoVersions { package_name: package_name.to_string() }.into());
    }

    let versions_to_update: Vec<String> = if let Some(range_str) = version_range {
        let range = Range::parse(range_str)
            .map_err(|_| DeprecateError::InvalidPackageSpec { spec: range_str.to_string() })?;
        package_meta
            .versions
            .keys()
            .filter(|ver_str| {
                if let Ok(ver) = node_semver::Version::parse(ver_str) {
                    range.satisfies(&ver)
                } else {
                    false
                }
            })
            .cloned()
            .collect()
    } else {
        package_meta.versions.keys().cloned().collect()
    };

    if versions_to_update.is_empty() {
        return Err(DeprecateError::NoMatchingVersions {
            version_range: version_range.unwrap_or("").to_string(),
        }
        .into());
    }

    if deprecated_message.is_none() {
        let has_deprecated = versions_to_update.iter().any(|ver_str| {
            package_meta
                .versions
                .get(ver_str)
                .and_then(|info| info.deprecated.as_ref())
                .is_some_and(|dep| !dep.is_empty())
        });
        if !has_deprecated {
            return Err(DeprecateError::NotDeprecated {
                package_name: package_name.to_string(),
                version_range_suffix: version_range
                    .map(|vr| format!(r#" matching "{vr}""#))
                    .unwrap_or_default(),
            }
            .into());
        }
    }

    for ver in &versions_to_update {
        if let Some(info) = package_meta.versions.get_mut(ver) {
            info.deprecated = Some(deprecated_message.map(ToString::to_string).unwrap_or_default());
        }
    }

    put_package_meta(
        context,
        &package_url,
        &package_meta,
        auth_header.as_deref(),
        context.otp.as_deref(),
        deprecated_message.is_some(),
    )
    .await?;

    let verb = if deprecated_message.is_some() { "deprecated" } else { "un-deprecated" };
    Ok(format!("Successfully {} {} version(s) of {}", verb, versions_to_update.len(), package_name))
}

#[derive(Debug, Serialize, Deserialize)]
struct PackageMeta {
    #[serde(default)]
    versions: BTreeMap<String, VersionInfo>,
    #[serde(flatten)]
    other: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct VersionInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    deprecated: Option<String>,
    #[serde(flatten)]
    other: serde_json::Value,
}

async fn fetch_package_meta(
    context: &DeprecateContext<'_>,
    url: &str,
    auth_header: Option<&str>,
    package_name: &str,
) -> miette::Result<PackageMeta> {
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
        matches!(self, Self::Body(_))
    }
}

async fn fetch_package_meta_once(
    context: &DeprecateContext<'_>,
    url: &str,
    auth_header: Option<&str>,
) -> Result<PackageMeta, FetchError> {
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

async fn put_package_meta(
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
    write_error_from_response(response, action).await
}

async fn write_error_from_response(response: Response, action: String) -> miette::Result<()> {
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();
    let body =
        read_limited_body(response, DEPRECATION_ERROR_BODY_LIMIT).await.map_err(|source| {
            registry_operation_error("reading the registry error response", source)
        })?;
    let body = body.into_display_string();
    if status == StatusCode::UNAUTHORIZED {
        return Err(DeprecateError::Unauthorized { action, body }.into());
    }
    if status == StatusCode::FORBIDDEN {
        return Err(DeprecateError::Forbidden { action, body }.into());
    }
    Err(DeprecateError::RegistryWriteFailed { action, status: status.as_u16(), status_text, body }
        .into())
}

fn registry_operation_error<ErrorType>(operation: &'static str, error: ErrorType) -> miette::Report
where
    ErrorType: std::fmt::Display,
{
    DeprecateError::RegistryOperationFailed {
        operation,
        reason: redact_url_credentials(&error.to_string()),
    }
    .into()
}

pub(crate) struct LimitedBody {
    bytes: Vec<u8>,
    truncated: bool,
}

impl LimitedBody {
    pub(crate) fn into_display_string(self) -> String {
        let body = String::from_utf8_lossy(&self.bytes);
        let mut body = sanitize::sanitize(&body).into_owned();
        if self.truncated {
            if !body.is_empty() && !body.chars().next_back().is_some_and(char::is_whitespace) {
                body.push(' ');
            }
            body.push_str("(response body truncated)");
        }
        body
    }
}

pub(crate) async fn read_limited_body(
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
    Ok(LimitedBody { bytes, truncated })
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
        &NetworkSettings {
            network_concurrency: config.network_concurrency,
            fetch_timeout: Duration::from_millis(config.fetch_timeout),
            user_agent: config.user_agent.clone(),
        },
    )
    .into_diagnostic()
    .wrap_err("create the network client for deprecate")
}

pub(crate) fn parse_package_spec(spec: &str) -> Result<PackageSpec, DeprecateError> {
    let parsed = parse_wanted_dependency(spec);
    let name = parsed
        .alias
        .ok_or_else(|| DeprecateError::InvalidPackageSpec { spec: spec.to_string() })?;
    let version = parsed.bare_specifier.filter(|version| !version.is_empty());
    Ok(PackageSpec { name, version })
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
