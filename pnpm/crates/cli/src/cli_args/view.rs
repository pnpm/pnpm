//! `pacquet view` (aliases `info`, `show`, `v`) — print package metadata
//! from the registry.
//!
//! When the package name is omitted, the nearest project manifest's `name`
//! is used (searching upward from `--dir`). With one or more trailing
//! field arguments, only those fields are printed; otherwise a formatted
//! summary (or, with `--json`, the whole assembled info object) is shown.

use super::deprecate::normalize_registry_url;
use chrono::{DateTime, Utc};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use owo_colors::{OwoColorize, Stream, Style};
use pnpm_config::Config;
use pnpm_network::{RetryOpts, ThrottledClient};
use pnpm_resolving_npm_resolver::{
    FetchFullMetadataOptions, FetchFullMetadataOutcome, PickPackageFromMetaOptions,
    fetch_full_metadata, parse_bare_specifier, pick_package_from_meta, pick_registry_for_package,
    pick_version_by_version_range,
};
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use pnpm_workspace::try_read_project_manifest;
use render::{render_fields, render_summary, to_pretty};
use serde_json::{Map, Value};
use std::{path::Path, sync::Arc};

/// Errors from `pacquet view`. The codes are the `ERR_PNPM_*` codes pnpm
/// defines for these failures.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ViewError {
    #[display("Package name is required. Usage: pnpm view [<package-name>]")]
    #[diagnostic(code(ERR_PNPM_MISSING_PACKAGE_NAME))]
    MissingPackageName,

    #[display("Invalid package name: \"{spec}\". This command only supports registry packages.")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidPackageName {
        #[error(not(source))]
        spec: String,
    },

    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_JSON))]
    InvalidPackageJson {
        #[error(not(source))]
        message: String,
    },

    #[display("No matching version found for {name}@{spec}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_NOT_FOUND))]
    PackageNotFound {
        #[error(not(source))]
        name: String,
        spec: String,
    },

    #[display("GET {url}: Not Found - 404")]
    #[diagnostic(code(ERR_PNPM_FETCH_404))]
    Fetch404 {
        #[error(not(source))]
        url: String,
    },
}

#[derive(Debug, Args)]
pub struct ViewArgs {
    /// `<package-name>[@<version>]` followed by optional fields to print
    /// (e.g. `view foo dist.tarball version`). When the package name is
    /// omitted, the nearest project manifest's name is used.
    pub params: Vec<String>,

    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    /// Show information in JSON format.
    #[clap(long)]
    pub json: bool,
}

impl ViewArgs {
    /// Resolve the package spec (the first positional, or the nearest
    /// manifest's name when omitted), fetch its registry metadata, pick the
    /// matching version, and render the requested fields, a JSON dump, or the
    /// formatted summary.
    pub async fn run(self, config: &Config, dir: &Path) -> miette::Result<String> {
        let package_spec = match self.params.first() {
            Some(spec) => spec.clone(),
            None => nearest_manifest_name(dir)?,
        };
        let fields = self.params.get(1..).unwrap_or(&[]);

        let (meta, picked) =
            fetch_package_metadata(config, self.registry.as_deref(), &package_spec, "view").await?;
        let info = assemble_info(&meta, &picked);

        if !fields.is_empty() {
            return Ok(render_fields(&info, fields, self.json));
        }
        if self.json {
            return Ok(to_pretty(&info));
        }
        Ok(render_summary(&info))
    }
}

/// Find the nearest project manifest's `name`, searching upward from
/// `start_dir`. A missing manifest is [`ViewError::MissingPackageName`]; a
/// present-but-invalid manifest (parse error, non-object body, or no `name`)
/// is [`ViewError::InvalidPackageJson`].
fn nearest_manifest_name(start_dir: &Path) -> Result<String, ViewError> {
    let mut dir = start_dir;
    loop {
        if dir.join("package.json").is_file() {
            return manifest_name(dir);
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return Err(ViewError::MissingPackageName),
        }
    }
}

/// The non-empty `name` of the manifest in `dir`. A body that is not an
/// object, or carries no usable name, is as invalid as one that fails to
/// parse.
fn manifest_name(dir: &Path) -> Result<String, ViewError> {
    let manifest = try_read_project_manifest(dir).map_err(|err| ViewError::InvalidPackageJson {
        message: format!(
            r#"Failed to read or parse project manifest in "{dir}": {err}"#,
            dir = dir.display(),
        ),
    })?;
    let value = manifest.map_or(Value::Null, |(_, manifest)| manifest.value().clone());
    let declared = value.get("name").and_then(Value::as_str);
    declared
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| invalid_manifest(dir))
}

/// The `ERR_PNPM_INVALID_PACKAGE_JSON` raised when a found manifest is not a
/// usable object or lacks a non-empty `name`.
fn invalid_manifest(dir: &Path) -> ViewError {
    ViewError::InvalidPackageJson {
        message: format!(
            r#"Invalid package.json at "{}". The "name" field is required and must be a non-empty string."#,
            dir.display(),
        ),
    }
}

