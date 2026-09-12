mod registry;

use super::sanitize;
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use node_semver::Version;
use pnpm_config::Config;
use pnpm_network::{
    RetryOpts, ThrottledClient, encode_uri_component, read_limited_body, redact_url_credentials,
    retry_async, send_with_retry,
};
use pnpm_resolving_npm_resolver::pick_registry_for_package;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use registry::{
    DeleteDistTagRequest, SetDistTagRequest, auth_header_for_registry, build_http_client,
    delete_dist_tag, fetch_dist_tags, normalize_registry_url, package_name_for_url,
    registry_for_package, set_dist_tag,
};
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

impl AuthType {
    fn header_value(self) -> &'static str {
        match self {
            AuthType::Legacy => "legacy",
            AuthType::Web => "web",
        }
    }
}
