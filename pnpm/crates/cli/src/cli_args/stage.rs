//! `pacquet stage` — stage packages for publishing, deferring
//! proof-of-presence (2FA) approval to a later point in time.
//!
//! `stage publish` runs the regular publish pipeline (packing, lifecycle
//! scripts, OIDC/OTP) against the registry's staging endpoint; the remaining
//! subcommands — `list`, `view`, `approve`, `reject`, `download` — talk to
//! the registry's `-/stage` API directly.

pub use registry::StageRegistryError;

mod approve;
mod summarize_tarball;

use super::{
    publish::{PublishArgs, PublishFlags},
    sanitize::body_display_string,
};
use crate::cli_args::registry_client::build_registry_client;
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_hooks::PnpmfileHooks;
use pnpm_network::{
    RetryOpts, ThrottledClient, read_limited_body, redact_url_credentials, send_with_retry,
};
use pnpm_network_web_auth::{
    Host as WebAuthHost, OtpChallenge, OtpError, OtpErrorBody, OtpSession, WebAuthFetchOptions,
    WebAuthRetryOptions, WithOtpError,
};
use pnpm_publish::{Host, PublishSummary, resolve_otp_from_env};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_resolving_npm_resolver::pick_registry_for_package;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;

use registry::{
    StageContext, fetch_stage_items, fetch_stage_tarball, stage_endpoint_url, stage_json_request,
    stage_request_in_session, stage_request_with_otp,
};
use render::{
    json_pretty, render_stage_item, render_stage_publish_summary, render_tarball_summary,
};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};
use summarize_tarball::{create_tarball_filename, summarize_tarball};

/// The staged-list page size; matches pnpm's paginated `-/stage` reads.
const PER_PAGE: usize = 100;
/// Fail-safe bound on the staged-list pagination loop, so a registry that
/// keeps answering full pages with an inflated `total` cannot drive it
/// forever.
const STAGE_LIST_MAX_PAGES: usize = 1000;
const STAGE_BODY_LIMIT: usize = 1024 * 1024;
const STAGE_ERROR_BODY_LIMIT: usize = 64 * 1024;
/// Cap on a staged tarball download; a registry response is
/// attacker-controlled input and must not exhaust memory.
const STAGE_TARBALL_BODY_LIMIT: usize = 512 * 1024 * 1024;
const STAGE_SUBCOMMANDS: &str = "publish, list, view, approve, reject, download";

