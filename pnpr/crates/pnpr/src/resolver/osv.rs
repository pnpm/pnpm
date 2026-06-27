use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
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

    pub(crate) fn is_vulnerable(&self, name: &str, version: &str) -> bool {
        let Some(advisories) = self.advisories(name) else {
            return false;
        };
        let parsed = Version::parse(version).ok();
        advisories.iter().any(|advisory| advisory.affects(version, parsed.as_ref()))
    }

    pub(crate) fn vulnerability_ids(&self, name: &str, version: &str) -> Vec<String> {
        let Some(advisories) = self.advisories(name) else {
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

    fn advisories(&self, name: &str) -> Option<&[Advisory]> {
        self.packages.get(normalized_name(name).as_ref()).map(Vec::as_slice)
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
    let file = File::open(path).map_err(|err| {
        invalid_config(format!("failed to open OSV database {}: {err}", path.display()))
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|err| {
        invalid_config(format!("failed to read OSV zip {}: {err}", path.display()))
    })?;
    let mut packages = HashMap::new();
    // Fingerprint the decompressed record contents while parsing (one
    // pass over the same handle), not the raw archive bytes: that avoids
    // a second full read and keeps the fingerprint stable across
    // recompression/repackaging of identical advisory data.
    let mut digests = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|err| {
            invalid_config(format!("failed to read OSV zip entry in {}: {err}", path.display()))
        })?;
        if !entry.is_file() || !entry.name().ends_with(".json") {
            continue;
        }
        // Bound the name before cloning it into errors/fingerprint — a
        // crafted zip can carry arbitrarily long entry names.
        if entry.name().len() > MAX_OSV_NAME_BYTES {
            return Err(invalid_config(format!(
                "OSV zip entry name is {} bytes, over the {MAX_OSV_NAME_BYTES}-byte limit",
                entry.name().len(),
            )));
        }
        let name = entry.name().to_string();
        if entry.size() > MAX_OSV_RECORD_BYTES {
            return Err(invalid_config(format!(
                "OSV zip entry {name} is {} bytes, over the {MAX_OSV_RECORD_BYTES}-byte per-record limit",
                entry.size(),
            )));
        }
        // Read one past the cap so an underreported `entry.size()` can't
        // silently truncate a record into still-valid JSON; reject if it
        // actually exceeds the limit (matching the directory loader).
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        (&mut entry)
            .take(MAX_OSV_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| invalid_config(format!("failed to read OSV zip entry {name}: {err}")))?;
        if bytes.len() as u64 > MAX_OSV_RECORD_BYTES {
            return Err(invalid_config(format!(
                "OSV zip entry {name} is over the {MAX_OSV_RECORD_BYTES}-byte per-record limit",
            )));
        }
        digests.push(record_digest(&name, &bytes));
        ingest_record_bytes(&mut packages, &bytes)?;
    }
    Ok(OsvIndex { packages, fingerprint: combine_fingerprint(digests) })
}

fn load_from_directory(path: &Path) -> Result<OsvIndex, RegistryError> {
    let entries = std::fs::read_dir(path)
        .map_err(|err| {
            invalid_config(format!("failed to read OSV directory {}: {err}", path.display()))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            invalid_config(format!("failed to read OSV directory {}: {err}", path.display()))
        })?;

    let mut packages = HashMap::new();
    let mut digests = Vec::new();
    for entry in entries {
        let entry_path = entry.path();
        if !entry_path.is_file()
            || entry_path.extension().is_none_or(|extension| extension != "json")
        {
            continue;
        }
        // Open non-blocking so a concurrent swap of the path to a
        // FIFO/socket after the `is_file` check can't make `open` itself
        // block startup; then re-check the opened handle is a regular file
        // before reading (the path check is racy on its own).
        let file = open_osv_record(&entry_path).map_err(|err| {
            invalid_config(format!("failed to read OSV record {}: {err}", entry_path.display()))
        })?;
        let is_regular_file = file.metadata().is_ok_and(|metadata| metadata.is_file());
        if !is_regular_file {
            return Err(invalid_config(format!(
                "OSV record {} is not a regular file",
                entry_path.display(),
            )));
        }
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
        let name = entry_path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        digests.push(record_digest(name, &bytes));
        ingest_record_bytes(&mut packages, &bytes)?;
    }
    Ok(OsvIndex { packages, fingerprint: combine_fingerprint(digests) })
}

/// Per-record content digest over a length-prefixed `(name, bytes)` so
/// the encoding is unambiguous — plain concatenation would let two
/// different record sets hash to the same input.
/// Open an OSV record file without blocking on it. On unix `O_NONBLOCK`
/// keeps `open` from hanging if the path was raced into a FIFO with no
/// writer; the caller still verifies the handle is a regular file before
/// reading. A regular file ignores the flag for reads.
#[cfg(unix)]
fn open_osv_record(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path)
}

