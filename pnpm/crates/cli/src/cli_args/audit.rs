use crate::State;
use clap::{Args, ValueEnum};
use derive_more::{Display, Error};
use dialoguer::MultiSelect;
use miette::{Diagnostic, IntoDiagnostic};
use node_semver::{Range, Version};
use owo_colors::{OwoColorize, Stream};
use pacquet_config::{AuditLevel as ConfigAuditLevel, Config};
use pacquet_lockfile::{
    EnvLockfile, ImporterDepVersion, Lockfile, PackageKey, PkgName, ResolvedDependencyMap,
    SnapshotDepRef, SnapshotEntry, SpecifierAndResolution, pick_registry_for_package,
};
use pacquet_network::{RetryOpts, send_with_retry};
use pacquet_package_manager::{ResolutionObserver, ResolvedPackageHint, Update};
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;
use pacquet_resolving_resolver_base::{
    PackageVersionGuard, PackageVersionGuardDecision, PackageVersionGuardFuture,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Write,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

mod fix;
mod paths;
mod render;
mod report;
mod request;
mod version_ranges;

pub(crate) use fix::{
    AuditFixObserver, VulnerabilityGuard, filter_advisories_for_fix, fix_override, fix_with_update,
    format_fix_with_update_output, ignore_vulnerabilities, interactive_select,
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
    AuditGraph, AuditIndexRequest, DepClass, DepKind, Edge, GraphImporter, Include,
    append_snapshot_edges, classify_graph, empty_snapshots, env_roots, importer_roots,
    lockfile_to_audit_request, root_included,
};
pub(crate) use version_ranges::{
    caret_range_for_patched, infer_patched_versions, satisfies_including_prerelease, satisfies_safe,
};

mod signatures;

const MAX_PATHS_COUNT: usize = 3;
const MAX_PATHS_PER_FINDING: usize = 100;

#[derive(Debug, Args)]
pub struct AuditArgs {
    /// Output audit report in JSON format.
    #[clap(long)]
    pub json: bool,

    /// Only print advisories with severity greater than or equal to this level.
    #[clap(long = "audit-level", value_enum)]
    pub audit_level: Option<AuditLevelArg>,

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

    /// Ignore a vulnerability by its GitHub advisory ID (e.g.
    /// GHSA-xxxx-xxxx-xxxx). May be repeated.
    #[clap(long, value_name = "GHSA")]
    pub ignore: Vec<String>,

    /// Ignore all vulnerabilities for which no fix exists.
    #[clap(long = "ignore-unfixable")]
    pub ignore_unfixable: bool,

    /// Show vulnerabilities and select which ones to fix interactively.
    #[clap(short = 'i', long)]
    pub interactive: bool,

    /// Audit subcommand. The only supported subcommand is `signatures`,
    /// which verifies registry signatures for the installed packages.
    pub params: Vec<String>,
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
    #[clap(long)]
    no_optional: bool,
}

impl AuditDependencyOptions {
    fn include(&self) -> Include {
        let mut dependencies = true;
        let mut dev_dependencies = true;
        let mut optional_dependencies = !self.no_optional;
        if self.prod {
            dev_dependencies = false;
        } else if self.dev {
            dependencies = false;
            optional_dependencies = false;
        }
        Include { dependencies, dev_dependencies, optional_dependencies }
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
        if let Some(subcommand) = self.params.first() {
            if subcommand == "signatures" {
                if self.params.len() > 1 {
                    return Err(AuditError::UnknownSubcommand {
                        subcommand: self
                            .params
                            .iter()
                            .take(2)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" "),
                    }
                    .into());
                }
                return self.run_signatures(state).await;
            }
            return Err(AuditError::UnknownSubcommand { subcommand: subcommand.clone() }.into());
        }

        let include = self.dependency_options.include();
        let audit_level = self
            .audit_level
            .map(ConfigAuditLevel::from)
            .or(state.config.audit_level)
            .unwrap_or(ConfigAuditLevel::Low);
        let fix_method = self.resolve_fix_method()?;

        let lockfile_dir = state.lockfile_dir().to_path_buf();
        // pnpm writes settings to `workspaceDir ?? rootProjectManifestDir`.
        let settings_dir =
            state.config.workspace_dir.clone().unwrap_or_else(|| lockfile_dir.clone());

        // Fetch the audit report, scoping the lockfile borrow so the later
        // `--fix update` path can re-borrow `state` mutably. Registry errors
        // are swallowed (per `--ignore-registry-errors`) the same way for
        // every path, matching pnpm's catch around the `audit()` call.
        let report = {
            let lockfile = state
                .lockfile
                .get()
                .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
            let Some(lockfile) = lockfile else {
                return Err(AuditError::NoLockfile.into());
            };
            let env_lockfile = EnvLockfile::read(&lockfile_dir)
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
                Ok(report) => report,
                Err(err) if self.ignore_registry_errors => {
                    eprintln!("{err}");
                    let _ = std::io::stderr().flush();
                    if self.json {
                        let report = empty_audit_report(lockfile, env_lockfile.as_ref(), include);
                        print!("{}", render_json_report(&report, audit_level)?);
                        let _ = std::io::stdout().flush();
                    }
                    return Ok(AuditOutcome::Clean);
                }
                Err(err) => return Err(err.into()),
            }
        };

        if let Some(fix_method) = fix_method {
            // Pre-filter by audit-level and ignored GHSAs so the interactive
            // prompt and both fix methods see the same advisory set the
            // override path's fixable filter would.
            let filtered = filter_advisories_for_fix(&report, audit_level, state.config);
            let filtered = if self.interactive {
                match interactive_select(filtered)? {
                    Some(selected) => selected,
                    // Cancelled or nothing selected — nothing to fix.
                    None => return Ok(AuditOutcome::Clean),
                }
            } else {
                filtered
            };
            return match fix_method {
                FixMethod::Override => {
                    let output = fix_override(&filtered, &settings_dir, state.config)?;
                    print!("{output}");
                    let _ = std::io::stdout().flush();
                    Ok(AuditOutcome::Clean)
                }
                FixMethod::Update => {
                    let (fixed, remaining, age_excludes) = fix_with_update::<Reporter>(
                        &mut state,
                        &filtered,
                        &lockfile_dir,
                        &settings_dir,
                    )
                    .await?;
                    let mut output = format_fix_with_update_output(&fixed, &remaining, &filtered);
                    if !age_excludes.is_empty() {
                        let note = format!(
                            "\n{} entries were added to minimumReleaseAgeExclude to allow installing the patched versions:\n{}\n",
                            age_excludes.len(),
                            age_excludes.join("\n"),
                        );
                        output.push_str(&note);
                    }
                    print!("{output}");
                    let _ = std::io::stdout().flush();
                    Ok(if remaining.is_empty() {
                        AuditOutcome::Clean
                    } else {
                        AuditOutcome::Vulnerable
                    })
                }
            };
        }

        if !self.ignore.is_empty() || self.ignore_unfixable {
            let output = ignore_vulnerabilities(
                &report,
                state.config,
                &settings_dir,
                &self.ignore,
                self.ignore_unfixable,
            )?;
            print!("{output}");
            let _ = std::io::stdout().flush();
            return Ok(AuditOutcome::Clean);
        }

        let mut report = report;
        let total_vulnerability_count = report.metadata.vulnerabilities.total();
        let ignored = filter_ignored_advisories(&mut report, state.config);

        let output = if self.json {
            render_json_report(&report, audit_level)?
        } else {
            render_text_report(&report, audit_level, total_vulnerability_count, &ignored)
        };
        print!("{output}");
        let _ = std::io::stdout().flush();

        Ok(
            if report
                .advisories
                .values()
                .any(|advisory| severity_number(advisory.severity) >= severity_number(audit_level))
            {
                AuditOutcome::Vulnerable
            } else {
                AuditOutcome::Clean
            },
        )
    }

    /// Resolve the `--fix` flag (and the `--interactive` implies-override
    /// rule) into a [`FixMethod`]. Mirrors pnpm's fix-method dispatch:
    /// `--fix`/`--fix override` → override, `--fix update` → update,
    /// `--interactive` without `--fix` → override, anything else → error.
    fn resolve_fix_method(&self) -> miette::Result<Option<FixMethod>> {
        match self.fix.as_deref() {
            Some("override") => Ok(Some(FixMethod::Override)),
            Some("update") => Ok(Some(FixMethod::Update)),
            Some(value) => Err(AuditError::InvalidFixOption { value: value.to_string() }.into()),
            None if self.interactive => Ok(Some(FixMethod::Override)),
            None => Ok(None),
        }
    }

    /// Handle `audit signatures`: verify registry signatures for every
    /// installed package and print the report. Exit code 1 (via
    /// [`AuditOutcome::Vulnerable`]) when any signature is missing or invalid.
    /// Ports pnpm's `auditSignatures`.
    async fn run_signatures(&self, state: State) -> miette::Result<AuditOutcome> {
        let include = self.dependency_options.include();
        let lockfile_dir = state.lockfile_dir().to_path_buf();

        let packages = {
            let lockfile = state
                .lockfile
                .get()
                .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
            let Some(lockfile) = lockfile else {
                return Err(AuditError::NoLockfile.into());
            };
            let env_lockfile = EnvLockfile::read(&lockfile_dir)
                .map_err(|err| miette::Report::new(err).wrap_err("load the env lockfile"))?;
            let audit_request = lockfile_to_audit_request(lockfile, env_lockfile.as_ref(), include);
            let registries: HashMap<String, String> =
                state.config.resolved_registries().into_iter().collect();
            audit_request
                .request
                .iter()
                .flat_map(|(name, versions)| {
                    let registry = pick_registry_for_package(&registries, name, None);
                    versions.iter().map(move |version| signatures::SignaturePackage {
                        name: name.clone(),
                        registry: registry.clone(),
                        version: version.clone(),
                    })
                })
                .collect::<Vec<_>>()
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
        print!("{output}");
        let _ = std::io::stdout().flush();

        Ok(if result.invalid.is_empty() && result.missing.is_empty() {
            AuditOutcome::Clean
        } else {
            AuditOutcome::Vulnerable
        })
    }
}