#[derive(Debug, Args)]
pub struct StageArgs {
    /// Stage subcommand and arguments.
    pub params: Vec<String>,

    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    #[clap(flatten)]
    pub flags: PublishFlags,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum StageError {
    #[display("Stage subcommand is required")]
    #[diagnostic(code(ERR_PNPM_STAGE_SUBCOMMAND_REQUIRED), help("Use one of: {STAGE_SUBCOMMANDS}"))]
    SubcommandRequired,

    #[display(r#"Unknown stage subcommand "{subcommand}""#)]
    #[diagnostic(code(ERR_PNPM_STAGE_UNKNOWN_SUBCOMMAND), help("Use one of: {STAGE_SUBCOMMANDS}"))]
    UnknownSubcommand {
        #[error(not(source))]
        subcommand: String,
    },

    #[display(r#"Missing required <stage-id> for "pnpm stage {subcommand}""#)]
    #[diagnostic(code(ERR_PNPM_STAGE_ID_REQUIRED))]
    StageIdRequired {
        #[error(not(source))]
        subcommand: &'static str,
    },

    #[display("stage-id must be a valid UUID")]
    #[diagnostic(code(ERR_PNPM_INVALID_STAGE_ID))]
    InvalidStageId,

    #[display("Invalid package spec: {spec}")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_SPEC))]
    InvalidPackageSpec {
        #[error(not(source))]
        spec: String,
    },

    #[display("Version specifiers are not supported for listing staged packages")]
    #[diagnostic(code(ERR_PNPM_STAGE_VERSION_SPECIFIER_UNSUPPORTED))]
    VersionSpecifierUnsupported,

    #[display("Failed to {operation}: {reason}")]
    #[diagnostic(code(ERR_PNPM_STAGE_REGISTRY_ERROR))]
    RequestFailed {
        #[error(not(source))]
        operation: String,
        #[error(not(source))]
        reason: String,
    },

    #[display("Could not read package.json from tarball")]
    #[diagnostic(code(ERR_PNPM_STAGE_TARBALL_MANIFEST_NOT_FOUND))]
    TarballManifestNotFound,

    #[display(
        "Cannot approve stages {first_stage_id} and {second_stage_id} together because both publish {package_name}@{version}"
    )]
    #[diagnostic(code(ERR_PNPM_STAGE_DUPLICATE_PACKAGE))]
    DuplicateStagePackage {
        #[error(not(source))]
        first_stage_id: String,
        #[error(not(source))]
        second_stage_id: String,
        #[error(not(source))]
        package_name: String,
        #[error(not(source))]
        version: String,
    },

    #[display(r#"Invalid package name "{name}"."#)]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidPackageName {
        #[error(not(source))]
        name: String,
    },

    #[display(r#"Invalid package version "{version}"."#)]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_VERSION))]
    InvalidPackageVersion {
        #[error(not(source))]
        version: String,
    },

    #[display(r#"Invalid tarball filename "{filename}"."#)]
    #[diagnostic(code(ERR_PNPM_INVALID_TARBALL_FILENAME))]
    InvalidTarballFilename {
        #[error(not(source))]
        filename: String,
    },
}

/// One page of the registry's `-/stage` listing.
#[derive(Debug, Deserialize)]
struct StageListResponse {
    items: Vec<Value>,
    total: usize,
}

impl StageArgs {
    /// Dispatch to the requested stage subcommand and return the output to
    /// print, if any.
    pub async fn run<Reporter: self::Reporter>(
        self,
        dir: &Path,
        config: &Config,
        recursive: bool,
        before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
    ) -> miette::Result<Option<String>> {
        match self.params.first().map(String::as_str) {
            Some("publish") => {
                self.stage_publish::<Reporter>(dir, config, recursive, before_packing_hooks).await
            }
            Some("list") => self.stage_list(config).await,
            Some("view") => self.stage_view(config).await,
            Some("approve") => approve::stage_approve::<Reporter>(&self, config).await,
            Some("reject") => self.stage_reject::<Reporter>(config).await,
            Some("download") => self.stage_download(dir, config).await,
            None => Err(StageError::SubcommandRequired.into()),
            Some(other) => {
                Err(StageError::UnknownSubcommand { subcommand: other.to_owned() }.into())
            }
        }
    }

    /// `stage publish` — the regular publish pipeline against the staging
    /// endpoint, rendered as `+ <pkg> (staged with id <id>)` lines or a
    /// name-keyed JSON object.
    async fn stage_publish<Reporter: self::Reporter>(
        self,
        dir: &Path,
        config: &Config,
        recursive: bool,
        before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
    ) -> miette::Result<Option<String>> {
        let StageArgs { params, flags, .. } = self;
        let json = flags.json;
        let dry_run = flags.dry_run;
        let publish = PublishArgs { package: params.get(1).cloned(), flags };
        let published = publish
            .publish_packages::<Reporter>(
                dir,
                config,
                recursive,
                /* stage */ true,
                before_packing_hooks,
            )
            .await?;
        let summaries = published.summaries();
        if json {
            let keyed = key_by_package_name(summaries);
            return Ok(Some(json_pretty(&Value::Object(keyed))?));
        }
        if summaries.is_empty() {
            return Ok(None);
        }
        let lines: Vec<String> = summaries
            .iter()
            .map(|summary| render_stage_publish_summary(summary, dry_run))
            .collect();
        Ok(Some(lines.join("\n")))
    }