#[cfg(not(unix))]
fn open_osv_record(path: &Path) -> std::io::Result<File> {
    File::open(path)
}

fn record_digest(name: &str, bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update((name.len() as u64).to_le_bytes());
    hasher.update(name.as_bytes());
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
    hasher.finalize().into()
}

/// Combine per-record digests into the database fingerprint. Each digest
/// already encodes its record's name and bytes, so sorting the digests
/// makes the fingerprint independent of the order entries happen to
/// appear in the archive or directory listing (even when two records
/// share a name) — reordering alone doesn't invalidate cached verdicts.
fn combine_fingerprint(mut digests: Vec<[u8; 32]>) -> String {
    digests.sort_unstable();
    let mut hasher = Sha256::new();
    for digest in &digests {
        hasher.update(digest);
    }
    format!("sha256:{:x}", hasher.finalize())
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
        // Reject deliberately bloated entries so a crafted record can't
        // expand into a huge persistent set in the index.
        if affected.versions.len() > MAX_VERSIONS_PER_AFFECTED
            || affected.ranges.len() > MAX_RANGES_PER_AFFECTED
            || affected.ranges.iter().any(|range| range.events.len() > MAX_EVENTS_PER_RANGE)
        {
            return Err(invalid_config(format!(
                "OSV record {} has an affected entry exceeding the version/range/event limits",
                truncate_advisory_id(&record.id),
            )));
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
    Advisory {
        id: truncate_advisory_id(id),
        versions: affected.versions.into_iter().collect(),
        ranges,
    }
}

/// Cap a stored advisory id at a char boundary so a crafted record can't
/// carry a multi-megabyte id into memory and reason strings.
fn truncate_advisory_id(id: &str) -> String {
    if id.len() <= MAX_ADVISORY_ID_BYTES {
        return id.to_string();
    }
    let end = (0..=MAX_ADVISORY_ID_BYTES).rev().find(|&i| id.is_char_boundary(i)).unwrap_or(0);
    format!("{}…", &id[..end])
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
        // OSV's `introduced: "0"` means "from the beginning". Map it to the
        // lowest possible semver (`0.0.0-0`) rather than `0.0.0`, so the
        // `version >= introduced` check still covers prereleases that sort
        // below `0.0.0` (e.g. `0.0.0-alpha.1`).
        return Version::parse("0.0.0-0").ok();
    }
    let parsed = Version::parse(raw).ok();
    if parsed.is_none() {
        // Surface rather than silently drop: an unparsable bound means
        // this range won't be enforced, so a corrupt dump can't degrade
        // coverage without leaving a trace in the logs. Bound the logged
        // value — an OSV field can be up to the per-record cap, so log a
        // short prefix plus the full length instead of the raw string.
        const MAX_LOGGED_CHARS: usize = 64;
        let prefix: String = raw.chars().take(MAX_LOGGED_CHARS).collect();
        tracing::warn!(
            version_prefix = %prefix,
            version_len = raw.len(),
            "ignoring OSV range event with an unparsable version; that range will not be enforced",
        );
    }
    parsed
}

/// Join advisory ids for a human-facing reason, capped so a package that
/// matches a huge number of advisories can't inflate response or log
/// payloads (which feed NDJSON frames and error messages).
pub(crate) fn format_advisory_ids(ids: &[String]) -> String {
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
