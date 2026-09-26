pub(crate) use fix::{
    AuditFixObserver, PackumentPublishInfo, VulnerabilityGuard, fetch_publish_times,
    filter_advisories_for_fix, fix_override, fix_with_update, format_fix_with_update_output,
    ignore_vulnerabilities, interactive_select, prune_ignored_ghsas,
};
pub(crate) use paths::{AuditPathIndex, PathInfo, build_audit_path_index, package_version};
pub(crate) use render::{
    blue, bold, color_severity, green, red, render_json_report, render_text_report,
};
pub(crate) use report::{
    AuditAdvisory, AuditError, AuditReport, AuditVulnerabilityCounts, RawBulkAdvisory,
    bulk_response_to_audit_report, empty_audit_report, normalize_ghsa_id, normalize_registry,
    redact_url_userinfo, sanitize_response_body,
};
pub(crate) use request::{
    AuditGraph, AuditIndexRequest, DepClass, DepKind, Edge, GraphImporter, Include, classify_graph,
    empty_packages, empty_snapshots, env_roots, importer_roots, lockfile_to_audit_request,
    root_included,
};
pub(crate) use version_ranges::{
    caret_range_for_patched, infer_patched_versions, is_range_subset, min_version_from_range,
    patched_range_for_style, satisfies_including_prerelease, satisfies_safe,
};

use crate::{
    State,
    cli_args::{install::resolve_bool_override, sanitize::sanitize_inline},
};
use advisories::{
    audit, correct_inferred_patched_versions, filter_ignored_advisories, parse_audit_level,
    retry_opts_from_config, severity_name, severity_number,
};
use chrono::{DateTime, Utc};
use clap::{Args, ValueEnum};
use derive_more::{Display, Error};
use dialoguer::MultiSelect;
use importers::{select_audited_importers, signature_packages};

use miette::{Diagnostic, IntoDiagnostic};
use node_semver::{Range, Version};
use owo_colors::{OwoColorize, Stream};

use pnpm_config::{AuditLevel as ConfigAuditLevel, Config};
use pnpm_lockfile::{
    EnvLockfile, ImporterDepVersion, Lockfile, PackageKey, PackageMetadata, PeerEdgeGraph,
    PeerEdgeOptions, PeerSatisfactionEdges, PkgName, ResolvedDependencyMap, SnapshotEntry,
    SpecifierAndResolution, pick_registry_for_package,
};
use pnpm_network::{RetryOpts, encode_package_name, send_with_retry};
use pnpm_package_manager::{ResolutionObserver, ResolvedPackageHint, Update};
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::{
    GuardExhaustionPolicy, PackageVersionGuard, PackageVersionGuardDecision,
    PackageVersionGuardFuture, parse_packument_timestamp,
};

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Write,
    path::Path,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

mod fix;
mod importers;
mod packages;
mod paths;
mod render;
mod report;
mod request;
mod version_ranges;

mod signatures;

const MAX_PATHS_COUNT: usize = 3;
const MAX_PATHS_PER_FINDING: usize = 100;

#[derive(Debug, Args)]
pub struct AuditArgs {
    /// Output audit report in JSON format.
    #[clap(long)]
    pub json: bool,
    /// --prod, --dev, and --no-optional.
    #[clap(flatten)]
    pub dependency_options: AuditDependencyOptions,
    /// Use exit code 0 if the registry responds with an error.
    #[clap(long = "ignore-registry-errors")]
    pub ignore_registry_errors: bool,
    /// Fix the audited vulnerabilities using the specified method:
    /// "override" or "update". "override" adds overrides to
    /// `pnpm-workspace.yaml` to force non-vulnerable versions; "update"
    /// re-resolves the lockfile to non-vulnerable versions. Defaults to
    /// "override" when no method is given.
    #[clap(long, value_name = "METHOD", num_args = 0..=1, default_missing_value = "override")]
    pub fix: Option<String>,
    /// Show vulnerabilities and select which ones to fix interactively.
    #[clap(short = 'i', long)]
    pub interactive: bool,
    /// Packages to audit instead of the project (`<name>[@<version>]`),
    /// or the `signatures` subcommand, which verifies registry signatures
    /// for the installed packages.
    pub params: Vec<String>,
    #[clap(flatten)]
    pub advisories: AdvisoryFilterArgs,
}

#[derive(Debug, Clone, clap::Args)]
pub struct AdvisoryFilterArgs {
    /// Only print advisories with severity greater than or equal to this level.
    #[clap(long = "audit-level", value_enum)]
    pub audit_level: Option<AuditLevelArg>,
    /// Ignore a vulnerability by its GitHub advisory ID (e.g.
    /// GHSA-xxxx-xxxx-xxxx). May be repeated.
    #[clap(long, value_name = "GHSA")]
    pub ignore: Vec<String>,
    /// Ignore all vulnerabilities for which no fix exists.
    #[clap(long = "ignore-unfixable")]
    pub ignore_unfixable: bool,
}

