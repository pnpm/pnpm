use super::{
    Advisory, Cow, Deserialize, HashMap, MAX_ADVISORY_ID_BYTES, MAX_EVENTS_PER_RANGE,
    MAX_RANGES_PER_AFFECTED, MAX_VERSIONS_PER_AFFECTED, RegistryError, SemverEvent, SemverRange,
    Version, invalid_config,
};

#[derive(Deserialize)]
pub(super) struct OsvRecord {
    pub(super) id: String,
    #[serde(default)]
    pub(super) withdrawn: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) affected: Vec<OsvAffected>,
}

#[derive(Deserialize)]
pub(super) struct OsvAffected {
    #[serde(default)]
    pub(super) package: Option<OsvPackage>,
    #[serde(default)]
    pub(super) ranges: Vec<OsvRange>,
    #[serde(default)]
    pub(super) versions: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct OsvPackage {
    pub(super) ecosystem: String,
    pub(super) name: String,
}

#[derive(Deserialize)]
pub(super) struct OsvRange {
    #[serde(rename = "type")]
    pub(super) kind: String,
    #[serde(default)]
    pub(super) events: Vec<OsvEvent>,
}

#[derive(Deserialize)]
pub(super) struct OsvEvent {
    #[serde(default)]
    pub(super) introduced: Option<String>,
    #[serde(default)]
    pub(super) fixed: Option<String>,
    #[serde(default, rename = "last_affected")]
    pub(super) last_affected: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<String>,
}

pub(super) fn ingest_record_bytes(
    packages: &mut HashMap<String, Vec<Advisory>>,
    source: &str,
    bytes: &[u8],
) -> Result<(), RegistryError> {
    let record: OsvRecord = serde_json::from_slice(bytes)
        .map_err(|err| invalid_config(format!("failed to parse OSV record {source}: {err}")))?;
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
        if exceeds_affected_limits(&affected) {
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

/// Whether one `affected` entry declares more versions, ranges or events than
/// the index will hold for it.
pub(super) fn exceeds_affected_limits(affected: &OsvAffected) -> bool {
    affected.versions.len() > MAX_VERSIONS_PER_AFFECTED
        || affected.ranges.len() > MAX_RANGES_PER_AFFECTED
        || affected.ranges.iter().any(|range| range.events.len() > MAX_EVENTS_PER_RANGE)
}

/// Fold an npm package name to its case-insensitive key. npm forbids
/// names that differ only in case, so lowercasing can't collide two
/// distinct packages, and it keeps OSV lookups from missing an advisory
/// when a lockfile name and the OSV dump disagree on casing. Borrows
/// when the name is already lowercase (the common case).
pub(super) fn normalized_name(name: &str) -> Cow<'_, str> {
    if name.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(name.to_ascii_lowercase())
    } else {
        Cow::Borrowed(name)
    }
}

pub(super) fn advisory_from_affected(id: &str, affected: OsvAffected) -> Advisory {
    let ranges = affected.ranges.into_iter().filter_map(semver_range_from_osv).collect();
    Advisory {
        id: truncate_advisory_id(id),
        versions: affected.versions.into_iter().collect(),
        ranges,
    }
}

/// Cap a stored advisory id at a char boundary so a crafted record can't
/// carry a multi-megabyte id into memory and reason strings.
pub(super) fn truncate_advisory_id(id: &str) -> String {
    if id.len() <= MAX_ADVISORY_ID_BYTES {
        return id.to_string();
    }
    let end = (0..=MAX_ADVISORY_ID_BYTES).rev().find(|&i| id.is_char_boundary(i)).unwrap_or(0);
    format!("{}…", &id[..end])
}

pub(super) fn semver_range_from_osv(range: OsvRange) -> Option<SemverRange> {
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

pub(super) fn semver_event_from_osv(event: OsvEvent) -> Option<SemverEvent> {
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

pub(super) fn parse_osv_version(raw: &str) -> Option<Version> {
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
