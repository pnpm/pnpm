mod loader;
use loader::{load_from_directory, load_from_zip};

mod records;
use records::{ingest_record_bytes, normalized_name};

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use node_semver::Version;
use pnpm_resolving_resolver_base::{
    PackageVersionGuard, PackageVersionGuardDecision, PackageVersionGuardFuture,
};
use serde::{Deserialize, Serialize};

use pnpr_config::Config;
use pnpr_error::RegistryError;

const OSV_POLICY_KEY: &str = "osvNpmDatabase";

/// Upper bound on the bytes read for a single OSV record (one zip entry
/// or one directory file). OSV advisories are a few KB; this cap only
/// exists so a crafted database can't exhaust memory at startup.
const MAX_OSV_RECORD_BYTES: u64 = 16 * 1024 * 1024;

/// Upper bound on a zip entry's name length. OSV filenames are short
/// (`GHSA-….json`); this only rejects crafted names that would otherwise
/// force large allocations at startup.
const MAX_OSV_NAME_BYTES: usize = 4096;

/// Upper bound on a stored advisory id. Real ids are short (`GHSA-…`,
/// `CVE-…`); cap so an oversized id from a crafted record can't bloat
/// in-memory state or the reason strings it feeds.
const MAX_ADVISORY_ID_BYTES: usize = 256;

/// Upper bounds on the item counts inside one `affected` entry. The
/// per-record byte cap bounds parse-time memory, but a crafted record
/// could still persist a huge `versions`/`ranges` set into the index;
/// these caps are far above any real advisory so they only reject
/// deliberately bloated records.
const MAX_VERSIONS_PER_AFFECTED: usize = 100_000;
const MAX_RANGES_PER_AFFECTED: usize = 10_000;
const MAX_EVENTS_PER_RANGE: usize = 10_000;

/// An advisory payload matching the npm/pnpm bulk advisories endpoint contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BulkAdvisory {
    pub id: u64,
    pub url: String,
    pub title: String,
    pub severity: String,
    pub vulnerable_versions: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub cwe: Vec<String>,
}

#[derive(Debug)]
pub struct OsvIndex {
    packages: HashMap<String, Vec<Advisory>>,
    fingerprint: String,
}

impl OsvIndex {
    pub(crate) fn load_from_config(config: &Config) -> Result<Option<Arc<Self>>, RegistryError> {
        if !config.osv.enabled {
            return Ok(None);
        }
        let path = config.osv.path.clone().unwrap_or_else(|| default_osv_path(config));
        if !path.exists() {
            return Err(invalid_config(format!(
                "OSV is enabled but database {} does not exist; download the npm OSV dump to this path or set osv.path",
                path.display(),
            )));
        }
        let index = Self::load_from_path(&path)?;
        if index.packages.is_empty() {
            return Err(invalid_config(format!(
                "OSV is enabled but database {} yielded no npm advisories; check that the path points at the npm OSV dump",
                path.display(),
            )));
        }
        Ok(Some(Arc::new(index)))
    }

    pub fn load_from_path(path: &Path) -> Result<Self, RegistryError> {
        if path.is_dir() {
            return load_from_directory(path);
        }
        if path.is_file() {
            return load_from_zip(path);
        }
        Err(invalid_config(format!(
            "OSV database {} is neither a directory nor a regular file; point osv.path at the npm OSV dump (a .zip file or an extracted directory)",
            path.display(),
        )))
    }

    #[must_use]
    pub fn policy(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut policy = serde_json::Map::new();
        policy.insert(
            OSV_POLICY_KEY.to_string(),
            serde_json::Value::String(self.fingerprint.clone()),
        );
        policy
    }

    #[must_use]
    pub fn can_trust_policy(&self, policy: &serde_json::Map<String, serde_json::Value>) -> bool {
        policy.get(OSV_POLICY_KEY).and_then(serde_json::Value::as_str)
            == Some(self.fingerprint.as_str())
    }

    #[must_use]
    pub fn is_vulnerable(&self, name: &str, version: &str) -> bool {
        let Some(advisories) = self.advisories(name) else {
            return false;
        };
        let parsed = Version::parse(version).ok();
        advisories
            .iter()
            .any(|advisory| advisory.affects(version, parsed.as_ref()))
    }

    #[must_use]
    pub fn vulnerability_ids(&self, name: &str, version: &str) -> Vec<String> {
        let Some(advisories) = self.advisories(name) else {
            return Vec::new();
        };
        let parsed = Version::parse(version).ok();
        let mut seen = HashSet::new();
        advisories
            .iter()
            .filter(|advisory| advisory.affects(version, parsed.as_ref()))
            .filter(|advisory| seen.insert(advisory.id.as_str()))
            .map(|advisory| advisory.id.clone())
            .collect()
    }