/// What a fix flow needs beyond the report itself.
struct FixContext<'a> {
    audit_level: ConfigAuditLevel,
    lockfile_dir: &'a std::path::Path,
    settings_dir: &'a std::path::Path,
    publish_infos: &'a HashMap<String, Option<PackumentPublishInfo>>,
}

/// Which `--fix` strategy to apply. Mirrors pnpm's `'override' | 'update'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixMethod {
    Override,
    Update,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum AuditLevelArg {
    Info,
    Low,
    Moderate,
    High,
    Critical,
}

impl From<AuditLevelArg> for ConfigAuditLevel {
    fn from(value: AuditLevelArg) -> Self {
        match value {
            AuditLevelArg::Info => ConfigAuditLevel::Info,
            AuditLevelArg::Low => ConfigAuditLevel::Low,
            AuditLevelArg::Moderate => ConfigAuditLevel::Moderate,
            AuditLevelArg::High => ConfigAuditLevel::High,
            AuditLevelArg::Critical => ConfigAuditLevel::Critical,
        }
    }
}

#[derive(Debug, Args)]
pub struct AuditDependencyOptions {
    /// Only audit "dependencies" and "optionalDependencies".
    #[clap(short = 'P', long, visible_alias = "production")]
    prod: bool,
    /// Only audit "devDependencies".
    #[clap(short = 'D', long)]
    dev: bool,
    /// Don't audit "optionalDependencies".
    #[clap(long, overrides_with = "optional")]
    no_optional: bool,
    /// Include "optionalDependencies".
    #[clap(long, overrides_with = "no_optional")]
    optional: bool,
}