    /// `stage list [<package-spec>]` — every staged version, paginated.
    async fn stage_list(&self, config: &Config) -> miette::Result<Option<String>> {
        let package_filter = parse_package_filter(self.params.get(1))?;
        let context = self.stage_context(config, package_filter.as_deref())?;
        let items = fetch_stage_items(&context, package_filter.as_deref()).await?;

        if self.flags.json {
            return Ok(Some(json_pretty(&Value::Array(items))?));
        }
        if items.is_empty() {
            return Ok(Some(match package_filter {
                Some(package) => format!(r#"No staged versions of package name "{package}"."#),
                None => "No staged packages found.".to_owned(),
            }));
        }
        let rendered: Vec<String> = items.iter().map(render_stage_item).collect();
        Ok(Some(rendered.join("\n\n")))
    }

    /// `stage view <stage-id>` — one staged version's metadata.
    async fn stage_view(&self, config: &Config) -> miette::Result<Option<String>> {
        let stage_id = require_stage_id(&self.params, "view")?;
        let context = self.stage_context(config, None)?;
        let url = stage_endpoint_url(&context.registry, &format!("-/stage/{stage_id}"))?;
        let item: Value =
            stage_json_request(&context, url.as_str(), &format!("view staged package {stage_id}"))
                .await?;
        if self.flags.json {
            return Ok(Some(json_pretty(&item)?));
        }
        Ok(Some(render_stage_item(&item)))
    }

    /// `stage reject <stage-id>` — permanently delete the staged version.
    async fn stage_reject<Reporter: self::Reporter>(
        &self,
        config: &Config,
    ) -> miette::Result<Option<String>> {
        let stage_id = require_stage_id(&self.params, "reject")?;
        let context = self.stage_context(config, None)?;
        global_warn::<Reporter>(
            "Rejecting will permanently delete this staged publish record and tarball from the \
             registry.",
        );
        let url = stage_endpoint_url(&context.registry, &format!("-/stage/{stage_id}"))?;
        stage_request_with_otp::<Reporter>(
            &context,
            reqwest::Method::DELETE,
            url.as_str(),
            &format!("reject staged package {stage_id}"),
        )
        .await?;
        Ok(Some(format!("Staged package {stage_id} has been rejected.")))
    }

    /// `stage download <stage-id>` — fetch the staged tarball into `dir` and
    /// print its summary.
    async fn stage_download(&self, dir: &Path, config: &Config) -> miette::Result<Option<String>> {
        let stage_id = require_stage_id(&self.params, "download")?;
        let context = self.stage_context(config, None)?;
        let tarball_data = fetch_stage_tarball(&context, stage_id).await?;

        let mut summary = summarize_tarball(&tarball_data)?;
        let filename = create_tarball_filename(&summary.name, &summary.version, Some(stage_id))?;
        summary.filename.clone_from(&filename);
        let output_path = dir.join(&filename);
        // `create_tarball_filename` already rejects separators; this guards
        // the write against any bare-basename assumption it might not cover.
        if output_path.file_name().map(|name| name.to_string_lossy().into_owned())
            != Some(filename.clone())
            || output_path.parent() != Some(dir)
        {
            return Err(StageError::InvalidTarballFilename { filename }.into());
        }
        std::fs::write(&output_path, &tarball_data)
            .into_diagnostic()
            .wrap_err_with(|| format!("write {}", output_path.display()))?;

        if self.flags.json {
            let mut keyed = serde_json::Map::new();
            keyed.insert(
                summary.name.clone(),
                serde_json::to_value(&summary).expect("a publish summary serializes"),
            );
            return Ok(Some(json_pretty(&Value::Object(keyed))?));
        }
        Ok(Some(format!("{}\n{filename}", render_tarball_summary(&summary))))
    }

    /// Shared per-subcommand request context: the resolved registry, its auth
    /// header (package-scoped when a package filter is given), the network
    /// client, and the configured OTP.
    fn stage_context(
        &self,
        config: &Config,
        package_name: Option<&str>,
    ) -> miette::Result<StageContext> {
        let mut registries: HashMap<String, String> =
            config.resolved_registries().into_iter().collect();
        if let Some(registry) = &self.registry {
            registries.insert("default".to_owned(), registry.clone());
        }
        let registry = match package_name {
            Some(package) => pick_registry_for_package(&registries, package, None),
            None => registries.get("default").cloned().unwrap_or_default(),
        };
        let registry = if registry.ends_with('/') { registry } else { format!("{registry}/") };
        let auth_header = config.auth_headers.for_url_with_package(&registry, package_name);
        Ok(StageContext {
            registry,
            auth_header,
            http_client: build_registry_client(config)?,
            retry_opts: RetryOpts {
                retries: config.fetch_retries,
                factor: config.fetch_retry_factor,
                min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
                max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
            },
            otp: resolve_otp_from_env::<Host>(self.flags.otp.clone()),
            web_auth_fetch_options: WebAuthFetchOptions {
                timeout: Some(config.fetch_timeout),
                retry: Some(WebAuthRetryOptions {
                    factor: Some(f64::from(config.fetch_retry_factor)),
                    max_timeout: Some(config.fetch_retry_maxtimeout),
                    min_timeout: Some(config.fetch_retry_mintimeout),
                    randomize: None,
                    retries: Some(config.fetch_retries),
                }),
            },
        })
    }
}

/// The `<stage-id>` argument of `view` / `approve` / `reject` / `download`,
/// validated as a UUID.
fn require_stage_id<'params>(
    params: &'params [String],
    subcommand: &'static str,
) -> Result<&'params str, StageError> {
    let stage_id = params.get(1).map(String::as_str).unwrap_or_default();
    if stage_id.is_empty() {
        return Err(StageError::StageIdRequired { subcommand });
    }
    if !is_uuid(stage_id) {
        return Err(StageError::InvalidStageId);
    }
    Ok(stage_id)
}