async fn audit(
    lockfile: &Lockfile,
    env_lockfile: Option<&EnvLockfile>,
    include: Include,
    config: &Config,
    http_client: &pacquet_network::ThrottledClient,
) -> Result<AuditReport, AuditError> {
    let audit_request = lockfile_to_audit_request(lockfile, env_lockfile, include);
    let registry = normalize_registry(&config.registry);
    let audit_url = format!("{registry}-/npm/v1/security/advisories/bulk");
    let body = serde_json::to_vec(&audit_request.request)
        .expect("audit request is a map of package names to version strings");
    let authorization = config.auth_headers.for_url(&registry);
    let retry_opts = retry_opts_from_config(config);
    let request_url = redact_url_userinfo(&audit_url);
    let display_audit_url = request_url.clone();
    let (_, response) = send_with_retry(http_client, &display_audit_url, retry_opts, |client| {
        let mut request =
            client.post(&request_url).header("content-type", "application/json").body(body.clone());
        if let Some(value) = &authorization {
            request = request.header("authorization", value);
        }
        request
    })
    .await
    .map_err(|source| AuditError::Network { url: display_audit_url.clone(), source })?;

    let status = response.status().as_u16();
    let raw_body = response
        .text()
        .await
        .map_err(|source| AuditError::Network { url: display_audit_url.clone(), source })?;
    match status {
        200 => {
            let parsed: serde_json::Value =
                serde_json::from_str(&raw_body).map_err(|source| AuditError::InvalidJson {
                    url: display_audit_url.clone(),
                    reason: source.to_string(),
                    body: sanitize_response_body(&raw_body),
                })?;
            let bulk: BTreeMap<String, Vec<RawBulkAdvisory>> =
                serde_json::from_value(parsed.clone()).map_err(|_| AuditError::UnexpectedBody {
                    url: display_audit_url.clone(),
                    body: sanitize_response_body(&parsed.to_string()),
                })?;
            Ok(bulk_response_to_audit_report(bulk, &audit_request, lockfile, env_lockfile, include))
        }
        404 => Err(AuditError::EndpointNotExists { url: display_audit_url }),
        _ => Err(AuditError::BadStatus {
            url: display_audit_url,
            status,
            body: sanitize_response_body(&raw_body),
        }),
    }
}