impl AuditDependencyOptions {
    fn include(&self, config: &Config) -> Include {
        let mut dependencies = true;
        let mut dev_dependencies = true;
        let mut optional_dependencies =
            resolve_bool_override(self.optional, self.no_optional, config.optional);
        if self.prod {
            dev_dependencies = false;
        } else if self.dev {
            dependencies = false;
            optional_dependencies = false;
        }
        Include {
            dependencies,
            dev_dependencies,
            optional_dependencies,
            peer_edges: config.peer_edge_options(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditOutcome {
    Clean,
    Vulnerable,
}

impl AuditArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
    ) -> miette::Result<AuditOutcome> {
        if !self.params.is_empty() {
            return self.run_subcommand(state).await;
        }

        let include = self.dependency_options.include(state.config);
        let audit_level = self.advisories.effective_level(state.config.audit_level);
        let fix_method = self.resolve_fix_method()?;

        let lockfile_dir = state.lockfile_dir().to_path_buf();
        // pnpm writes settings to `workspaceDir ?? rootProjectManifestDir`.
        let settings_dir =
            state.config.workspace_dir.clone().unwrap_or_else(|| lockfile_dir.clone());

        let Some(mut report) =
            self.fetch_report(&state, include, audit_level, &lockfile_dir).await?
        else {
            return Ok(AuditOutcome::Clean);
        };
        // The inferred patched range is syntactic: verify a published version
        // actually satisfies it before the report and any fix flow can claim
        // one. The fetched publish-time maps are reused by the fix flows for
        // the age-gate exclusion check.
        let publish_infos = correct_inferred_patched_versions(
            &mut report,
            state.config,
            state.http_client.as_ref(),
        )
        .await;

        if let Some(fix_method) = fix_method {
            return self.run_fix::<Reporter>(
                fix_method,
                &mut state,
                &report,
                &FixContext {
                    audit_level,
                    lockfile_dir: &lockfile_dir,
                    settings_dir: &settings_dir,
                    publish_infos: &publish_infos,
                },
            )
            .await;
        }

        self.render_report(report, state.config, &settings_dir, audit_level)
    }

    fn render_report(
        &self,
        mut report: AuditReport,
        config: &Config,
        settings_dir: &Path,
        audit_level: ConfigAuditLevel,
    ) -> miette::Result<AuditOutcome> {
        if !self.advisories.ignore.is_empty() || self.advisories.ignore_unfixable {
            let output = ignore_vulnerabilities(
                &report,
                config,
                settings_dir,
                &self.advisories.ignore,
                self.advisories.ignore_unfixable,
            )?;
            print_command_output(&output);
            return Ok(AuditOutcome::Clean);
        }

        let ignored = filter_ignored_advisories(&mut report, config);

        let output = if self.json {
            render_json_report(&report, audit_level)?
        } else {
            render_text_report(&report, audit_level, &ignored)
        };
        print_command_output(&output);

        Ok(audit_outcome(&report, audit_level))
    }

    /// `audit` takes exactly one subcommand, `signatures`. Any other first
    /// parameter names a package, which [`Self::run_packages`] audits.
    async fn run_subcommand(&self, state: State) -> miette::Result<AuditOutcome> {
        if self.params.len() > 1 {
            return Err(AuditError::UnknownSubcommand {
                subcommand: self.params
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" "),
            }
            .into());
        }
        self.run_signatures(state).await
    }

    /// Fetch the audit report. `None` when the selectors matched no project,
    /// or when a registry error was swallowed per `--ignore-registry-errors`,
    /// matching pnpm's catch around the `audit()` call; under `--json` the
    /// empty report has already been printed by then.
    ///
    /// Takes `state` by shared reference so the `--fix update` path can
    /// re-borrow it mutably once the report is in hand.
    async fn fetch_report(
        &self,
        state: &State,
        include: Include,
        audit_level: ConfigAuditLevel,
        lockfile_dir: &std::path::Path,
    ) -> miette::Result<Option<AuditReport>> {
        let lockfile = state.lockfile
            .get()
            .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
        let Some(lockfile) = lockfile else {
            return Err(AuditError::NoLockfile.into());
        };
        let Some(lockfile) = select_audited_importers(state, lockfile)? else {
            return Ok(None);
        };
        self.audit_lockfile(state, lockfile.as_ref(), include, audit_level, lockfile_dir).await
    }

    /// Request the advisories for `lockfile`. `None` when a registry error
    /// was swallowed per `--ignore-registry-errors`.
    async fn audit_lockfile(
        &self,
        state: &State,
        lockfile: &Lockfile,
        include: Include,
        audit_level: ConfigAuditLevel,
        lockfile_dir: &std::path::Path,
    ) -> miette::Result<Option<AuditReport>> {
        let env_lockfile = EnvLockfile::read(lockfile_dir)
            .map_err(|err| miette::Report::new(err).wrap_err("load the env lockfile"))?;
        match audit(
            lockfile,
            env_lockfile.as_ref(),
            include,
            state.config,
            state.http_client.as_ref(),
        )
        .await
        {
            Ok(report) => Ok(Some(report)),
            Err(err) if self.ignore_registry_errors => {
                eprintln!("{err}");
                let _ = std::io::stderr().flush();
                if self.json {
                    let report = empty_audit_report(lockfile, env_lockfile.as_ref(), include);
                    print_command_output(&render_json_report(&report, audit_level)?);
                }
                Ok(None)
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Handle `audit signatures`: verify registry signatures for every
    /// installed package and print the report. Exit code 1 (via
    /// [`AuditOutcome::Vulnerable`]) when any signature is missing or invalid.
    /// Ports pnpm's `auditSignatures`.
    async fn run_signatures(&self, state: State) -> miette::Result<AuditOutcome> {
        let include = self.dependency_options.include(state.config);
        let lockfile_dir = state.lockfile_dir().to_path_buf();

        let Some(packages) = signature_packages(&state, include, &lockfile_dir)? else {
            return Ok(AuditOutcome::Clean);
        };
        if packages.is_empty() {
            return Err(AuditError::NoPackages.into());
        }

        let result =
            signatures::verify_signatures(&packages, state.config, state.http_client.as_ref())
                .await?;

        let output = if self.json {
            serde_json::to_string_pretty(&result).into_diagnostic()?
        } else {
            signatures::render_signature_verification_result(&result)
        };
        print_command_output(&output);

        Ok(if result.invalid.is_empty() && result.missing.is_empty() {
            AuditOutcome::Clean
        } else {
            AuditOutcome::Vulnerable
        })
    }
}

/// Whether the report holds an advisory at or above the configured
/// audit level.
fn audit_outcome(report: &AuditReport, audit_level: ConfigAuditLevel) -> AuditOutcome {
    if report.advisories
        .values()
        .any(|advisory| severity_number(advisory.severity) >= severity_number(audit_level))
    {
        AuditOutcome::Vulnerable
    } else {
        AuditOutcome::Clean
    }
}

/// Write one command result to stdout, appending the newline it lacks. Mirrors
/// pnpm's CLI, which terminates every command's output the same way and writes
/// nothing when a command produced none.
fn print_command_output(output: &str) {
    if output.is_empty() {
        return;
    }
    if output.ends_with('\n') {
        print!("{output}");
    } else {
        println!("{output}");
    }
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests;

mod advisories;

mod remediation;

impl AdvisoryFilterArgs {
    fn effective_level(&self, configured: Option<ConfigAuditLevel>) -> ConfigAuditLevel {
        self.audit_level
            .map(ConfigAuditLevel::from)
            .or(configured)
            .unwrap_or(ConfigAuditLevel::Low)
    }
}
