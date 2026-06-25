use crate::State;
use clap::{Args, ValueEnum};
use derive_more::{Display, Error};
use miette::{Diagnostic, IntoDiagnostic};
use node_semver::{Range, Version};
use owo_colors::{OwoColorize, Stream};
use pacquet_config::{AuditLevel as ConfigAuditLevel, Config};
use pacquet_lockfile::{
    EnvLockfile, ImporterDepVersion, Lockfile, PackageKey, PkgName, ResolvedDependencyMap,
    SnapshotDepRef, SnapshotEntry, SpecifierAndResolution,
};
use pacquet_network::{RetryOpts, send_with_retry};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Write,
    rc::Rc,
    time::Duration,
};

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

    /// Audit subcommand. `audit signatures` has not been ported yet.
    pub params: Vec<String>,
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
    pub async fn run(self, state: State) -> miette::Result<AuditOutcome> {
        if let Some(subcommand) = self.params.first() {
            return if subcommand == "signatures" {
                Err(miette::miette!(
                    "`pacquet audit signatures` is not supported yet; registry signature verification has not been ported to pacquet."
                ))
            } else {
                Err(AuditError::UnknownSubcommand {
                    subcommand: self.params.iter().take(2).cloned().collect::<Vec<_>>().join(" "),
                }
                .into())
            };
        }

        let lockfile = state
            .lockfile
            .get()
            .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
        let Some(lockfile) = lockfile else {
            return Err(AuditError::NoLockfile.into());
        };
        let lockfile_dir = state.manifest.path().parent().unwrap_or_else(|| state.manifest.path());
        let env_lockfile_dir = state.config.workspace_dir.as_deref().unwrap_or(lockfile_dir);
        let env_lockfile = EnvLockfile::read(env_lockfile_dir)
            .map_err(|err| miette::Report::new(err).wrap_err("load the env lockfile"))?;

        let include = self.dependency_options.include();
        let audit_level = self
            .audit_level
            .map(ConfigAuditLevel::from)
            .or(state.config.audit_level)
            .unwrap_or(ConfigAuditLevel::Low);
        let mut report = match audit(
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
                print!("{err}");
                let _ = std::io::stdout().flush();
                return Ok(AuditOutcome::Clean);
            }
            Err(err) => return Err(err.into()),
        };

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
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
enum AuditError {
    #[display("No pnpm-lock.yaml found: Cannot audit a project without a lockfile")]
    #[diagnostic(code(ERR_PNPM_AUDIT_NO_LOCKFILE))]
    NoLockfile,

    #[display("Unknown audit subcommand: {subcommand}")]
    #[diagnostic(code(ERR_PNPM_AUDIT_UNKNOWN_SUBCOMMAND))]
    UnknownSubcommand { subcommand: String },

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
    let request_url = audit_url.clone();
    let (_, response) = send_with_retry(http_client, &audit_url, retry_opts, |client| {
        let mut request =
            client.post(&request_url).header("content-type", "application/json").body(body.clone());
        if let Some(value) = &authorization {
            request = request.header("authorization", value);
        }
        request
    })
    .await
    .map_err(|source| AuditError::Network { url: audit_url.clone(), source })?;

    let status = response.status().as_u16();
    let raw_body = response
        .text()
        .await
        .map_err(|source| AuditError::Network { url: audit_url.clone(), source })?;
    match status {
        200 => {
            let parsed: serde_json::Value =
                serde_json::from_str(&raw_body).map_err(|source| AuditError::InvalidJson {
                    url: audit_url.clone(),
                    reason: source.to_string(),
                    body: truncate_chars(&raw_body, 500),
                })?;
            let bulk: BTreeMap<String, Vec<RawBulkAdvisory>> =
                serde_json::from_value(parsed.clone()).map_err(|_| AuditError::UnexpectedBody {
                    url: audit_url.clone(),
                    body: truncate_chars(&parsed.to_string(), 500),
                })?;
            Ok(bulk_response_to_audit_report(bulk, &audit_request, lockfile, env_lockfile, include))
        }
        404 => Err(AuditError::EndpointNotExists { url: audit_url }),
        _ => Err(AuditError::BadStatus { url: audit_url, status, body: raw_body }),
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

    AuditReport {
        advisories,
        metadata: AuditMetadata {
            vulnerabilities,
            dependencies: audit_request.dependencies,
            dev_dependencies: audit_request.dev_dependencies,
            optional_dependencies: audit_request.optional_dependencies,
            total_dependencies: audit_request.total_dependencies,
        },
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

#[allow(
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
        .map(|ghsa| normalize_ghsa_id(ghsa))
        .collect::<HashSet<_>>();
    if ignore_set.is_empty() {
        return AuditVulnerabilityCounts::default();
    }
    let mut ignored = AuditVulnerabilityCounts::default();
    report.advisories.retain(|_, advisory| {
        if !ignore_set.contains(&normalize_ghsa_id(&advisory.github_advisory_id)) {
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
    version.satisfies(&range)
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

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
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

#[cfg(test)]
mod tests;