fn retry_opts_from_config(config: &Config) -> RetryOpts {
    RetryOpts {
        retries: config.fetch_retries,
        factor: config.fetch_retry_factor,
        min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
        max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
    }
}

impl<'a> AuditGraph<'a> {
    fn main(lockfile: &'a Lockfile) -> Self {
        let empty = empty_snapshots();
        let snapshots = lockfile.snapshots.as_ref().unwrap_or(empty);
        let importers = lockfile
            .importers
            .iter()
            .map(|(id, importer)| GraphImporter {
                path_segment: id.replace('/', "__"),
                roots: importer_roots(importer),
            })
            .collect();
        Self { importers, snapshots }
    }

    fn env(env_lockfile: &'a EnvLockfile) -> Self {
        let importer = env_lockfile.importers.get(EnvLockfile::ROOT_IMPORTER_KEY);
        let mut importers = Vec::new();
        if let Some(importer) = importer {
            let config_roots = env_roots(&importer.config_dependencies);
            if !config_roots.is_empty() {
                importers.push(GraphImporter {
                    path_segment: "configDependencies".to_string(),
                    roots: config_roots.into_iter().map(|edge| (DepKind::Prod, edge)).collect(),
                });
            }
            if let Some(package_manager_dependencies) = &importer.package_manager_dependencies {
                let package_manager_roots = env_roots(package_manager_dependencies);
                if !package_manager_roots.is_empty() {
                    importers.push(GraphImporter {
                        path_segment: "packageManagerDependencies".to_string(),
                        roots: package_manager_roots
                            .into_iter()
                            .map(|edge| (DepKind::Prod, edge))
                            .collect(),
                    });
                }
            }
        }
        Self { importers, snapshots: &env_lockfile.snapshots }
    }

