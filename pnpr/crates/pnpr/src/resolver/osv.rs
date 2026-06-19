use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{Read, Seek},
    path::{Path, PathBuf},
    sync::Arc,
};

use node_semver::Version;
use pacquet_resolving_resolver_base::{
    PackageVersionGuard, PackageVersionGuardDecision, PackageVersionGuardFuture,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{Config, error::RegistryError};

const OSV_POLICY_KEY: &str = "osvNpmDatabase";

/// Upper bound on the bytes read for a single OSV record (one zip entry
/// or one directory file). OSV advisories are a few KB; this cap only
/// exists so a crafted database can't exhaust memory at startup.
const MAX_OSV_RECORD_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct OsvIndex {
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
        // An enabled-but-empty index would make the guard a no-op and
        // silently disable enforcement, so treat it as misconfiguration.
        if index.packages.is_empty() {
            return Err(invalid_config(format!(
                "OSV is enabled but database {} yielded no npm advisories; check that the path points at the npm OSV dump",
                path.display(),
            )));
        }
        Ok(Some(Arc::new(index)))
    }

    pub(crate) fn load_from_path(path: &Path) -> Result<Self, RegistryError> {
        if path.is_dir() {
            return load_from_directory(path);
        }
        load_from_zip(path)
    }

    pub(crate) fn policy(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut policy = serde_json::Map::new();
        policy.insert(
            OSV_POLICY_KEY.to_string(),
            serde_json::Value::String(self.fingerprint.clone()),
        );
        policy
    }

    pub(crate) fn can_trust_policy(
        &self,
        policy: &serde_json::Map<String, serde_json::Value>,
    ) -> bool {
        policy.get(OSV_POLICY_KEY).and_then(serde_json::Value::as_str)
            == Some(self.fingerprint.as_str())
    }

    pub(crate) fn vulnerability_ids(&self, name: &str, version: &str) -> Vec<String> {
        let Some(advisories) = self.packages.get(name) else {
            return Vec::new();
        };
        advisories
            .iter()
            .filter(|advisory| advisory.affects(version))
            .map(|advisory| advisory.id.clone())
            .collect()
    }

    fn decision(&self, name: &str, version: &str) -> PackageVersionGuardDecision {
        let ids = self.vulnerability_ids(name, version);
        if ids.is_empty() {
            return PackageVersionGuardDecision::Allow;
        }
        PackageVersionGuardDecision::Reject {
            reason: format!(
                "is listed in the local OSV database as vulnerable ({})",
                ids.join(", "),
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
}

impl Advisory {
    fn affects(&self, version: &str) -> bool {
        if self.versions.contains(version) {
            return true;
        }
        let Ok(parsed) = Version::parse(version) else {
            return false;
        };
        self.ranges.iter().any(|range| range.affects(&parsed))
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
            match event {
                SemverEvent::Introduced(introduced) => {
                    if version >= introduced {
                        affected = true;
                    }
                }
                SemverEvent::Fixed(fixed) => {
                    if version >= fixed {
                        affected = false;
                    }
                }
                SemverEvent::LastAffected(last_affected) => {
                    if version > last_affected {
                        affected = false;
                    }
                }
                SemverEvent::Limit(limit) => {
                    if version >= limit {
                        affected = false;
                    }
                }
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

#[derive(Deserialize)]
struct OsvRecord {
    id: String,
    #[serde(default)]
    withdrawn: Option<serde_json::Value>,
    #[serde(default)]
    affected: Vec<OsvAffected>,
}

#[derive(Deserialize)]
struct OsvAffected {
    #[serde(default)]
    package: Option<OsvPackage>,
    #[serde(default)]
    ranges: Vec<OsvRange>,
    #[serde(default)]
    versions: Vec<String>,
}

#[derive(Deserialize)]
struct OsvPackage {
    ecosystem: String,
    name: String,
}

#[derive(Deserialize)]
struct OsvRange {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    events: Vec<OsvEvent>,
}

#[derive(Deserialize)]
struct OsvEvent {
    #[serde(default)]
    introduced: Option<String>,
    #[serde(default)]
    fixed: Option<String>,
    #[serde(default, rename = "last_affected")]
    last_affected: Option<String>,
    #[serde(default)]
    limit: Option<String>,
}

pub(crate) fn load_osv_index(config: &Config) -> Result<Option<Arc<OsvIndex>>, RegistryError> {
    OsvIndex::load_from_config(config)
}

fn default_osv_path(config: &Config) -> PathBuf {
    config.cache_storage.join("osv").join("npm").join("all.zip")
}

fn load_from_zip(path: &Path) -> Result<OsvIndex, RegistryError> {
    let fingerprint = file_fingerprint(path)?;
    let file = File::open(path).map_err(|err| {
        invalid_config(format!("failed to open OSV database {}: {err}", path.display()))
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|err| {
        invalid_config(format!("failed to read OSV zip {}: {err}", path.display()))
    })?;
    let mut packages = HashMap::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|err| {
            invalid_config(format!("failed to read OSV zip entry in {}: {err}", path.display()))
        })?;
        if !entry.is_file() || !entry.name().ends_with(".json") {
            continue;
        }
        let name = entry.name().to_string();
        if entry.size() > MAX_OSV_RECORD_BYTES {
            return Err(invalid_config(format!(
                "OSV zip entry {name} is {} bytes, over the {MAX_OSV_RECORD_BYTES}-byte per-record limit",
                entry.size(),
            )));
        }
        // `take` also caps the read if the entry's declared size lies.
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .take(MAX_OSV_RECORD_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|err| invalid_config(format!("failed to read OSV zip entry {name}: {err}")))?;
        ingest_record_bytes(&mut packages, &bytes)?;
    }
    Ok(OsvIndex { packages, fingerprint })
}

fn load_from_directory(path: &Path) -> Result<OsvIndex, RegistryError> {
    let mut entries = std::fs::read_dir(path)
        .map_err(|err| {
            invalid_config(format!("failed to read OSV directory {}: {err}", path.display()))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            invalid_config(format!("failed to read OSV directory {}: {err}", path.display()))
        })?;
    entries.sort_by_key(std::fs::DirEntry::path);

    let mut packages = HashMap::new();
    let mut hasher = Sha256::new();
    for entry in entries {
        let entry_path = entry.path();
        if !entry_path.is_file()
            || entry_path.extension().is_none_or(|extension| extension != "json")
        {
            continue;
        }
        let len = entry.metadata().map(|metadata| metadata.len()).unwrap_or_default();
        if len > MAX_OSV_RECORD_BYTES {
            return Err(invalid_config(format!(
                "OSV record {} is {len} bytes, over the {MAX_OSV_RECORD_BYTES}-byte per-record limit",
                entry_path.display(),
            )));
        }
        hasher.update(entry_path.file_name().and_then(|name| name.to_str()).unwrap_or_default());
        let bytes = std::fs::read(&entry_path).map_err(|err| {
            invalid_config(format!("failed to read OSV record {}: {err}", entry_path.display()))
        })?;
        hasher.update(&bytes);
        ingest_record_bytes(&mut packages, &bytes)?;
    }
    Ok(OsvIndex { packages, fingerprint: format!("sha256:{:x}", hasher.finalize()) })
}

fn file_fingerprint(path: &Path) -> Result<String, RegistryError> {
    let mut file = File::open(path).map_err(|err| {
        invalid_config(format!("failed to open OSV database {}: {err}", path.display()))
    })?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0_u8; 1024 * 64];
    loop {
        let read = file.read(&mut buf).map_err(|err| {
            invalid_config(format!("failed to hash OSV database {}: {err}", path.display()))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    file.rewind().map_err(|err| {
        invalid_config(format!("failed to rewind OSV database {}: {err}", path.display()))
    })?;
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn ingest_record_bytes(
    packages: &mut HashMap<String, Vec<Advisory>>,
    bytes: &[u8],
) -> Result<(), RegistryError> {
    let record: OsvRecord = serde_json::from_slice(bytes)
        .map_err(|err| invalid_config(format!("failed to parse OSV record: {err}")))?;
    if record.withdrawn.is_some() {
        return Ok(());
    }
    for affected in record.affected {
        let Some(package) = affected.package.as_ref() else { continue };
        if package.ecosystem != "npm" {
            continue;
        }
        let name = package.name.clone();
        let advisory = advisory_from_affected(&record.id, affected);
        if advisory.versions.is_empty() && advisory.ranges.is_empty() {
            continue;
        }
        packages.entry(name).or_default().push(advisory);
    }
    Ok(())
}

fn advisory_from_affected(id: &str, affected: OsvAffected) -> Advisory {
    let ranges = affected.ranges.into_iter().filter_map(semver_range_from_osv).collect();
    Advisory { id: id.to_string(), versions: affected.versions.into_iter().collect(), ranges }
}

fn semver_range_from_osv(range: OsvRange) -> Option<SemverRange> {
    if range.kind != "SEMVER" && range.kind != "ECOSYSTEM" {
        return None;
    }
    let events = range.events.into_iter().filter_map(semver_event_from_osv).collect::<Vec<_>>();
    (!events.is_empty()).then_some(SemverRange { events })
}

fn semver_event_from_osv(event: OsvEvent) -> Option<SemverEvent> {
    if let Some(introduced) = event.introduced {
        return parse_osv_version(&introduced).map(SemverEvent::Introduced);
    }
    if let Some(fixed) = event.fixed {
        return parse_osv_version(&fixed).map(SemverEvent::Fixed);
    }
    if let Some(last_affected) = event.last_affected {
        return parse_osv_version(&last_affected).map(SemverEvent::LastAffected);
    }
    if let Some(limit) = event.limit {
        return parse_osv_version(&limit).map(SemverEvent::Limit);
    }
    None
}

fn parse_osv_version(raw: &str) -> Option<Version> {
    if raw == "0" {
        return Version::parse("0.0.0").ok();
    }
    Version::parse(raw).ok()
}

fn invalid_config(reason: String) -> RegistryError {
    RegistryError::InvalidConfig { reason }
}

#[cfg(test)]
mod tests;
