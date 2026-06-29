use super::sanitize;
use clap::Args;
use derive_more::{Display, Error};
use futures_util::StreamExt as _;
use miette::{Context, Diagnostic, IntoDiagnostic};
use node_semver::Version;
use pacquet_config::Config;
use pacquet_network::{
    NetworkSettings, RetryOpts, ThrottledClient, encode_uri_component, redact_url_credentials,
    retry_async, send_with_retry,
};
use pacquet_resolving_npm_resolver::pick_registry_for_package;
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

const DIST_TAGS_BODY_LIMIT: usize = 1024 * 1024;
const DIST_TAG_ERROR_BODY_LIMIT: usize = 64 * 1024;

#[derive(Debug, Args)]
pub struct DistTagArgs {
    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    /// One-time password for registries that require two-factor authentication.
    #[clap(long)]
    pub otp: Option<String>,

    /// dist-tag subcommand and arguments.
    pub params: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum DistTagError {
    #[display("Package name is required")]
    #[diagnostic(code(ERR_PNPM_DIST_TAG_LS_PACKAGE_REQUIRED))]
    LsPackageRequired,

    #[display("Package name and version are required (e.g., pnpm dist-tag add pkg@1.0.0 latest)")]
    #[diagnostic(code(ERR_PNPM_DIST_TAG_ADD_SPEC_REQUIRED))]
    AddSpecRequired,

    #[display("Version is required (e.g., pnpm dist-tag add pkg@1.0.0 latest)")]
    #[diagnostic(code(ERR_PNPM_DIST_TAG_ADD_VERSION_REQUIRED))]
    AddVersionRequired,

    #[display(r#"Version must be an exact semver version, got "{version}""#)]
    #[diagnostic(code(ERR_PNPM_DIST_TAG_ADD_INVALID_VERSION))]
    AddInvalidVersion {
        #[error(not(source))]
        version: String,
    },

    #[display("Package name and tag are required (e.g., pnpm dist-tag rm pkg tag)")]
    #[diagnostic(code(ERR_PNPM_DIST_TAG_RM_ARGS_REQUIRED))]
    RmArgsRequired,

    #[display(r#"Removing the "latest" dist-tag is not allowed"#)]
    #[diagnostic(code(ERR_PNPM_DIST_TAG_RM_LATEST))]
    RmLatest,

    #[display(r#"dist-tag "{tag}" is not set on package "{package_name}""#)]
    #[diagnostic(code(ERR_PNPM_DIST_TAG_NOT_FOUND))]
    DistTagNotFound {
        #[error(not(source))]
        tag: String,
        #[error(not(source))]
        package_name: String,
    },

    #[display(r#"Package "{package_name}" not found in registry"#)]
    #[diagnostic(code(ERR_PNPM_PACKAGE_NOT_FOUND))]
    PackageNotFound {
        #[error(not(source))]
        package_name: String,
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

    #[display(
        "This registry requires web-based OTP to {action} packages. Open {auth_url}, wait for {done_url} to finish, then rerun with --otp <token>."
    )]
    #[diagnostic(code(ERR_PNPM_DIST_TAG_WEB_OTP_REQUIRED))]
    WebOtpRequired {
        #[error(not(source))]
        action: String,
        #[error(not(source))]
        auth_url: String,
        #[error(not(source))]
        done_url: String,
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

#[derive(Clone, Copy)]
enum AuthType {
    Legacy,
    Web,
}

struct DistTagContext<'a> {
    config: &'a Config,
    http_client: ThrottledClient,
    retry_opts: RetryOpts,
    registries: HashMap<String, String>,
    otp: Option<String>,
}

struct PackageSpec {
    name: String,
    version: Option<String>,
}

impl DistTagArgs {
    pub async fn run(self, config: &Config) -> miette::Result<Option<String>> {
        let context = self.context(config)?;
        let Some(subcommand) = self.params.first().map(String::as_str) else {
            return dist_tag_ls(&context, &[]).await.map(Some);
        };
        match subcommand {
            "add" => dist_tag_add(&context, &self.params[1..]).await.map(Some),
            "rm" => dist_tag_rm(&context, &self.params[1..]).await.map(Some),
            "ls" | "list" => dist_tag_ls(&context, &self.params[1..]).await.map(Some),
            _ => dist_tag_ls(&context, &self.params).await.map(Some),
        }
    }

    fn context<'config>(&self, config: &'config Config) -> miette::Result<DistTagContext<'config>> {
        let mut registries: HashMap<String, String> =
            config.resolved_registries().into_iter().collect();
        if let Some(registry) = &self.registry {
            registries.insert("default".to_string(), normalize_registry_url(registry));
        }
        Ok(DistTagContext {
            config,
            http_client: build_http_client(config)?,
            retry_opts: RetryOpts {
                retries: config.fetch_retries,
                factor: config.fetch_retry_factor,
                min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
                max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
            },
            registries,
            otp: self.otp.clone(),
        })
    }
}

async fn dist_tag_ls(context: &DistTagContext<'_>, params: &[String]) -> miette::Result<String> {
    let package_name = params.first().ok_or(DistTagError::LsPackageRequired)?;
    let package_name = package_name_for_url(package_name)?;
    let registry_url = registry_for_package(context, &package_name);
    let auth_header = auth_header_for_registry(context, &registry_url, &package_name);
    let dist_tags =
        fetch_dist_tags(context, &package_name, &registry_url, auth_header.as_deref()).await?;
    let mut lines = Vec::with_capacity(dist_tags.len());
    for (tag, version) in dist_tags {
        lines.push(format!("{tag}: {version}"));
    }
    Ok(lines.join("\n"))
}