    fn children(&self, key: &PackageKey, include_optional_edges: bool) -> Vec<Edge> {
        let Some(snapshot) = self.snapshots.get(key) else { return Vec::new() };
        let mut children = Vec::new();
        append_snapshot_edges(&mut children, snapshot.dependencies.as_ref());
        if include_optional_edges {
            append_snapshot_edges(&mut children, snapshot.optional_dependencies.as_ref());
        }
        children
    }
}

fn filter_ignored_advisories(
    report: &mut AuditReport,
    config: &Config,
) -> AuditVulnerabilityCounts {
    let ignore_set = config
        .audit_config
        .ignore_ghsas
        .iter()
        .filter_map(|ghsa| {
            let ghsa_id = normalize_ghsa_id(ghsa);
            (!ghsa_id.is_empty()).then_some(ghsa_id)
        })
        .collect::<HashSet<_>>();
    if ignore_set.is_empty() {
        return AuditVulnerabilityCounts::default();
    }
    let mut ignored = AuditVulnerabilityCounts::default();
    report.advisories.retain(|_, advisory| {
        let ghsa_id = normalize_ghsa_id(&advisory.github_advisory_id);
        if ghsa_id.is_empty() || !ignore_set.contains(&ghsa_id) {
            return true;
        }
        ignored.increment(advisory.severity);
        false
    });
    ignored
}

fn count_for_level(counts: &AuditVulnerabilityCounts, level: ConfigAuditLevel) -> usize {
    match level {
        ConfigAuditLevel::Info => counts.info,
        ConfigAuditLevel::Low => counts.low,
        ConfigAuditLevel::Moderate => counts.moderate,
        ConfigAuditLevel::High => counts.high,
        ConfigAuditLevel::Critical => counts.critical,
    }
}

fn parse_audit_level(value: &str) -> Option<ConfigAuditLevel> {
    match value {
        "info" => Some(ConfigAuditLevel::Info),
        "low" => Some(ConfigAuditLevel::Low),
        "moderate" => Some(ConfigAuditLevel::Moderate),
        "high" => Some(ConfigAuditLevel::High),
        "critical" => Some(ConfigAuditLevel::Critical),
        _ => None,
    }
}

fn severity_number(level: ConfigAuditLevel) -> u8 {
    match level {
        ConfigAuditLevel::Info => 0,
        ConfigAuditLevel::Low => 1,
        ConfigAuditLevel::Moderate => 2,
        ConfigAuditLevel::High => 3,
        ConfigAuditLevel::Critical => 4,
    }
}

fn severity_name(level: ConfigAuditLevel) -> &'static str {
    match level {
        ConfigAuditLevel::Info => "info",
        ConfigAuditLevel::Low => "low",
        ConfigAuditLevel::Moderate => "moderate",
        ConfigAuditLevel::High => "high",
        ConfigAuditLevel::Critical => "critical",
    }
}

impl PackageVersionGuard for VulnerabilityGuard {
    fn check<'a>(&'a self, name: &'a str, version: &'a str) -> PackageVersionGuardFuture<'a> {
        Box::pin(async move {
            let rejected = self.ranges_by_name.get(name).is_some_and(|ranges| {
                version.parse::<Version>().is_ok_and(|version| {
                    ranges.iter().any(|range| satisfies_including_prerelease(&version, range))
                })
            });
            Ok(if rejected {
                PackageVersionGuardDecision::Reject {
                    reason: format!("{name}@{version} is vulnerable"),
                }
            } else {
                PackageVersionGuardDecision::Allow
            })
        })
    }
}

impl ResolutionObserver for AuditFixObserver {
    fn on_resolved(&self, _hint: ResolvedPackageHint<'_>) {}

    fn package_version_guard(&self) -> Option<Arc<dyn PackageVersionGuard>> {
        Some(Arc::clone(&self.guard))
    }

    fn minimum_release_age_exclude_override(&self) -> Option<Vec<String>> {
        if self.age_excludes.is_empty() { None } else { Some(self.age_excludes.clone()) }
    }
}

#[cfg(test)]
mod tests;