/// Fetch the registry packument for `package_spec` and select the version that
/// satisfies the spec. The caller decides how much of the result to assemble.
pub(super) async fn fetch_package_metadata(
    config: &Config,
    registry_override: Option<&str>,
    package_spec: &str,
    command_name: &str,
) -> miette::Result<(pnpm_registry::Package, Arc<pnpm_registry::PackageVersion>)> {
    let parsed = parse_wanted_dependency(package_spec);
    let alias = parsed.alias.as_deref();
    let bare = parsed.bare_specifier.as_deref().unwrap_or("latest");
    let name_hint = alias.unwrap_or(package_spec);

    let mut registries: std::collections::HashMap<String, String> =
        config.resolved_registries().into_iter().collect();
    if let Some(registry) = registry_override {
        registries.insert("default".to_string(), normalize_registry_url(registry));
    }
    let registry = pick_registry_for_package(&registries, name_hint, Some(bare));

    let spec = parse_bare_specifier(bare, alias, "latest", &registry)
        .ok_or_else(|| ViewError::InvalidPackageName { spec: package_spec.to_string() })?;

    let http_client = metadata_client(config, command_name)?;
    let outcome = fetch_full_metadata(
        &spec.name,
        &FetchFullMetadataOptions {
            registry: &registry,
            http_client: &http_client,
            auth_headers: &config.auth_headers,
            full_metadata: true,
            etag: None,
            modified: None,
            retry_opts: RetryOpts::default(),
        },
    )
    .await
    .map_err(|error| map_fetch_error(error, &registry, &spec.name))?;

    let meta = match outcome {
        FetchFullMetadataOutcome::Modified(meta) => *meta,
        FetchFullMetadataOutcome::NotModified => {
            miette::bail!("registry returned 304 Not Modified unexpectedly for {}", spec.name)
        }
    };

    let picked = pick_view_version(&meta, &spec)?;

    Ok((meta, picked))
}

/// Build the info object the renderers consume from the picked version's
/// manifest and the packument-level fields. The picked version's raw JSON
/// fragment is reused so registry key order is preserved in `--json` output,
/// and the extra fields are appended in place.
fn assemble_info(meta: &pnpm_registry::Package, picked: &pnpm_registry::PackageVersion) -> Value {
    let mut info = match version_data(meta, picked) {
        Value::Object(map) => map,
        _ => Map::new(),
    };

    // An object author collapses to its `name` (dropped entirely when it has
    // none); a string author is left untouched.
    if let Some(Value::Object(author)) = info.get("author") {
        match author.get("name").cloned() {
            Some(name) => {
                info.insert("author".to_string(), name);
            }
            None => {
                info.shift_remove("author");
            }
        }
    }

    let versions: Vec<&String> = meta.versions.keys().collect();
    let versions_count = versions.len();
    let deps_count = picked.dependencies.as_ref().map_or(0, std::collections::HashMap::len);

    info.insert("versions".to_string(), serde_json::to_value(&versions).unwrap_or(Value::Null));
    if versions_count > 0 {
        info.insert("versionsCount".to_string(), serde_json::json!(versions_count));
    }
    if deps_count > 0 {
        info.insert("depsCount".to_string(), serde_json::json!(deps_count));
    }
    let dist_tags = serde_json::to_value(&meta.dist_tags).unwrap_or(Value::Null);
    info.insert("distTags".to_string(), dist_tags.clone());
    info.insert("dist-tags".to_string(), dist_tags);
    if let Some(time) = &meta.time {
        info.insert("time".to_string(), serde_json::to_value(time).unwrap_or(Value::Null));
    }

    Value::Object(info)
}

/// The picked version's own packument fragment, else the version as parsed.
fn version_data(meta: &pnpm_registry::Package, picked: &pnpm_registry::PackageVersion) -> Value {
    let version_key = picked.version.to_string();
    meta.versions
        .fragments()
        .find(|(version, _)| version.as_str() == version_key)
        .and_then(|(_, json)| serde_json::from_str::<Value>(&json).ok())
        .unwrap_or_else(|| serde_json::to_value(picked).unwrap_or(Value::Null))
}

/// Map a metadata-fetch failure to the matching pnpm error. A `404`
/// becomes [`ViewError::Fetch404`] (pnpm's `ERR_PNPM_FETCH_404`); every
/// other failure is surfaced verbatim.
fn map_fetch_error(
    error: pnpm_resolving_npm_resolver::FetchMetadataError,
    registry: &str,
    pkg_name: &str,
) -> miette::Report {
    use pnpm_resolving_npm_resolver::FetchMetadataError;
    if let FetchMetadataError::Network { error: ref source, .. } = error
        && source.status() == Some(reqwest::StatusCode::NOT_FOUND)
    {
        return ViewError::Fetch404 {
            url: pnpm_network::redact_url_credentials(&format!("{registry}{pkg_name}")),
        }
        .into();
    }
    miette::Report::new(error)
}

#[cfg(test)]
mod tests;

fn metadata_client(config: &Config, command_name: &str) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &config.network_settings(),
    )
    .into_diagnostic()
    .wrap_err_with(|| format!("create the network client for {command_name}"))
}

fn pick_view_version(
    meta: &pnpm_registry::Package,
    spec: &pnpm_resolving_npm_resolver::RegistryPackageSpec,
) -> miette::Result<Arc<pnpm_registry::PackageVersion>> {
    pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions::default(),
        meta,
        spec,
    )?
    .ok_or_else(|| {
        ViewError::PackageNotFound { name: spec.name.clone(), spec: spec.fetch_spec.clone() }.into()
    })
}

mod render;
