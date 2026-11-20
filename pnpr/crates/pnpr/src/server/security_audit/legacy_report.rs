use std::collections::{
    BTreeMap,
    HashMap,
    HashSet,
};

use indexmap::IndexMap;
use node_semver::{
    Range,
    Version,
};
use pnpr_osv::{
    BulkAdvisory,
    OsvIndex,
};
use serde::Serialize;
use serde_json::Value;

const MAX_PATHS_PER_VERSION: usize = 1024;
const NO_PATCHED_VERSIONS: &str = "<0.0.0";

pub(super) struct InstalledNode {
    pub(super) package: String,
    pub(super) version: String,
    pub(super) path: String,
    pub(super) is_dev: bool,
    pub(super) is_optional: bool,
    pub(super) is_bundled: bool,
}

#[derive(Default, Serialize)]
pub(super) struct LegacyAuditReport {
    pub(super) actions: Vec<Value>,
    pub(super) advisories: BTreeMap<String, LegacyAdvisory>,
    pub(super) muted: Vec<Value>,
    pub(super) metadata: LegacyAuditMetadata,
}

#[derive(Default, Serialize)]
pub(super) struct LegacyAuditMetadata {
    pub(super) vulnerabilities: LegacyVulnerabilityCounts,
    pub(super) dependencies: usize,
    #[serde(rename = "devDependencies")]
    pub(super) dev_dependencies: usize,
    #[serde(rename = "optionalDependencies")]
    pub(super) optional_dependencies: usize,
    #[serde(rename = "totalDependencies")]
    pub(super) total_dependencies: usize,
}

#[derive(Default, Serialize)]
pub(super) struct LegacyVulnerabilityCounts {
    pub(super) info: usize,
    pub(super) low: usize,
    pub(super) moderate: usize,
    pub(super) high: usize,
    pub(super) critical: usize,
    pub(super) total: usize,
}

#[derive(Serialize)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror the npm legacy security audit advisory JSON format."
    )
)]
pub(super) struct LegacyAdvisory {
    pub(super) findings: Vec<LegacyFinding>,
    pub(super) id: u64,
    pub(super) title: String,
    pub(super) module_name: String,
    pub(super) vulnerable_versions: String,
    pub(super) patched_versions: String,
    pub(super) overview: String,
    pub(super) recommendation: String,
    pub(super) references: String,
    pub(super) access: &'static str,
    pub(super) severity: String,
    pub(super) cwe: String,
    pub(super) cves: Vec<String>,
    pub(super) github_advisory_id: String,
    pub(super) url: String,
}

#[derive(Clone, Serialize)]
pub(super) struct LegacyFinding {
    pub(super) version: String,
    pub(super) paths: Vec<String>,
    pub(super) dev: bool,
    pub(super) optional: bool,
    pub(super) bundled: bool,
}

struct VersionOccurrences {
    paths: Vec<String>,
    seen_paths: HashSet<String>,
    is_dev: bool,
    is_optional: bool,
    is_bundled: bool,
}

impl LegacyVulnerabilityCounts {
    pub(super) fn record_severity(&mut self, severity: &str) {
        match severity {
            "info" => self.info += 1,
            "low" => self.low += 1,
            "moderate" => self.moderate += 1,
            "high" => self.high += 1,
            "critical" => self.critical += 1,
            _ => self.moderate += 1,
        }
        self.total += 1;
    }
}

pub(super) fn build_osv_legacy_report(
    osv_index: &OsvIndex,
    nodes: &[InstalledNode],
) -> LegacyAuditReport {
    let mut metadata = count_installed_metadata(nodes);
    let pending = collect_osv_advisory_matches(osv_index, nodes);
    let mut advisories = BTreeMap::new();
    let mut next_suffix = HashMap::new();
    for ((package_name, _), (bulk, findings)) in pending {
        metadata.vulnerabilities.record_severity(&bulk.severity);
        let key = unique_advisory_key(&advisories, bulk.id, &mut next_suffix);
        advisories.insert(key, into_legacy_advisory(bulk, package_name, findings));
    }
    LegacyAuditReport { actions: Vec::new(), advisories, muted: Vec::new(), metadata }
}

fn collect_osv_advisory_matches(
    osv_index: &OsvIndex,
    nodes: &[InstalledNode],
) -> IndexMap<(String, u64), (BulkAdvisory, Vec<LegacyFinding>)> {
    let grouped = group_installed_versions(nodes);
    let mut pending = IndexMap::new();
    for ((package_name, version), occurrences) in grouped {
        let finding = LegacyFinding {
            version: version.clone(),
            paths: occurrences.paths,
            dev: occurrences.is_dev,
            optional: occurrences.is_optional,
            bundled: occurrences.is_bundled,
        };
        for advisory in osv_index.advisories_for_version(&package_name, &version) {
            pending
                .entry((package_name.clone(), advisory.id))
                .or_insert_with(|| (advisory, Vec::new()))
                .1
                .push(finding.clone());
        }
    }
    pending
}

fn count_installed_metadata(nodes: &[InstalledNode]) -> LegacyAuditMetadata {
    let mut metadata = LegacyAuditMetadata::default();
    for node in nodes {
        if node.is_dev {
            metadata.dev_dependencies += 1;
        } else {
            metadata.dependencies += 1;
        }
        if node.is_optional {
            metadata.optional_dependencies += 1;
        }
        metadata.total_dependencies += 1;
    }
    metadata
}

