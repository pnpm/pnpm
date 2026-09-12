pub(crate) use registry::{
    auth_header_for_registry, build_http_client, fetch_package_meta, normalize_registry_url,
    package_url, registry_for_package, registry_operation_error, registry_operation_failed,
    registry_write_error, write_error_for_status,
};

use super::sanitize;
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use node_semver::Range;
use pnpm_config::Config;
use pnpm_network::{
    LimitedBody, RetryOpts, ThrottledClient, encode_uri_component, read_limited_body,
    redact_url_credentials, retry_async, send_with_retry,
};
use pnpm_resolving_npm_resolver::pick_registry_for_package;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use registry::{PackageMeta, put_package_meta};

use reqwest::{Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

const DEPRECATION_BODY_LIMIT: usize = 10 * 1024 * 1024;
pub(crate) const DEPRECATION_ERROR_BODY_LIMIT: usize = 64 * 1024;

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

    let mut package_meta: PackageMeta =
        fetch_package_meta(context, &package_url, auth_header.as_deref(), package_name).await?;

    if package_meta.versions.is_empty() {
        return Err(DeprecateError::NoVersions { package_name: package_name.to_string() }.into());
    }

    let versions_to_update = versions_matching(&package_meta, version_range);
    if versions_to_update.is_empty() {
        return Err(DeprecateError::NoMatchingVersions {
            version_range: version_range.unwrap_or("").to_string(),
        }
        .into());
    }

    validate_undeprecation(
        &package_meta,
        &versions_to_update,
        deprecated_message,
        package_name,
        version_range,
    )?;

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

/// The published versions the range selects, or every version when the
/// command named none.
///
/// Mirrors the TypeScript CLI's `semver.satisfies`, which treats an
/// unparsable range as matching nothing — the caller reports that as
/// `NoMatchingVersions` rather than a distinct "invalid spec" error.
fn versions_matching(package_meta: &PackageMeta, version_range: Option<&str>) -> Vec<String> {
    let Some(range_str) = version_range else {
        return package_meta.versions.keys().cloned().collect();
    };
    let Ok(range) = Range::parse(range_str) else {
        return Vec::new();
    };
    package_meta
        .versions
        .keys()
        .filter(|ver_str| {
            node_semver::Version::parse(ver_str).is_ok_and(|ver| range.satisfies(&ver))
        })
        .cloned()
        .collect()
}

pub(crate) fn parse_package_spec(spec: &str) -> Result<PackageSpec, DeprecateError> {
    let parsed = parse_wanted_dependency(spec);
    let name = parsed
        .alias
        .ok_or_else(|| DeprecateError::InvalidPackageSpec { spec: spec.to_string() })?;
    let version = parsed.bare_specifier.filter(|version| !version.is_empty());
    Ok(PackageSpec { name, version })
}

/// Undeprecating a range with no deprecated versions is an error.
fn validate_undeprecation(
    package_meta: &PackageMeta,
    versions_to_update: &[String],
    deprecated_message: Option<&str>,
    package_name: &str,
    version_range: Option<&str>,
) -> miette::Result<()> {
    let has_deprecated = versions_to_update.iter().any(|ver_str| {
        package_meta
            .versions
            .get(ver_str)
            .and_then(|info| info.deprecated.as_ref())
            .is_some_and(|dep| !dep.is_empty())
    });
    if deprecated_message.is_none() && !has_deprecated {
        return Err(DeprecateError::NotDeprecated {
            package_name: package_name.to_string(),
            version_range_suffix: version_range
                .map(|vr| format!(r#" matching "{vr}""#))
                .unwrap_or_default(),
        }
        .into());
    }

    Ok(())
}

mod registry;