    /// Query bulk advisories matching requested package names and versions,
    /// matching the `/-/npm/v1/security/advisories/bulk` endpoint schema.
    #[must_use]
    pub fn bulk_advisories(
        &self,
        packages: &HashMap<String, Vec<String>>,
    ) -> HashMap<String, Vec<BulkAdvisory>> {
        let mut result = HashMap::new();
        for (package_name, versions) in packages {
            let Some(advisories) = self.advisories(package_name) else {
                continue;
            };
            let parsed_versions: Vec<(&str, Option<Version>)> = versions
                .iter()
                .map(|raw_version| (raw_version.as_str(), Version::parse(raw_version).ok()))
                .collect();
            let mut matched = Vec::new();
            let mut seen_ids = HashSet::new();
            for advisory in advisories {
                let affects = parsed_versions
                    .iter()
                    .any(|(raw_version, parsed)| advisory.affects(raw_version, parsed.as_ref()));
                if affects && seen_ids.insert(advisory.bulk.id) {
                    matched.push(advisory.bulk.clone());
                }
            }
            if !matched.is_empty() {
                result.insert(package_name.clone(), matched);
            }
        }
        result
    }

    fn advisories(&self, name: &str) -> Option<&[Advisory]> {
        self.packages
            .get(normalized_name(name).as_ref())
            .map(Vec::as_slice)
    }

    fn decision(&self, name: &str, version: &str) -> PackageVersionGuardDecision {
        let ids = self.vulnerability_ids(name, version);
        if ids.is_empty() {
            return PackageVersionGuardDecision::Allow;
        }
        PackageVersionGuardDecision::Reject {
            reason: format!(
                "is listed in the local OSV database as vulnerable ({})",
                format_advisory_ids(&ids),
            ),
        }
    }
}

impl PackageVersionGuard for OsvIndex {
    fn check<'a>(&'a self, name: &'a str, version: &'a str) -> PackageVersionGuardFuture<'a> {
        Box::pin(async move { Ok(self.decision(name, version)) })
    }
}

#[derive(Debug)]
struct Advisory {
    id: String,
    versions: HashSet<String>,
    ranges: Vec<SemverRange>,
    bulk: BulkAdvisory,
}

impl Advisory {
    fn affects(&self, version: &str, parsed: Option<&Version>) -> bool {
        if self.versions.contains(version) {
            return true;
        }
        let Some(parsed) = parsed else {
            return false;
        };
        self.ranges
            .iter()
            .any(|range| range.affects(parsed))
    }
}

#[derive(Debug)]
struct SemverRange {
    events: Vec<SemverEvent>,
}

impl SemverRange {
    fn affects(&self, version: &Version) -> bool {
        let mut affected = false;
        for event in &self.events {
            if let Some(opens) = event.verdict_at(version) {
                affected = opens;
            }
        }
        affected
    }
}

#[derive(Debug)]
enum SemverEvent {
    Introduced(Version),
    Fixed(Version),
    LastAffected(Version),
    Limit(Version),
}

impl SemverEvent {
    fn bound(&self) -> &Version {
        match self {
            SemverEvent::Introduced(version)
            | SemverEvent::Fixed(version)
            | SemverEvent::LastAffected(version)
            | SemverEvent::Limit(version) => version,
        }
    }

    fn verdict_at(&self, version: &Version) -> Option<bool> {
        match self {
            SemverEvent::Introduced(bound) => (version >= bound).then_some(true),
            SemverEvent::Fixed(bound) => (version >= bound).then_some(false),
            SemverEvent::LastAffected(bound) => (version > bound).then_some(false),
            SemverEvent::Limit(bound) => (version >= bound).then_some(false),
        }
    }

    fn sort_rank(&self) -> u8 {
        match self {
            SemverEvent::Introduced(_) => 0,
            SemverEvent::Fixed(_) | SemverEvent::LastAffected(_) | SemverEvent::Limit(_) => 1,
        }
    }

    pub(crate) fn cmp_events(&self, other: &Self) -> std::cmp::Ordering {
        self.bound()
            .partial_cmp(other.bound())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| self.sort_rank().cmp(&other.sort_rank()))
    }
}

pub fn load_osv_index(config: &Config) -> Result<Option<Arc<OsvIndex>>, RegistryError> {
    OsvIndex::load_from_config(config)
}

fn default_osv_path(config: &Config) -> PathBuf {
    config.storage.cache_dir
        .join("osv")
        .join("npm")
        .join("all.zip")
}

#[must_use]
pub fn format_advisory_ids(ids: &[String]) -> String {
    const MAX: usize = 20;
    if ids.len() <= MAX {
        return ids.join(", ");
    }
    format!("{}, and {} more", ids[..MAX].join(", "), ids.len() - MAX)
}

fn invalid_config(reason: String) -> RegistryError {
    RegistryError::InvalidConfig { reason }
}

#[cfg(test)]
mod tests;
