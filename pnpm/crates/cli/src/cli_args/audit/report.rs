//! The registry audit response, and the report shape derived from it.

use super::{
    AuditIndexRequest, AuditPathIndex, BTreeMap, ConfigAuditLevel, Deserialize, Diagnostic,
    Display, EnvLockfile, Error, HashSet, Include, Lockfile, PathInfo, Serialize,
    build_audit_path_index, infer_patched_versions, lockfile_to_audit_request, parse_audit_level,
    satisfies_safe,
};

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub(crate) enum AuditError {
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
pub(crate) struct AuditReport {
    pub(crate) advisories: BTreeMap<String, AuditAdvisory>,
    pub(crate) metadata: AuditMetadata,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AuditMetadata {
    pub(crate) vulnerabilities: AuditVulnerabilityCounts,
    pub(crate) dependencies: usize,
    #[serde(rename = "devDependencies")]
    pub(crate) dev_dependencies: usize,
    #[serde(rename = "optionalDependencies")]
    pub(crate) optional_dependencies: usize,
    #[serde(rename = "totalDependencies")]
    pub(crate) total_dependencies: usize,
}

#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct AuditVulnerabilityCounts {
    pub(crate) info: usize,
    pub(crate) low: usize,
    pub(crate) moderate: usize,
    pub(crate) high: usize,
    pub(crate) critical: usize,
}

impl AuditVulnerabilityCounts {
    pub(crate) fn increment(&mut self, severity: ConfigAuditLevel) {
        match severity {
            ConfigAuditLevel::Info => self.info += 1,
            ConfigAuditLevel::Low => self.low += 1,
            ConfigAuditLevel::Moderate => self.moderate += 1,
            ConfigAuditLevel::High => self.high += 1,
            ConfigAuditLevel::Critical => self.critical += 1,
        }
    }

    pub(crate) fn total(&self) -> usize {
        self.info + self.low + self.moderate + self.high + self.critical
    }

    pub(crate) fn entries(&self) -> [(ConfigAuditLevel, usize); 5] {
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
pub(crate) struct AuditAdvisory {
    pub(crate) findings: Vec<AuditFinding>,
    pub(crate) id: u64,
    pub(crate) title: String,
    pub(crate) module_name: String,
    pub(crate) vulnerable_versions: String,
    pub(crate) patched_versions: Option<String>,
    pub(crate) severity: ConfigAuditLevel,
    pub(crate) cwe: String,
    pub(crate) github_advisory_id: String,
    pub(crate) url: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AuditFinding {
    pub(crate) version: String,
    pub(crate) paths: Vec<String>,
    pub(crate) dev: bool,
    pub(crate) optional: bool,
    pub(crate) bundled: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawBulkAdvisory {
    pub(crate) id: Option<serde_json::Value>,
    pub(crate) url: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) severity: Option<String>,
    pub(crate) vulnerable_versions: String,
    pub(crate) cwe: Option<Cwe>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Cwe {
    One(String),
    Many(Vec<String>),
}

impl Cwe {
    pub(crate) fn into_string(self) -> String {
        match self {
            Cwe::One(value) => value,
            Cwe::Many(values) => values.join(", "),
        }
    }
}

pub(crate) fn bulk_response_to_audit_report(
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

pub(crate) fn empty_audit_report(
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

pub(crate) fn audit_metadata(
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

pub(crate) fn build_findings(
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

pub(crate) fn normalize_advisory(
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

pub(crate) fn derive_github_advisory_id(url: &str) -> String {
    let Some(idx) = url.to_ascii_uppercase().find("GHSA-") else {
        return String::new();
    };
    let id = url[idx..]
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
        .next()
        .unwrap_or_default();
    normalize_ghsa_id(id)
}

pub(crate) fn normalize_ghsa_id(ghsa_id: &str) -> String {
    let trimmed = ghsa_id.trim();
    let Some(dash) = trimmed.find('-') else {
        return trimmed.to_ascii_uppercase();
    };
    format!("{}{}", trimmed[..dash].to_ascii_uppercase(), trimmed[dash..].to_ascii_lowercase())
}

pub(crate) fn normalize_registry(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}

pub(crate) fn redact_url_userinfo(url: &str) -> String {
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

pub(crate) fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub(crate) fn sanitize_response_body(value: &str) -> String {
    sanitize_control_chars(&truncate_chars(value, 500))
}

pub(crate) fn sanitize_control_chars(value: &str) -> String {
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
