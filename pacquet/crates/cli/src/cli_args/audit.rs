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

        let lockfile_dir = state
            .manifest
            .path()
            .parent()
            .map_or_else(|| state.manifest.path().to_path_buf(), std::path::Path::to_path_buf);
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
            let env_lockfile_dir = state.config.workspace_dir.as_deref().unwrap_or(&lockfile_dir);
            let env_lockfile = EnvLockfile::read(env_lockfile_dir)
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
        let lockfile_dir = state
            .manifest
            .path()
            .parent()
            .map_or_else(|| state.manifest.path().to_path_buf(), std::path::Path::to_path_buf);

        let packages = {
            let lockfile = state
                .lockfile
                .get()
                .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
            let Some(lockfile) = lockfile else {
                return Err(AuditError::NoLockfile.into());
            };
            let env_lockfile_dir = state.config.workspace_dir.as_deref().unwrap_or(&lockfile_dir);
            let env_lockfile = EnvLockfile::read(env_lockfile_dir)
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

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
enum AuditError {
    #[display("No pnpm-lock.yaml found: Cannot audit a project without a lockfile")]
    #[diagnostic(code(ERR_PNPM_AUDIT_NO_LOCKFILE))]
    NoLockfile,

    #[display("No installed packages found to audit")]
    #[diagnostic(code(ERR_PNPM_AUDIT_NO_PACKAGES))]
    NoPackages,

    #[display("No pnpm-lock.yaml found after update: Cannot report fixed vulnerabilities")]
    #[diagnostic(code(ERR_PNPM_AUDIT_NO_LOCKFILE))]
    NoLockfileAfterUpdate,

    #[display("Unknown audit subcommand: {subcommand}")]
    #[diagnostic(code(ERR_PNPM_AUDIT_UNKNOWN_SUBCOMMAND))]
    UnknownSubcommand { subcommand: String },

    #[display("Invalid value for --fix: {value}. Should be one of \"override\" or \"update\"")]
    #[diagnostic(code(ERR_PNPM_INVALID_FIX_OPTION))]
    InvalidFixOption { value: String },

    #[display(
        "Cannot ignore advisory {id} ({module_name}): the registry did not provide a GHSA id or a resolvable url."
    )]
    #[diagnostic(code(ERR_PNPM_AUDIT_MISSING_GHSA))]
    MissingGhsa { id: u64, module_name: String },

    #[display("Failed to request the audit endpoint (at {url}): {source}")]
    #[diagnostic(code(ERR_PNPM_AUDIT_BAD_RESPONSE))]
    Network {
        url: String,
        #[error(source)]
        source: reqwest::Error,
    },

    #[display(
        "The audit endpoint (at {url}) returned invalid JSON: {reason}. Response body: {body}"
    )]
    #[diagnostic(code(ERR_PNPM_AUDIT_BAD_RESPONSE))]
    InvalidJson { url: String, reason: String, body: String },

    #[display(
        "The audit endpoint (at {url}) returned an unexpected body. Expected an object keyed by package name; got: {body}"
    )]
    #[diagnostic(code(ERR_PNPM_AUDIT_BAD_RESPONSE))]
    UnexpectedBody { url: String, body: String },

    #[display("The audit endpoint (at {url}) doesn't exist.")]
    #[diagnostic(
        code(ERR_PNPM_AUDIT_ENDPOINT_NOT_EXISTS),
        help(
            "This issue is probably because you are using a private npm registry and that endpoint doesn't have an implementation of audit."
        )
    )]
    EndpointNotExists { url: String },

    #[display("The audit endpoint (at {url}) responded with {status}: {body}")]
    #[diagnostic(code(ERR_PNPM_AUDIT_BAD_RESPONSE))]
    BadStatus { url: String, status: u16, body: String },
}

#[derive(Debug, Clone, Serialize)]
struct AuditReport {
    advisories: BTreeMap<String, AuditAdvisory>,
    metadata: AuditMetadata,
}

#[derive(Debug, Clone, Serialize)]
struct AuditMetadata {
    vulnerabilities: AuditVulnerabilityCounts,
    dependencies: usize,
    #[serde(rename = "devDependencies")]
    dev_dependencies: usize,
    #[serde(rename = "optionalDependencies")]
    optional_dependencies: usize,
    #[serde(rename = "totalDependencies")]
    total_dependencies: usize,
}

#[derive(Debug, Default, Clone, Serialize)]
struct AuditVulnerabilityCounts {
    info: usize,
    low: usize,
    moderate: usize,
    high: usize,
    critical: usize,
}

impl AuditVulnerabilityCounts {
    fn increment(&mut self, severity: ConfigAuditLevel) {
        match severity {
            ConfigAuditLevel::Info => self.info += 1,
            ConfigAuditLevel::Low => self.low += 1,
            ConfigAuditLevel::Moderate => self.moderate += 1,
            ConfigAuditLevel::High => self.high += 1,
            ConfigAuditLevel::Critical => self.critical += 1,
        }
    }

    fn total(&self) -> usize {
        self.info + self.low + self.moderate + self.high + self.critical
    }

    fn entries(&self) -> [(ConfigAuditLevel, usize); 5] {
        [
            (ConfigAuditLevel::Info, self.info),
            (ConfigAuditLevel::Low, self.low),
            (ConfigAuditLevel::Moderate, self.moderate),
            (ConfigAuditLevel::High, self.high),
            (ConfigAuditLevel::Critical, self.critical),
        ]
    }
}

#[derive(Debug, Clone, Serialize)]
struct AuditAdvisory {
    findings: Vec<AuditFinding>,
    id: u64,
    title: String,
    module_name: String,
    vulnerable_versions: String,
    patched_versions: Option<String>,
    severity: ConfigAuditLevel,
    cwe: String,
    github_advisory_id: String,
    url: String,
}

#[derive(Debug, Clone, Serialize)]
struct AuditFinding {
    version: String,
    paths: Vec<String>,
    dev: bool,
    optional: bool,
    bundled: bool,
}

