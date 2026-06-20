use std::{
    borrow::Cow,
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
        // Only treat a regular file as a zip. Opening and streaming a
        // FIFO/socket/device for fingerprinting could block startup
        // indefinitely (the OSV DB loads before the listen socket binds),
        // so reject anything that isn't a directory or regular file.
        if path.is_file() {
            return load_from_zip(path);
        }
        Err(invalid_config(format!(
            "OSV database {} is neither a directory nor a regular file; point osv.path at the npm OSV dump (a .zip file or an extracted directory)",
            path.display(),
        )))
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
        let Some(advisories) = self.packages.get(normalized_name(name).as_ref()) else {
            return Vec::new();
        };
        // Parse the candidate once and share it across every advisory's
        // range check; exact-version advisories never need the parse.
        let parsed = Version::parse(version).ok();
        let mut seen = HashSet::new();
        advisories
            .iter()
            .filter(|advisory| advisory.affects(version, parsed.as_ref()))
            // A single OSV record can list the same package in several
            // `affected` blocks; dedup so one advisory id isn't repeated.
            .filter(|advisory| seen.insert(advisory.id.as_str()))
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
    fn affects(&self, version: &str, parsed: Option<&Version>) -> bool {
        if self.versions.contains(version) {
            return true;
        }
        let Some(parsed) = parsed else {
            return false;
        };
        self.ranges.iter().any(|range| range.affects(parsed))
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

impl SemverEvent {
    fn bound(&self) -> &Version {
        match self {
            SemverEvent::Introduced(version)
            | SemverEvent::Fixed(version)
            | SemverEvent::LastAffected(version)
            | SemverEvent::Limit(version) => version,
        }
    }

    /// At an equal version bound an `introduced` opens the range before a
    /// closing event shuts it, so it must sort first.
    fn sort_rank(&self) -> u8 {
        match self {
            SemverEvent::Introduced(_) => 0,
            SemverEvent::Fixed(_) | SemverEvent::LastAffected(_) | SemverEvent::Limit(_) => 1,
        }
    }
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
    // Fingerprint and parse the *same* open handle. Hashing one open and
    // parsing a second would let an atomic replace of the path slip in
    // between, so the recorded fingerprint could describe different bytes
    // than the advisories actually loaded — and the verdict cache trusts
    // that fingerprint to decide whether a cached pass still holds.
    let mut file = File::open(path).map_err(|err| {
        invalid_config(format!("failed to open OSV database {}: {err}", path.display()))
    })?;
    let fingerprint = fingerprint_reader(&mut file).map_err(|err| {
        invalid_config(format!("failed to hash OSV database {}: {err}", path.display()))
    })?;
    file.rewind().map_err(|err| {
        invalid_config(format!("failed to rewind OSV database {}: {err}", path.display()))
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
        // Bound the read itself with `take` rather than trusting a
        // pre-read `metadata().len()`, which a concurrent swap could
        // invalidate (TOCTOU) — matching the zip loader's guarantee.
        let file = File::open(&entry_path).map_err(|err| {
            invalid_config(format!("failed to read OSV record {}: {err}", entry_path.display()))
        })?;
        let mut bytes = Vec::new();
        file.take(MAX_OSV_RECORD_BYTES + 1).read_to_end(&mut bytes).map_err(|err| {
            invalid_config(format!("failed to read OSV record {}: {err}", entry_path.display()))
        })?;
        if bytes.len() as u64 > MAX_OSV_RECORD_BYTES {
            return Err(invalid_config(format!(
                "OSV record {} is over the {MAX_OSV_RECORD_BYTES}-byte per-record limit",
                entry_path.display(),
            )));
        }
        // Length-prefix each component so the hash input is unambiguous:
        // plain concatenation of (name, bytes) lets two different
        // directory states hash to the same stream, which would let an
        // attacker with write access edit advisories while keeping the
        // fingerprint (and thus a trusted verdict-cache pass) unchanged.
        let name = entry_path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        hasher.update((name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
        ingest_record_bytes(&mut packages, &bytes)?;
    }
    Ok(OsvIndex { packages, fingerprint: format!("sha256:{:x}", hasher.finalize()) })
}

fn fingerprint_reader(reader: &mut impl Read) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0_u8; 1024 * 64];
    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn ingest_record_bytes(
    packages: &mut HashMap<String, Vec<Advisory>>,
    bytes: &[u8],
) -> Result<(), RegistryError> {
    let record: OsvRecord = serde_json::from_slice(bytes)
        .map_err(|err| invalid_config(format!("failed to parse OSV record: {err}")))?;
    // OSV sets `withdrawn` to a timestamp string only for withdrawn
    // records; a literal `null` is not a withdrawal, so don't drop the
    // advisory on it.
    if record.withdrawn.as_ref().is_some_and(|withdrawn| !withdrawn.is_null()) {
        return Ok(());
    }
    for affected in record.affected {
        let Some(package) = affected.package.as_ref() else { continue };
        if package.ecosystem != "npm" {
            continue;
        }
        let name = normalized_name(&package.name).into_owned();
        let advisory = advisory_from_affected(&record.id, affected);
        if advisory.versions.is_empty() && advisory.ranges.is_empty() {
            continue;
        }
        packages.entry(name).or_default().push(advisory);
    }
    Ok(())
}

/// Fold an npm package name to its case-insensitive key. npm forbids
/// names that differ only in case, so lowercasing can't collide two
/// distinct packages, and it keeps OSV lookups from missing an advisory
/// when a lockfile name and the OSV dump disagree on casing. Borrows
/// when the name is already lowercase (the common case).
fn normalized_name(name: &str) -> Cow<'_, str> {
    if name.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(name.to_ascii_lowercase())
    } else {
        Cow::Borrowed(name)
    }
}

fn advisory_from_affected(id: &str, affected: OsvAffected) -> Advisory {
    let ranges = affected.ranges.into_iter().filter_map(semver_range_from_osv).collect();
    Advisory { id: id.to_string(), versions: affected.versions.into_iter().collect(), ranges }
}

fn semver_range_from_osv(range: OsvRange) -> Option<SemverRange> {
    if range.kind != "SEMVER" && range.kind != "ECOSYSTEM" {
        return None;
    }
    let mut events = range.events.into_iter().filter_map(semver_event_from_osv).collect::<Vec<_>>();
    // `SemverRange::affects` toggles state as it walks events, so it is
    // order-sensitive. OSV expects events sorted by version bound; sort
    // here so a malformed or reordered events array can't flip a verdict.
    events.sort_by(|a, b| {
        a.bound()
            .partial_cmp(b.bound())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.sort_rank().cmp(&b.sort_rank()))
    });
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
    let parsed = Version::parse(raw).ok();
    if parsed.is_none() {
        // Surface rather than silently drop: an unparsable bound means
        // this range won't be enforced, so a corrupt dump can't degrade
        // coverage without leaving a trace in the logs.
        tracing::warn!(
            version = raw,
            "ignoring OSV range event with an unparsable version; that range will not be enforced",
        );
    }
    parsed
}

fn invalid_config(reason: String) -> RegistryError {
    RegistryError::InvalidConfig { reason }
}

#[cfg(test)]
mod tests;