/// Whether `value` is a hyphenated UUID (`8-4-4-4-12` hex digits).
fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.char_indices().all(|(index, char)| match index {
            8 | 13 | 18 | 23 => char == '-',
            _ => char.is_ascii_hexdigit(),
        })
}

/// The `list` package filter: a bare package name; a version specifier other
/// than `*` is rejected.
fn parse_package_filter(raw_spec: Option<&String>) -> Result<Option<String>, StageError> {
    let Some(raw_spec) = raw_spec.filter(|spec| !spec.is_empty()) else {
        return Ok(None);
    };
    let parsed = parse_wanted_dependency(raw_spec);
    let Some(name) = parsed.alias else {
        return Err(StageError::InvalidPackageSpec { spec: raw_spec.clone() });
    };
    match parsed.bare_specifier.as_deref() {
        None | Some("" | "*") => Ok(Some(name)),
        Some(_) => Err(StageError::VersionSpecifierUnsupported),
    }
}

/// The `--json` map `stage publish` and `stage download` print: summaries
/// keyed by package name.
fn key_by_package_name(summaries: &[PublishSummary]) -> serde_json::Map<String, Value> {
    let mut keyed = serde_json::Map::new();
    for summary in summaries {
        let key = if summary.name.is_empty() { summary.id.clone() } else { summary.name.clone() };
        if key.is_empty() {
            continue;
        }
        keyed.insert(key, serde_json::to_value(summary).expect("a publish summary serializes"));
    }
    keyed
}

fn global_info<Reporter: self::Reporter>(message: &str) {
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Info,
        message: message.to_owned(),
    }));
}

fn global_warn<Reporter: self::Reporter>(message: &str) {
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Warn,
        message: message.to_owned(),
    }));
}

#[cfg(test)]
mod tests;

mod registry;

mod render;