#[derive(Debug, Deserialize)]
struct RawBulkAdvisory {
    id: Option<serde_json::Value>,
    url: Option<String>,
    title: Option<String>,
    severity: Option<String>,
    vulnerable_versions: String,
    cwe: Option<Cwe>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Cwe {
    One(String),
    Many(Vec<String>),
}

impl Cwe {
    fn into_string(self) -> String {
        match self {
            Cwe::One(value) => value,
            Cwe::Many(values) => values.join(", "),
        }
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

fn bulk_response_to_audit_report(
    bulk: BTreeMap<String, Vec<RawBulkAdvisory>>,
    audit_request: &AuditIndexRequest,
    lockfile: &Lockfile,
    env_lockfile: Option<&EnvLockfile>,
    include: Include,
) -> AuditReport {
    let vulnerable_names: HashSet<String> = bulk.keys().cloned().collect();
    let audit_path_index = if vulnerable_names.is_empty() {
        AuditPathIndex::default()
    } else {
        build_audit_path_index(lockfile, env_lockfile, &vulnerable_names, include)
    };
    let mut advisories = BTreeMap::new();
    let mut vulnerabilities = AuditVulnerabilityCounts::default();

    for (module_name, package_advisories) in bulk {
        let by_version = audit_path_index.get(&module_name);
        for raw in package_advisories {
            let Some(id) = raw.id.as_ref().and_then(serde_json::Value::as_u64) else {
                continue;
            };
            let Some(severity) = raw.severity.as_deref().and_then(parse_audit_level) else {
                continue;
            };
            let findings = build_findings(&raw.vulnerable_versions, by_version);
            if findings.is_empty() {
                continue;
            }
            let advisory = normalize_advisory(raw, id, module_name.clone(), severity, findings);
            vulnerabilities.increment(severity);
            advisories.insert(id.to_string(), advisory);
        }
    }

    AuditReport { advisories, metadata: audit_metadata(audit_request, vulnerabilities) }
}

fn empty_audit_report(
    lockfile: &Lockfile,
    env_lockfile: Option<&EnvLockfile>,
    include: Include,
) -> AuditReport {
    let audit_request = lockfile_to_audit_request(lockfile, env_lockfile, include);
    AuditReport {
        advisories: BTreeMap::new(),
        metadata: audit_metadata(&audit_request, AuditVulnerabilityCounts::default()),
    }
}

fn audit_metadata(
    audit_request: &AuditIndexRequest,
    vulnerabilities: AuditVulnerabilityCounts,
) -> AuditMetadata {
    AuditMetadata {
        vulnerabilities,
        dependencies: audit_request.dependencies,
        dev_dependencies: audit_request.dev_dependencies,
        optional_dependencies: audit_request.optional_dependencies,
        total_dependencies: audit_request.total_dependencies,
    }
}

fn build_findings(
    vulnerable_versions: &str,
    by_version: Option<&BTreeMap<String, PathInfo>>,
) -> Vec<AuditFinding> {
    let Some(by_version) = by_version else { return Vec::new() };
    by_version
        .iter()
        .filter(|(version, _)| satisfies_safe(version, vulnerable_versions))
        .map(|(version, info)| AuditFinding {
            version: version.clone(),
            paths: info.paths.clone(),
            dev: info.dev,
            optional: info.optional,
            bundled: false,
        })
        .collect()
}

fn normalize_advisory(
    raw: RawBulkAdvisory,
    id: u64,
    module_name: String,
    severity: ConfigAuditLevel,
    findings: Vec<AuditFinding>,
) -> AuditAdvisory {
    let url = raw.url.unwrap_or_default();
    AuditAdvisory {
        findings,
        id,
        title: raw.title.unwrap_or_default(),
        module_name,
        vulnerable_versions: raw.vulnerable_versions.clone(),
        patched_versions: infer_patched_versions(&raw.vulnerable_versions),
        severity,
        cwe: raw.cwe.map_or_else(String::new, Cwe::into_string),
        github_advisory_id: derive_github_advisory_id(&url),
        url,
    }
}

#[derive(Debug, Default)]
struct AuditIndexRequest {
    request: BTreeMap<String, Vec<String>>,
    total_dependencies: usize,
    dependencies: usize,
    dev_dependencies: usize,
    optional_dependencies: usize,
}

#[derive(Debug, Clone, Copy)]
struct Include {
    dependencies: bool,
    dev_dependencies: bool,
    optional_dependencies: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepKind {
    Prod,
    Dev,
    Optional,
}

#[derive(Debug, Clone)]
struct Edge {
    key: PackageKey,
}

#[derive(Debug)]
struct GraphImporter {
    path_segment: String,
    roots: Vec<(DepKind, Edge)>,
}

#[derive(Debug)]
struct AuditGraph<'a> {
    importers: Vec<GraphImporter>,
    snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
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

fn empty_snapshots() -> &'static HashMap<PackageKey, SnapshotEntry> {
    use std::sync::OnceLock;

    static EMPTY: OnceLock<HashMap<PackageKey, SnapshotEntry>> = OnceLock::new();
    EMPTY.get_or_init(HashMap::new)
}

fn importer_roots(importer: &pacquet_lockfile::ProjectSnapshot) -> Vec<(DepKind, Edge)> {
    let mut roots = Vec::new();
    append_importer_edges(&mut roots, DepKind::Prod, importer.dependencies.as_ref());
    append_importer_edges(&mut roots, DepKind::Dev, importer.dev_dependencies.as_ref());
    append_importer_edges(&mut roots, DepKind::Optional, importer.optional_dependencies.as_ref());
    roots
}

fn append_importer_edges(
    roots: &mut Vec<(DepKind, Edge)>,
    kind: DepKind,
    deps: Option<&ResolvedDependencyMap>,
) {
    let Some(deps) = deps else { return };
    for (name, spec) in deps {
        if let Some(key) = spec.version.resolved_key(name) {
            roots.push((kind, Edge { key }));
        }
    }
}

fn env_roots(deps: &BTreeMap<String, SpecifierAndResolution>) -> Vec<Edge> {
    deps.iter()
        .filter_map(|(name, spec)| {
            let name = name.parse::<PkgName>().ok()?;
            let version = spec.version.parse::<ImporterDepVersion>().ok()?;
            version.resolved_key(&name).map(|key| Edge { key })
        })
        .collect()
}

fn append_snapshot_edges(
    children: &mut Vec<Edge>,
    deps: Option<&HashMap<PkgName, SnapshotDepRef>>,
) {
    let Some(deps) = deps else { return };
    for (name, dep_ref) in deps {
        if let Some(key) = dep_ref.resolve(name) {
            children.push(Edge { key });
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DepClass {
    dev_only: bool,
    optional_only: bool,
}

fn classify_graph(graph: &AuditGraph<'_>, include: Include) -> HashMap<PackageKey, DepClass> {
    let dev_include = Include {
        dependencies: false,
        dev_dependencies: include.dev_dependencies,
        optional_dependencies: false,
    };
    let non_dev_include = Include {
        dependencies: include.dependencies,
        dev_dependencies: false,
        optional_dependencies: include.optional_dependencies,
    };
    let dev_reachable = walk_reachable(graph, dev_include, include.optional_dependencies);
    let non_dev_reachable = walk_reachable(graph, non_dev_include, include.optional_dependencies);
    let optional_only = collect_optional_only_keys(graph, include);
    dev_reachable
        .union(&non_dev_reachable)
        .cloned()
        .map(|key| {
            let class = DepClass {
                dev_only: dev_reachable.contains(&key) && !non_dev_reachable.contains(&key),
                optional_only: optional_only.contains(&key),
            };
            (key, class)
        })
        .collect()
}

fn collect_optional_only_keys(graph: &AuditGraph<'_>, include: Include) -> HashSet<PackageKey> {
    if !include.optional_dependencies {
        return HashSet::new();
    }
    let with_optional = walk_reachable(graph, include, true);
    let without_optional =
        walk_reachable(graph, Include { optional_dependencies: false, ..include }, false);
    with_optional.difference(&without_optional).cloned().collect()
}

fn walk_reachable(
    graph: &AuditGraph<'_>,
    include: Include,
    include_optional_edges: bool,
) -> HashSet<PackageKey> {
    let mut seen = HashSet::new();
    let mut stack =
        selected_root_edges(graph, include).map(|edge| edge.key.clone()).collect::<Vec<_>>();
    while let Some(key) = stack.pop() {
        if !seen.insert(key.clone()) {
            continue;
        }
        stack.extend(graph.children(&key, include_optional_edges).into_iter().map(|edge| edge.key));
    }
    seen
}

fn selected_root_edges<'a>(
    graph: &'a AuditGraph<'a>,
    include: Include,
) -> impl Iterator<Item = &'a Edge> {
    graph.importers.iter().flat_map(move |importer| {
        importer
            .roots
            .iter()
            .filter(move |(kind, _)| root_included(*kind, include))
            .map(|(_, edge)| edge)
    })
}

fn root_included(kind: DepKind, include: Include) -> bool {
    match kind {
        DepKind::Prod => include.dependencies,
        DepKind::Dev => include.dev_dependencies,
        DepKind::Optional => include.optional_dependencies,
    }
}

fn lockfile_to_audit_request(
    lockfile: &Lockfile,
    env_lockfile: Option<&EnvLockfile>,
    include: Include,
) -> AuditIndexRequest {
    let mut request = AuditRequestBuilder::default();
    let main = AuditGraph::main(lockfile);
    request.register_graph(&main, include);
    if let Some(env_lockfile) = env_lockfile {
        let env = AuditGraph::env(env_lockfile);
        request.register_graph(&env, include);
    }
    request.finish()
}

#[derive(Default)]
struct AuditRequestBuilder {
    request: BTreeMap<String, Vec<String>>,
    states_by_name: BTreeMap<String, BTreeMap<String, VersionState>>,
    total_dependencies: usize,
    dependencies: usize,
    dev_dependencies: usize,
    optional_dependencies: usize,
}

#[derive(Debug, Clone, Copy)]
struct VersionState {
    dev_only: bool,
    optional_only: bool,
}

impl AuditRequestBuilder {
    fn register_graph(&mut self, graph: &AuditGraph<'_>, include: Include) {
        let classes = classify_graph(graph, include);
        let mut seen = HashSet::new();
        let mut stack =
            selected_root_edges(graph, include).map(|edge| edge.key.clone()).collect::<Vec<_>>();
        while let Some(key) = stack.pop() {
            if !seen.insert(key.clone()) {
                continue;
            }
            let class = classes
                .get(&key)
                .copied()
                .unwrap_or(DepClass { dev_only: false, optional_only: false });
            self.register_occurrence(&key, class);
            stack.extend(
                graph
                    .children(&key, include.optional_dependencies)
                    .into_iter()
                    .map(|edge| edge.key),
            );
        }
    }

    fn register_occurrence(&mut self, key: &PackageKey, class: DepClass) {
        let Some(version) = package_version(key) else { return };
        let name = key.name.to_string();
        let version_states = self.states_by_name.entry(name.clone()).or_default();
        let Some(state) = version_states.get_mut(&version) else {
            version_states.insert(
                version.clone(),
                VersionState { dev_only: class.dev_only, optional_only: class.optional_only },
            );
            self.request.entry(name).or_default().push(version);
            self.total_dependencies += 1;
            if class.dev_only {
                self.dev_dependencies += 1;
            }
            if class.optional_only {
                self.optional_dependencies += 1;
            }
            if !class.dev_only && !class.optional_only {
                self.dependencies += 1;
            }
            return;
        };
        let was_production = !state.dev_only && !state.optional_only;
        if state.dev_only && !class.dev_only {
            state.dev_only = false;
            self.dev_dependencies -= 1;
        }
        if state.optional_only && !class.optional_only {
            state.optional_only = false;
            self.optional_dependencies -= 1;
        }
        if !was_production && !state.dev_only && !state.optional_only {
            self.dependencies += 1;
        }
    }

    fn finish(self) -> AuditIndexRequest {
        AuditIndexRequest {
            request: self.request,
            total_dependencies: self.total_dependencies,
            dependencies: self.dependencies,
            dev_dependencies: self.dev_dependencies,
            optional_dependencies: self.optional_dependencies,
        }
    }
}

#[derive(Debug, Default)]
struct PathInfo {
    paths: Vec<String>,
    dev: bool,
    optional: bool,
}

type AuditPathIndex = BTreeMap<String, BTreeMap<String, PathInfo>>;

fn build_audit_path_index(
    lockfile: &Lockfile,
    env_lockfile: Option<&EnvLockfile>,
    vulnerable_names: &HashSet<String>,
    include: Include,
) -> AuditPathIndex {
    let mut paths = AuditPathIndex::default();
    let main = AuditGraph::main(lockfile);
    walk_for_paths(&main, vulnerable_names, include, &mut paths);
    if let Some(env_lockfile) = env_lockfile {
        let env = AuditGraph::env(env_lockfile);
        walk_for_paths(&env, vulnerable_names, include, &mut paths);
    }
    paths
}

#[derive(Debug)]
struct TrailNode {
    name: String,
    parent: Option<Rc<TrailNode>>,
}

#[derive(Debug)]
struct PathFrame {
    key: PackageKey,
    trail: Rc<TrailNode>,
    children: Vec<Edge>,
    next: usize,
}

fn walk_for_paths(
    graph: &AuditGraph<'_>,
    vulnerable_names: &HashSet<String>,
    include: Include,
    paths: &mut AuditPathIndex,
) {
    let classes = classify_graph(graph, include);
    for importer in &graph.importers {
        let importer_trail =
            Rc::new(TrailNode { name: importer.path_segment.clone(), parent: None });
        let mut in_trail = HashSet::new();
        let mut stack: Vec<PathFrame> = Vec::new();
        for (_, root) in importer.roots.iter().filter(|(kind, _)| root_included(*kind, include)) {
            open_path_node(
                graph,
                root.key.clone(),
                Rc::clone(&importer_trail),
                vulnerable_names,
                include,
                &classes,
                paths,
                &mut in_trail,
                &mut stack,
            );
            while let Some(frame) = stack.last_mut() {
                if frame.next < frame.children.len() {
                    let child = frame.children[frame.next].key.clone();
                    let parent = Rc::clone(&frame.trail);
                    frame.next += 1;
                    open_path_node(
                        graph,
                        child,
                        parent,
                        vulnerable_names,
                        include,
                        &classes,
                        paths,
                        &mut in_trail,
                        &mut stack,
                    );
                } else {
                    let frame = stack.pop().expect("stack is non-empty");
                    in_trail.remove(&frame.key);
                }
            }
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Path traversal carries independent graph, filter, classification, and output state without a useful grouping abstraction"
)]
fn open_path_node(
    graph: &AuditGraph<'_>,
    key: PackageKey,
    parent_trail: Rc<TrailNode>,
    vulnerable_names: &HashSet<String>,
    include: Include,
    classes: &HashMap<PackageKey, DepClass>,
    paths: &mut AuditPathIndex,
    in_trail: &mut HashSet<PackageKey>,
    stack: &mut Vec<PathFrame>,
) {
    if in_trail.contains(&key) {
        return;
    }
    let name = key.name.to_string();
    let trail = Rc::new(TrailNode { name: name.clone(), parent: Some(parent_trail) });
    if vulnerable_names.contains(&name)
        && let Some(version) = package_version(&key)
    {
        let class = classes
            .get(&key)
            .copied()
            .unwrap_or(DepClass { dev_only: false, optional_only: false });
        record_path(
            paths,
            &name,
            &version,
            join_trail(&trail),
            class.dev_only,
            class.optional_only,
        );
    }
    let children = graph.children(&key, include.optional_dependencies);
    if children.is_empty() {
        return;
    }
    in_trail.insert(key.clone());
    stack.push(PathFrame { key, trail, children, next: 0 });
}

fn record_path(
    paths: &mut AuditPathIndex,
    name: &str,
    version: &str,
    joined: String,
    is_dev: bool,
    is_optional: bool,
) {
    let by_version = paths.entry(name.to_string()).or_default();
    let info = by_version.entry(version.to_string()).or_insert_with(|| PathInfo {
        paths: Vec::new(),
        dev: is_dev,
        optional: is_optional,
    });
    if !is_dev {
        info.dev = false;
    }
    if !is_optional {
        info.optional = false;
    }
    if info.paths.len() >= MAX_PATHS_PER_FINDING || info.paths.contains(&joined) {
        return;
    }
    info.paths.push(joined);
}

fn join_trail(node: &Rc<TrailNode>) -> String {
    let mut parts = Vec::new();
    let mut current = Some(Rc::clone(node));
    while let Some(node) = current.take() {
        parts.push(node.name.clone());
        current.clone_from(&node.parent);
    }
    parts.reverse();
    parts.join(">")
}

fn package_version(key: &PackageKey) -> Option<String> {
    key.suffix.version_semver().map(ToString::to_string)
}

fn render_json_report(
    report: &AuditReport,
    audit_level: ConfigAuditLevel,
) -> miette::Result<String> {
    let advisories = report
        .advisories
        .iter()
        .filter(|(_, advisory)| severity_number(advisory.severity) >= severity_number(audit_level))
        .map(|(id, advisory)| (id.clone(), advisory.clone()))
        .collect();
    serde_json::to_string_pretty(&AuditReport { advisories, metadata: report.metadata.clone() })
        .into_diagnostic()
}

fn render_text_report(
    report: &AuditReport,
    audit_level: ConfigAuditLevel,
    total_vulnerability_count: usize,
    ignored: &AuditVulnerabilityCounts,
) -> String {
    let mut advisories = report
        .advisories
        .values()
        .filter(|advisory| severity_number(advisory.severity) >= severity_number(audit_level))
        .collect::<Vec<_>>();
    advisories.sort_by(|left, right| {
        severity_number(right.severity).cmp(&severity_number(left.severity))
    });
    let mut output = String::new();
    for advisory in advisories {
        output.push_str(&render_advisory(advisory));
    }
    output.push_str(&report_summary(
        &report.metadata.vulnerabilities,
        total_vulnerability_count,
        ignored,
    ));
    output
}

fn render_advisory(advisory: &AuditAdvisory) -> String {
    use tabled::{builder::Builder, settings::Style};

    let paths = advisory
        .findings
        .iter()
        .flat_map(|finding| finding.paths.iter().cloned())
        .collect::<Vec<_>>();
    let rendered_paths = if paths.len() > MAX_PATHS_COUNT {
        paths[..MAX_PATHS_COUNT]
            .iter()
            .cloned()
            .chain(std::iter::once(format!(
                "... Found {} paths, run `pnpm why {}` for more information",
                paths.len(),
                advisory.module_name,
            )))
            .collect::<Vec<_>>()
            .join("\n\n")
    } else {
        paths.join("\n\n")
    };

    let mut builder = Builder::default();
    builder.push_record(vec![
        color_severity(advisory.severity, severity_name(advisory.severity)),
        bold(&advisory.title),
    ]);
    builder.push_record(vec!["Package".to_string(), advisory.module_name.clone()]);
    builder
        .push_record(vec!["Vulnerable versions".to_string(), advisory.vulnerable_versions.clone()]);
    builder.push_record(vec![
        "Patched versions".to_string(),
        advisory.patched_versions.clone().unwrap_or_else(|| "(unknown)".to_string()),
    ]);
    builder.push_record(vec!["Paths".to_string(), rendered_paths]);
    builder.push_record(vec!["More info".to_string(), advisory.url.clone()]);
    let mut table = builder.build();
    table.with(Style::modern());
    format!("{table}\n")
}

fn report_summary(
    vulnerabilities: &AuditVulnerabilityCounts,
    total_vulnerability_count: usize,
    ignored: &AuditVulnerabilityCounts,
) -> String {
    if total_vulnerability_count == 0 {
        return "No known vulnerabilities found\n".to_string();
    }
    let severities = vulnerabilities
        .entries()
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(level, count)| {
            let ignored_count = count_for_level(ignored, level);
            let label = if ignored_count > 0 {
                format!("{count} {} ({ignored_count} ignored)", severity_name(level))
            } else {
                format!("{count} {}", severity_name(level))
            };
            color_severity(level, &label)
        })
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        "{} vulnerabilities found\nSeverity: {severities}",
        red(&total_vulnerability_count.to_string()),
    )
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

fn satisfies_safe(version: &str, range: &str) -> bool {
    let Ok(version) = version.parse::<Version>() else { return false };
    let Ok(range) = range.parse::<Range>() else { return false };
    satisfies_including_prerelease(&version, &range)
}

fn satisfies_including_prerelease(version: &Version, range: &Range) -> bool {
    if version.satisfies(range) {
        return true;
    }
    range.to_string().split("||").any(|comparators| {
        comparators.split_whitespace().all(|comparator| comparator_matches(version, comparator))
    })
}

fn comparator_matches(version: &Version, comparator: &str) -> bool {
    if comparator == "*" {
        return true;
    }
    let (operator, wanted) = comparator_operator_and_version(comparator);
    let Ok(wanted) = wanted.parse::<Version>() else { return false };
    match operator {
        ">" => version > &wanted,
        ">=" => version >= &wanted,
        "<" => version < &wanted,
        "<=" => version <= &wanted,
        _ => version == &wanted,
    }
}

fn comparator_operator_and_version(comparator: &str) -> (&str, &str) {
    for operator in [">=", "<=", ">", "<"] {
        if let Some(version) = comparator.strip_prefix(operator) {
            return (operator, version);
        }
    }
    ("", comparator)
}

fn infer_patched_versions(vulnerable_range: &str) -> Option<String> {
    let (operator, version) = last_upper_bound(vulnerable_range.trim())?;
    let version = version.parse::<Version>().ok()?;
    match operator {
        "<" => Some(format!(">={version}")),
        "<=" => {
            let next = Version {
                major: version.major,
                minor: version.minor,
                patch: version.patch + 1,
                pre_release: Vec::new(),
                build: Vec::new(),
            };
            Some(format!(">={next}"))
        }
        _ => None,
    }
}

fn last_upper_bound(input: &str) -> Option<(&str, &str)> {
    let mut parts = input.split_whitespace().collect::<Vec<_>>();
    let last = parts.pop()?;
    if let Some(version) = last.strip_prefix("<=") {
        return Some(("<=", version.trim()));
    }
    if let Some(version) = last.strip_prefix('<') {
        return Some(("<", version.trim()));
    }
    let operator = parts.pop()?;
    matches!(operator, "<" | "<=").then_some((operator, last))
}

fn derive_github_advisory_id(url: &str) -> String {
    let Some(idx) = url.to_ascii_uppercase().find("GHSA-") else {
        return String::new();
    };
    let id = url[idx..]
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
        .next()
        .unwrap_or_default();
    normalize_ghsa_id(id)
}

fn normalize_ghsa_id(ghsa_id: &str) -> String {
    let trimmed = ghsa_id.trim();
    let Some(dash) = trimmed.find('-') else {
        return trimmed.to_ascii_uppercase();
    };
    format!("{}{}", trimmed[..dash].to_ascii_uppercase(), trimmed[dash..].to_ascii_lowercase())
}

fn normalize_registry(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}

fn redact_url_userinfo(url: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return url.to_string();
    };
    if parsed.username().is_empty() && parsed.password().is_none() {
        return url.to_string();
    }
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.to_string()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn sanitize_response_body(value: &str) -> String {
    sanitize_control_chars(&truncate_chars(value, 500))
}

fn sanitize_control_chars(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() {
            output.extend(ch.escape_unicode());
        } else {
            output.push(ch);
        }
    }
    output
}

fn bold(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

fn red(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.red()).to_string()
}

fn color_severity(level: ConfigAuditLevel, text: &str) -> String {
    match level {
        ConfigAuditLevel::Info => {
            text.if_supports_color(Stream::Stdout, |t| t.dimmed()).to_string()
        }
        ConfigAuditLevel::Low => text.if_supports_color(Stream::Stdout, |t| t.bold()).to_string(),
        ConfigAuditLevel::Moderate => {
            let style = owo_colors::Style::new().yellow().bold();
            text.if_supports_color(Stream::Stdout, |t| t.style(style)).to_string()
        }
        ConfigAuditLevel::High | ConfigAuditLevel::Critical => {
            let style = owo_colors::Style::new().red().bold();
            text.if_supports_color(Stream::Stdout, |t| t.style(style)).to_string()
        }
    }
}

/// Filter `report`'s advisories down to the set both fix methods and the
/// interactive prompt operate on: severity at or above `audit_level` and
/// not suppressed by `auditConfig.ignoreGhsas`. Mirrors pnpm's
/// `filterAdvisoriesForFix`.
fn filter_advisories_for_fix(
    report: &AuditReport,
    audit_level: ConfigAuditLevel,
    config: &Config,
) -> BTreeMap<String, AuditAdvisory> {
    let ignore_set = config
        .audit_config
        .ignore_ghsas
        .iter()
        .filter_map(|ghsa| {
            let ghsa = normalize_ghsa_id(ghsa);
            (!ghsa.is_empty()).then_some(ghsa)
        })
        .collect::<HashSet<_>>();
    report
        .advisories
        .iter()
        .filter(|(_, advisory)| severity_number(advisory.severity) >= severity_number(audit_level))
        .filter(|(_, advisory)| {
            let ghsa = normalize_ghsa_id(&advisory.github_advisory_id);
            ghsa.is_empty() || !ignore_set.contains(&ghsa)
        })
        .map(|(id, advisory)| (id.clone(), advisory.clone()))
        .collect()
}

/// The minimum patched version with a caret, mirroring pnpm's
/// `caretRangeForPatched`: `^X.Y.Z` keeps the resolver within the same major
/// the user pinned to, where a bare `>=X.Y.Z` could silently promote a dep to
/// a later breaking major. `patched` is always pacquet's inferred `>=V` form,
/// so its minimum is the version after `>=`.
fn caret_range_for_patched(patched: &str) -> String {
    patched
        .strip_prefix(">=")
        .and_then(|version| version.trim().parse::<Version>().ok())
        .map_or_else(|| patched.to_string(), |version| format!("^{version}"))
}

/// Build the `name@vulnerable_versions → ^patched` override map from the
/// fixable advisories (those with an inferred patched range). Keyed by a
/// `BTreeMap` so the output is sorted, mirroring pnpm's `sortDirectKeys`.
fn create_overrides(advisories: &BTreeMap<String, AuditAdvisory>) -> BTreeMap<String, String> {
    let mut overrides = BTreeMap::new();
    for advisory in advisories.values() {
        let Some(patched) = advisory.patched_versions.as_deref() else { continue };
        let key = format!("{}@{}", advisory.module_name, advisory.vulnerable_versions);
        overrides.insert(key, caret_range_for_patched(patched));
    }
    overrides
}

/// Write the override-method fixes to `pnpm-workspace.yaml` and return the
/// user-facing summary. Mirrors the override branch of pnpm's audit handler.
fn fix_override(
    advisories: &BTreeMap<String, AuditAdvisory>,
    settings_dir: &std::path::Path,
    config: &Config,
) -> miette::Result<String> {
    let overrides = create_overrides(advisories);
    if overrides.is_empty() {
        return Ok("No fixes were made".to_string());
    }
    let entries = overrides.iter().map(|(key, value)| (key.as_str(), value.as_str()));
    pacquet_workspace_manifest_writer::set_overrides(settings_dir, entries).map_err(|err| {
        miette::Report::new(err).wrap_err("write overrides to pnpm-workspace.yaml")
    })?;
    let json = serde_json::to_string_pretty(&overrides).into_diagnostic()?;
    let mut output = format!(
        "{} overrides were added to pnpm-workspace.yaml to fix vulnerabilities.\nRun \"pnpm install\" to apply the fixes.\n\nThe added overrides:\n{json}",
        overrides.len(),
    );
    if config.resolved_minimum_release_age().is_some() {
        let added = minimum_release_age_excludes(advisories)?;
        if !added.is_empty() {
            write_age_excludes(settings_dir, config, &added)?;
            let note = format!(
                "\n\n{} entries were added to minimumReleaseAgeExclude to allow installing the patched versions:\n{}",
                added.len(),
                added.join("\n"),
            );
            output.push_str(&note);
        }
    }
    Ok(output)
}

/// Patched minimum versions of the fixable advisories, as
/// `name@minVersion` specs merged per package. Ports pnpm's
/// `createMinimumReleaseAgeExcludes`: these are appended to
/// `minimumReleaseAgeExclude` so a `minimumReleaseAge` cutoff doesn't block
/// installing a freshly-published patched version.
fn minimum_release_age_excludes(
    advisories: &BTreeMap<String, AuditAdvisory>,
) -> miette::Result<Vec<String>> {
    let specs: Vec<String> = advisories
        .values()
        .filter_map(|advisory| {
            let patched = advisory.patched_versions.as_deref()?;
            let min = patched
                .strip_prefix(">=")
                .and_then(|version| version.trim().parse::<Version>().ok())?;
            Some(format!("{}@{min}", advisory.module_name))
        })
        .collect();
    pacquet_config::version_policy::merge_package_version_specs(&specs).map_err(miette::Report::new)
}

/// Merge `added` into the existing `minimumReleaseAgeExclude` and persist the
/// canonical result. Mirrors pnpm's `writeSettings` re-merge of
/// `[...existing, ...added]`.
fn write_age_excludes(
    settings_dir: &std::path::Path,
    config: &Config,
    added: &[String],
) -> miette::Result<()> {
    let mut all = config.minimum_release_age_exclude.clone().unwrap_or_default();
    all.extend(added.iter().cloned());
    let merged = pacquet_config::version_policy::merge_package_version_specs(&all)
        .map_err(miette::Report::new)?;
    pacquet_workspace_manifest_writer::set_minimum_release_age_excludes(settings_dir, &merged)
        .map_err(|err| {
            miette::Report::new(err)
                .wrap_err("write minimumReleaseAgeExclude to pnpm-workspace.yaml")
        })
}

/// Merge the requested ignores into `auditConfig.ignoreGhsas` and persist
/// them, returning the user-facing summary. Mirrors pnpm's `ignore()`:
/// `--ignore-unfixable` adds every advisory with no inferable fix (erroring
/// when one lacks a GHSA id); otherwise the `--ignore` GHSA ids are added.
fn ignore_vulnerabilities(
    report: &AuditReport,
    config: &Config,
    settings_dir: &std::path::Path,
    ignore: &[String],
    ignore_unfixable: bool,
) -> miette::Result<String> {
    let mut ordered: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for ghsa in &config.audit_config.ignore_ghsas {
        let ghsa = normalize_ghsa_id(ghsa);
        if !ghsa.is_empty() && seen.insert(ghsa.clone()) {
            ordered.push(ghsa);
        }
    }

    let mut new_ignores: Vec<String> = Vec::new();
    let mut add = |ghsa: String, ordered: &mut Vec<String>, new_ignores: &mut Vec<String>| {
        if seen.insert(ghsa.clone()) {
            ordered.push(ghsa.clone());
            new_ignores.push(ghsa);
        }
    };

    if ignore_unfixable {
        for advisory in
            report.advisories.values().filter(|advisory| advisory.patched_versions.is_none())
        {
            if advisory.github_advisory_id.is_empty() {
                return Err(AuditError::MissingGhsa {
                    id: advisory.id,
                    module_name: advisory.module_name.clone(),
                }
                .into());
            }
            add(normalize_ghsa_id(&advisory.github_advisory_id), &mut ordered, &mut new_ignores);
        }
    } else {
        for ghsa in ignore {
            add(normalize_ghsa_id(ghsa), &mut ordered, &mut new_ignores);
        }
    }

    pacquet_workspace_manifest_writer::set_audit_ignore_ghsas(settings_dir, &ordered).map_err(
        |err| {
            miette::Report::new(err)
                .wrap_err("write auditConfig.ignoreGhsas to pnpm-workspace.yaml")
        },
    )?;

    if new_ignores.is_empty() {
        Ok("No new vulnerabilities were ignored".to_string())
    } else {
        Ok(format!(
            "{} new vulnerabilities were ignored:\n{}",
            new_ignores.len(),
            new_ignores.join("\n"),
        ))
    }
}

/// Prompt the user to choose which fixable vulnerabilities to fix and return
/// the chosen subset. `Ok(None)` means "nothing to do" — the prompt was
/// cancelled or no row was selected; an `Err` means the prompt itself failed
/// (e.g. a non-TTY/CI stdin) and must surface rather than read as a clean
/// audit. Ports pnpm's `interactiveAuditFix`, with the flat `dialoguer`
/// multi-select pacquet's `update --interactive` also uses in place of pnpm's
/// severity-grouped table.
fn interactive_select(
    advisories: BTreeMap<String, AuditAdvisory>,
) -> miette::Result<Option<BTreeMap<String, AuditAdvisory>>> {
    let mut fixable: Vec<&AuditAdvisory> =
        advisories.values().filter(|advisory| advisory.patched_versions.is_some()).collect();
    fixable.sort_by_key(|advisory| std::cmp::Reverse(severity_number(advisory.severity)));

    let mut keys: Vec<String> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for advisory in fixable {
        let key = format!("{}@{}", advisory.module_name, advisory.vulnerable_versions);
        if !seen.insert(key.clone()) {
            continue;
        }
        let patched =
            advisory.patched_versions.as_deref().map(caret_range_for_patched).unwrap_or_default();
        labels.push(format!(
            "[{}] {} {} ❯ {} {}",
            severity_name(advisory.severity),
            advisory.module_name,
            advisory.vulnerable_versions,
            patched,
            advisory.github_advisory_id,
        ));
        keys.push(key);
    }

    // Nothing fixable: mirror pnpm returning the report unchanged (the fix
    // method then makes no changes).
    if keys.is_empty() {
        return Ok(Some(advisories));
    }

    // `interact_opt` distinguishes an explicit cancel (Esc/Ctrl-C → `Ok(None)`)
    // from a prompt failure (`Err`). A failure must not be swallowed into a
    // clean audit, so it propagates; a cancel or empty selection is "nothing
    // to do".
    let selected = MultiSelect::new()
        .with_prompt("Choose which vulnerabilities to fix (space to select, enter to confirm)")
        .items(&labels)
        .interact_opt()
        .into_diagnostic()
        .map_err(|err| err.wrap_err("interactive audit selection failed"))?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    if selected.is_empty() {
        return Ok(None);
    }
    let chosen: HashSet<&String> = selected.iter().map(|&index| &keys[index]).collect();
    Ok(Some(
        advisories
            .into_iter()
            .filter(|(_, advisory)| {
                chosen
                    .contains(&format!("{}@{}", advisory.module_name, advisory.vulnerable_versions))
            })
            .collect(),
    ))
}

/// The advisories of a `--fix update` run, partitioned by how the update can
/// act on each: ones with a concrete vulnerable range (guarded against and
/// re-checked after update), ones whose range is `>=0.0.0` / `*` (no version
/// could ever be safe), and ones whose range the registry sent in a form we
/// can't parse.
struct UpdateClassification {
    vulnerabilities: HashMap<String, Vec<(u64, Range)>>,
    unfixable: HashMap<String, Vec<u64>>,
    /// Advisory ids whose `vulnerable_versions` failed to parse. The registry
    /// is untrusted, so a malformed range must not silently drop the advisory
    /// — it is counted as remaining rather than read as a clean exit.
    unparsable: Vec<u64>,
}

fn classify_for_update(advisories: &BTreeMap<String, AuditAdvisory>) -> UpdateClassification {
    let mut vulnerabilities: HashMap<String, Vec<(u64, Range)>> = HashMap::new();
    let mut unfixable: HashMap<String, Vec<u64>> = HashMap::new();
    let mut unparsable: Vec<u64> = Vec::new();
    for advisory in advisories.values() {
        // The registry is untrusted: trim both the package name and the range
        // so a whitespace-padded name still keys the guard and the
        // installed-name comparison against the (clean) lockfile, and so the
        // sentinel check matches like the rest of the audit range logic
        // (e.g. `infer_patched_versions`).
        let name = advisory.module_name.trim();
        let range = advisory.vulnerable_versions.trim();
        if range == ">=0.0.0" || range == "*" {
            unfixable.entry(name.to_string()).or_default().push(advisory.id);
            continue;
        }
        let Ok(range) = range.parse::<Range>() else {
            unparsable.push(advisory.id);
            continue;
        };
        vulnerabilities.entry(name.to_string()).or_default().push((advisory.id, range));
    }
    UpdateClassification { vulnerabilities, unfixable, unparsable }
}

/// Re-resolve the lockfile to non-vulnerable versions and report which
/// advisories that fixed. Ports pnpm's `fixWithUpdate`: a resolver-time
/// [`PackageVersionGuard`] rejects vulnerable versions so the picker falls
/// back to a safe one, then the post-update lockfile decides fixed vs.
/// remaining. Advisories whose vulnerable range is `>=0.0.0` / `*` cannot be
/// fixed by an update and are remaining iff the package is still installed.
async fn fix_with_update<Reporter: self::Reporter + 'static>(
    state: &mut State,
    advisories: &BTreeMap<String, AuditAdvisory>,
    lockfile_dir: &std::path::Path,
    settings_dir: &std::path::Path,
) -> miette::Result<(Vec<u64>, Vec<u64>, Vec<String>)> {
    let UpdateClassification { vulnerabilities, unfixable, unparsable } =
        classify_for_update(advisories);

    // When `minimumReleaseAge` is set, the patched versions are likely
    // fresher than the cutoff; record them as exclusions (persisted to config
    // and injected into this resolve) so the picker may install them.
    let age_excludes = if state.config.resolved_minimum_release_age().is_some() {
        let added = minimum_release_age_excludes(advisories)?;
        if !added.is_empty() {
            write_age_excludes(settings_dir, state.config, &added)?;
        }
        added
    } else {
        Vec::new()
    };

    let guard_ranges: HashMap<String, Vec<Range>> = vulnerabilities
        .iter()
        .map(|(name, entries)| {
            (name.clone(), entries.iter().map(|(_, range)| range.clone()).collect())
        })
        .collect();
    let observer: Arc<dyn ResolutionObserver> = Arc::new(AuditFixObserver {
        guard: Arc::new(VulnerabilityGuard { ranges_by_name: guard_ranges }),
        age_excludes: age_excludes.clone(),
    });

    {
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            state;
        let lockfile =
            lockfile.get().map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
        let lockfile_path = manifest.path().parent().map(|parent| parent.join(Lockfile::FILE_NAME));
        Update {
            tarball_mem_cache: Arc::clone(tarball_mem_cache),
            resolved_packages,
            http_client,
            http_client_arc: Arc::clone(http_client),
            config,
            manifest,
            lockfile,
            lockfile_path: lockfile_path.as_deref(),
            packages: &[],
            latest: false,
            save_exact: false,
            save: true,
            include_direct: vec![
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ],
            depth: usize::MAX,
            supported_architectures: config.supported_architectures.clone(),
            lockfile_only: false,
            resolution_observer: Some(observer),
        }
        .run::<Reporter>()
        .await
        .map_err(|err| {
            miette::Report::new(err).wrap_err("update dependencies to fix vulnerabilities")
        })?;
    }

    // A missing lockfile here means the update couldn't be verified; mirror
    // pnpm's `fixWithUpdate`, which errors rather than reporting everything
    // fixed against an empty installed set.
    let Some(updated) = Lockfile::load_wanted_from_dir(lockfile_dir)
        .map_err(|err| miette::Report::new(err).wrap_err("re-read the lockfile after update"))?
    else {
        return Err(AuditError::NoLockfileAfterUpdate.into());
    };
    // Every still-installed package name, regardless of how its lockfile key
    // is shaped, plus the subset whose key parses as semver (the only ones a
    // vulnerable range can be checked against).
    let mut installed_names: HashSet<String> = HashSet::new();
    let mut installed_versions: HashMap<String, Vec<Version>> = HashMap::new();
    if let Some(snapshots) = updated.snapshots.as_ref() {
        for key in snapshots.keys() {
            let name = key.name.to_string();
            installed_names.insert(name.clone());
            if let Some(version) = key.suffix.version_semver() {
                installed_versions.entry(name).or_default().push(version.clone());
            }
        }
    }

    let installed = InstalledPackages { names: installed_names, versions: installed_versions };
    let (fixed, remaining) =
        report_fixed_remaining(&vulnerabilities, &unfixable, &unparsable, &installed);

    Ok((fixed, remaining, age_excludes))
}

/// The packages present in the post-update lockfile: every name (regardless of
/// lockfile-key shape) plus, for each, the versions whose key parsed as semver.
struct InstalledPackages {
    names: HashSet<String>,
    versions: HashMap<String, Vec<Version>>,
}

/// Decide which advisories an update fixed. An advisory is **fixed** only when
/// its package is gone, or every installed semver version of it escapes the
/// vulnerable range. It stays **remaining** when a vulnerable version is still
/// installed, when the package survives only under non-semver keys (`file:` /
/// git / tarball — unverifiable), when its range is `>=0.0.0` / `*` and the
/// package is still installed, or when its range was unparsable. The
/// conservative bias keeps `audit --fix update` from reporting a clean state
/// it can't prove.
fn report_fixed_remaining(
    vulnerabilities: &HashMap<String, Vec<(u64, Range)>>,
    unfixable: &HashMap<String, Vec<u64>>,
    unparsable: &[u64],
    installed: &InstalledPackages,
) -> (Vec<u64>, Vec<u64>) {
    let mut fixed: Vec<u64> = Vec::new();
    let mut remaining: Vec<u64> = Vec::new();
    for (name, entries) in vulnerabilities {
        if !installed.names.contains(name) {
            fixed.extend(entries.iter().map(|(id, _)| *id));
            continue;
        }
        match installed.versions.get(name) {
            // Still installed, but only via non-semver keys (file:/git/tarball);
            // the range can't be evaluated, so don't claim it's fixed.
            None => remaining.extend(entries.iter().map(|(id, _)| *id)),
            Some(versions) => {
                for (id, range) in entries {
                    let still_vulnerable = versions
                        .iter()
                        .any(|version| satisfies_including_prerelease(version, range));
                    if still_vulnerable {
                        remaining.push(*id);
                    } else {
                        fixed.push(*id);
                    }
                }
            }
        }
    }
    for (name, ids) in unfixable {
        if installed.names.contains(name) {
            remaining.extend(ids.iter().copied());
        } else {
            fixed.extend(ids.iter().copied());
        }
    }
    // Advisories with an unparsable vulnerable range can't be proven fixed.
    remaining.extend(unparsable.iter().copied());

    (fixed, remaining)
}

/// Render the `--fix update` summary, mirroring pnpm's
/// `formatFixWithUpdateOutput`: a one-line count, then the fixed and
/// remaining advisories listed severity-high-to-low.
fn format_fix_with_update_output(
    fixed: &[u64],
    remaining: &[u64],
    advisories: &BTreeMap<String, AuditAdvisory>,
) -> String {
    let by_id = |id: u64| advisories.get(&id.to_string());
    let sort_by_severity = |ids: &[u64]| -> Vec<u64> {
        let mut ids = ids.to_vec();
        ids.sort_by_key(|id| {
            std::cmp::Reverse(
                by_id(*id).map_or(-1, |advisory| i32::from(severity_number(advisory.severity))),
            )
        });
        ids
    };
    let fixed = sort_by_severity(fixed);
    let remaining = sort_by_severity(remaining);

    let fixed_word =
        if fixed.len() == 1 { "vulnerability was fixed" } else { "vulnerabilities were fixed" };
    let remaining_word =
        if remaining.len() == 1 { "vulnerability remains" } else { "vulnerabilities remain" };

    let mut lines = vec![format!(
        "{} {fixed_word}, {} {remaining_word}.",
        green(&fixed.len().to_string()),
        red(&remaining.len().to_string()),
    )];

    let summarize = |is_fixed: bool, id: u64| -> String {
        match by_id(id) {
            Some(advisory) => {
                let (severity, title) = if is_fixed {
                    (green(severity_name(advisory.severity)), green(&advisory.title))
                } else {
                    (
                        color_severity(advisory.severity, severity_name(advisory.severity)),
                        color_severity(advisory.severity, &advisory.title),
                    )
                };
                format!(r#"- ({severity}) "{title}" {}"#, blue(&advisory.module_name))
            }
            None => format!("- Advisory with ID {id} (details not found in the audit report)"),
        }
    };

    if !fixed.is_empty() {
        lines.push("\nThe fixed vulnerabilities are:".to_string());
        lines.extend(fixed.iter().map(|id| summarize(true, *id)));
    }
    if !remaining.is_empty() {
        lines.push("\nThe remaining vulnerabilities are:".to_string());
        lines.extend(remaining.iter().map(|id| summarize(false, *id)));
    }
    lines.push(String::new());
    lines.join("\n")
}

/// Resolver-time guard that rejects concrete versions matching any known
/// vulnerable range for a package, so `audit --fix update` re-picks a safe
/// version. Ports the `isVulnerable` half of pnpm's
/// `PackageVulnerabilityAudit`.
#[derive(Debug)]
struct VulnerabilityGuard {
    ranges_by_name: HashMap<String, Vec<Range>>,
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

/// Carries the [`VulnerabilityGuard`] and the patched-version
/// `minimumReleaseAgeExclude` entries into the install's resolve pass. The
/// resolution stream itself is not observed (`on_resolved` is a no-op); the
/// observer exists only as the seam the resolver reads both from.
struct AuditFixObserver {
    guard: Arc<dyn PackageVersionGuard>,
    age_excludes: Vec<String>,
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

fn green(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.green()).to_string()
}

fn blue(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.blue()).to_string()
}

#[cfg(test)]
mod tests;