fn group_installed_versions(
    nodes: &[InstalledNode],
) -> IndexMap<(String, String), VersionOccurrences> {
    let mut grouped: IndexMap<(String, String), VersionOccurrences> = IndexMap::new();
    for node in nodes {
        let entry = grouped
            .entry((node.package.clone(), node.version.clone()))
            .or_insert_with(|| VersionOccurrences {
                paths: Vec::new(),
                seen_paths: HashSet::new(),
                is_dev: true,
                is_optional: true,
                is_bundled: true,
            });
        if entry.paths.len() < MAX_PATHS_PER_VERSION && entry.seen_paths.insert(node.path.clone()) {
            entry.paths.push(node.path.clone());
        }
        entry.is_dev &= node.is_dev;
        entry.is_optional &= node.is_optional;
        entry.is_bundled &= node.is_bundled;
    }
    grouped
}

fn unique_advisory_key(
    existing: &BTreeMap<String, LegacyAdvisory>,
    id: u64,
    next_suffix: &mut HashMap<u64, usize>,
) -> String {
    let base = id.to_string();
    if !existing.contains_key(&base) {
        return base;
    }
    let suffix_cursor = next_suffix.entry(id).or_insert(2);
    loop {
        let suffix = *suffix_cursor;
        *suffix_cursor += 1;
        let candidate = format!("{id}-{suffix}");
        if !existing.contains_key(&candidate) {
            return candidate;
        }
    }
}

fn into_legacy_advisory(
    bulk: BulkAdvisory,
    module_name: String,
    findings: Vec<LegacyFinding>,
) -> LegacyAdvisory {
    let patched_versions = infer_patched_versions(&bulk.vulnerable_versions);
    let recommendation = match patched_versions.strip_prefix(">=") {
        Some(min_version) => format!("Upgrade to version {min_version} or later"),
        None => "No fix is currently available".to_string(),
    };
    let overview = bulk.title.clone();
    let references = if bulk.url.is_empty() { String::new() } else { format!("- {}", bulk.url) };
    let github_advisory_id = derive_ghsa_id(&bulk.url);
    LegacyAdvisory {
        findings,
        id: bulk.id,
        title: bulk.title,
        module_name,
        vulnerable_versions: bulk.vulnerable_versions,
        patched_versions,
        overview,
        recommendation,
        references,
        access: "public",
        severity: bulk.severity,
        cwe: bulk.cwe.join(", "),
        cves: Vec::new(),
        github_advisory_id,
        url: bulk.url,
    }
}

pub(super) fn infer_patched_versions(vulnerable_versions: &str) -> String {
    let trimmed = vulnerable_versions.trim();
    if trimmed.is_empty() {
        return NO_PATCHED_VERSIONS.to_string();
    }
    let Ok(vulnerable_range) = Range::parse(trimmed) else {
        return NO_PATCHED_VERSIONS.to_string();
    };
    let Some(max_patched) = max_patched_bound_across_clauses(trimmed) else {
        return NO_PATCHED_VERSIONS.to_string();
    };
    if vulnerable_range.satisfies(&max_patched) {
        return NO_PATCHED_VERSIONS.to_string();
    }
    format!(">={max_patched}")
}

fn max_patched_bound_across_clauses(vulnerable_versions: &str) -> Option<Version> {
    let mut highest: Option<Version> = None;
    for clause in vulnerable_versions.split("||") {
        let candidate = clause_min_patched_version(clause.trim())?;
        if highest
            .as_ref()
            .is_none_or(|current| candidate > *current)
        {
            highest = Some(candidate);
        }
    }
    highest
}

fn clause_min_patched_version(clause: &str) -> Option<Version> {
    let (operator, raw_version) = parse_trailing_upper_bound(clause)?;
    let version = Version::parse(raw_version).ok()?;
    match operator {
        "<" => Some(version),
        "<=" | "=" if version.pre_release.is_empty() => Version::parse(format!(
            "{}.{}.{}",
            version.major,
            version.minor,
            version.patch.saturating_add(1),
        ))
        .ok(),
        _ => None,
    }
}

fn parse_trailing_upper_bound(clause: &str) -> Option<(&str, &str)> {
    let mut parts = clause.split_whitespace().collect::<Vec<_>>();
    let last = parts.pop()?;
    if let Some(version) = last.strip_prefix("<=") {
        return Some(("<=", version.trim()));
    }
    if let Some(version) = last.strip_prefix('<') {
        return Some(("<", version.trim()));
    }
    if parts.is_empty() {
        let exact = last
            .strip_prefix('=')
            .unwrap_or(last)
            .trim();
        return (!exact.is_empty()).then_some(("=", exact));
    }
    let operator = parts.pop()?;
    if operator == "<=" || operator == "<" {
        return Some((operator, last));
    }
    None
}

pub(super) fn derive_ghsa_id(url: &str) -> String {
    let upper = url.to_ascii_uppercase();
    let Some(start) = upper.find("GHSA-") else {
        return String::new();
    };
    let token = url[start..]
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
        .next()
        .unwrap_or_default();
    let Some(dash) = token.find('-') else {
        return token.to_ascii_uppercase();
    };
    format!("{}{}", token[..dash].to_ascii_uppercase(), token[dash..].to_ascii_lowercase())
}