async fn dist_tag_add(context: &DistTagContext<'_>, params: &[String]) -> miette::Result<String> {
    let spec = params.first().ok_or(DistTagError::AddSpecRequired)?;
    let PackageSpec { name: package_name, version } = parse_package_spec(spec)?;
    let raw_version = version.ok_or(DistTagError::AddVersionRequired)?;
    let Some(version) = normalize_exact_semver(&raw_version) else {
        return Err(DistTagError::AddInvalidVersion { version: raw_version }.into());
    };
    let tag = params.get(1).map_or("latest", String::as_str);
    let registry_url = registry_for_package(context, &package_name);
    let auth_header = auth_header_for_registry(context, &registry_url, &package_name);
    let auth_type = if context.otp.is_some() { AuthType::Legacy } else { AuthType::Web };
    set_dist_tag(
        context,
        SetDistTagRequest {
            package_name: &package_name,
            version: &version,
            tag,
            registry_url: &registry_url,
            auth_header: auth_header.as_deref(),
            auth_type,
            otp: context.otp.as_deref(),
        },
    )
    .await?;
    Ok(format!("+{tag}: {package_name}@{version}"))
}

async fn dist_tag_rm(context: &DistTagContext<'_>, params: &[String]) -> miette::Result<String> {
    if params.len() < 2 {
        return Err(DistTagError::RmArgsRequired.into());
    }
    let package_name = package_name_for_url(&params[0])?;
    let tag = &params[1];
    if tag == "latest" {
        return Err(DistTagError::RmLatest.into());
    }
    let registry_url = registry_for_package(context, &package_name);
    let auth_header = auth_header_for_registry(context, &registry_url, &package_name);
    let dist_tags =
        fetch_dist_tags(context, &package_name, &registry_url, auth_header.as_deref()).await?;
    let version = dist_tags.get(tag).ok_or_else(|| DistTagError::DistTagNotFound {
        tag: tag.clone(),
        package_name: package_name.clone(),
    })?;
    let auth_type = if context.otp.is_some() { AuthType::Legacy } else { AuthType::Web };
    delete_dist_tag(
        context,
        DeleteDistTagRequest {
            package_name: &package_name,
            tag,
            registry_url: &registry_url,
            auth_header: auth_header.as_deref(),
            auth_type,
            otp: context.otp.as_deref(),
        },
    )
    .await?;
    Ok(format!("-{tag}: {package_name}@{version}"))
}

struct SetDistTagRequest<'a> {
    package_name: &'a str,
    version: &'a str,
    tag: &'a str,
    registry_url: &'a str,
    auth_header: Option<&'a str>,
    auth_type: AuthType,
    otp: Option<&'a str>,
}

async fn set_dist_tag(
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

struct DeleteDistTagRequest<'a> {
    package_name: &'a str,
    tag: &'a str,
    registry_url: &'a str,
    auth_header: Option<&'a str>,
    auth_type: AuthType,
    otp: Option<&'a str>,
}

async fn delete_dist_tag(
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

async fn fetch_dist_tags(
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
    let body = body.into_display_string();
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

struct LimitedBody {
    bytes: Vec<u8>,
    truncated: bool,
}

impl LimitedBody {
    fn into_display_string(self) -> String {
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
    Ok(LimitedBody { bytes, truncated })
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

fn registry_for_package(context: &DistTagContext<'_>, package_name: &str) -> String {
    pick_registry_for_package(&context.registries, package_name, None)
}

fn auth_header_for_registry(
    context: &DistTagContext<'_>,
    registry_url: &str,
    package_name: &str,
) -> Option<String> {
    context.config.auth_headers.for_url_with_package(registry_url, Some(package_name))
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
    .wrap_err("create the network client for dist-tag")
}

fn parse_package_spec(spec: &str) -> Result<PackageSpec, DistTagError> {
    let parsed = parse_wanted_dependency(spec);
    let name =
        parsed.alias.ok_or_else(|| DistTagError::InvalidPackageSpec { spec: spec.to_string() })?;
    let version = parsed.bare_specifier.filter(|version| !version.is_empty());
    Ok(PackageSpec { name, version })
}

fn normalize_exact_semver(version: &str) -> Option<String> {
    if let Some(version) = version.strip_prefix('v')
        && Version::parse(version).is_ok()
    {
        return Some(version.to_string());
    }
    if Version::parse(version).is_ok() {
        return Some(version.to_string());
    }
    None
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

fn package_name_for_url(package_name: &str) -> Result<String, DistTagError> {
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

fn normalize_registry_url(registry_url: &str) -> String {
    if registry_url.ends_with('/') { registry_url.to_string() } else { format!("{registry_url}/") }
}

fn escaped_package_name(package_name: &str) -> String {
    match package_name.strip_prefix('@') {
        Some(rest) => format!("@{}", encode_uri_component(rest).replace("%2F", "%2f")),
        None => encode_uri_component(package_name),
    }
}



impl AuthType {
    fn header_value(self) -> &'static str {
        match self {
            AuthType::Legacy => "legacy",
            AuthType::Web => "web",
        }
    }
}
