use super::{
    Advisory, BulkAdvisory, Cow, Deserialize, HashMap, HashSet, MAX_ADVISORY_ID_BYTES,
    MAX_EVENTS_PER_RANGE, MAX_RANGES_PER_AFFECTED, MAX_VERSIONS_PER_AFFECTED, RegistryError,
    SemverEvent, SemverRange, Version, invalid_config,
};
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
pub(super) struct OsvRecord {
    pub(super) id: String,
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) withdrawn: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) affected: Vec<OsvAffected>,
    #[serde(default)]
    pub(super) database_specific: Option<OsvDatabaseSpecific>,
    #[serde(default)]
    pub(super) references: Vec<OsvReference>,
}

#[derive(Deserialize)]
pub(super) struct OsvDatabaseSpecific {
    #[serde(default)]
    pub(super) severity: Option<String>,
    #[serde(default)]
    pub(super) cwe_ids: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct OsvReference {
    #[serde(rename = "type")]
    pub(super) kind: String,
    pub(super) url: String,
}

#[derive(Deserialize)]
pub(super) struct OsvAffected {
    #[serde(default)]
    pub(super) package: Option<OsvPackage>,
    #[serde(default)]
    pub(super) ranges: Vec<OsvRange>,
    #[serde(default)]
    pub(super) versions: Vec<String>,
    #[serde(default)]
    pub(super) database_specific: Option<OsvAffectedDatabaseSpecific>,
}

#[derive(Deserialize)]
pub(super) struct OsvAffectedDatabaseSpecific {
    #[serde(default)]
    pub(super) last_known_affected_version_range: Option<String>,
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
    let mut record: OsvRecord = serde_json::from_slice(bytes)
        .map_err(|err| invalid_config(format!("failed to parse OSV record {source}: {err}")))?;
    if record.withdrawn
        .as_ref()
        .is_some_and(|withdrawn| !withdrawn.is_null())
    {
        return Ok(());
    }

    let affected = std::mem::take(&mut record.affected);
    let mut by_package: HashMap<String, Vec<OsvAffected>> = HashMap::new();
    for item in affected {
        let Some(package) = item.package.as_ref() else { continue };
        if package.ecosystem != "npm" {
            continue;
        }
        if exceeds_affected_limits(&item) {
            return Err(invalid_config(format!(
                "OSV record {} has an affected entry exceeding the version/range/event limits",
                truncate_advisory_id(&record.id),
            )));
        }
        let name = normalized_name(&package.name).into_owned();
        by_package
            .entry(name)
            .or_default()
            .push(item);
    }

    for (name, affected_list) in by_package {
        if let Some(advisory) = advisory_from_record(&record, affected_list) {
            packages
                .entry(name)
                .or_default()
                .push(advisory);
        }
    }
    Ok(())
}

pub(super) fn exceeds_affected_limits(affected: &OsvAffected) -> bool {
    affected.versions.len() > MAX_VERSIONS_PER_AFFECTED
        || affected.ranges.len() > MAX_RANGES_PER_AFFECTED
        || affected.ranges
            .iter()
            .any(|range| range.events.len() > MAX_EVENTS_PER_RANGE)
}

pub(super) fn normalized_name(name: &str) -> Cow<'_, str> {
    if name.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(name.to_ascii_lowercase())
    } else {
        Cow::Borrowed(name)
    }
}

struct AffectedBounds {
    versions: HashSet<String>,
    ranges: Vec<SemverRange>,
    range_clauses: Vec<String>,
}

fn process_one_affected(affected: OsvAffected, bounds: &mut AffectedBounds) {
    let explicit = affected.database_specific
        .as_ref()
        .and_then(|specific| specific.last_known_affected_version_range.as_deref())
        .map(str::trim)
        .filter(|range_str| !range_str.is_empty());
    if let Some(explicit) = explicit {
        bounds.range_clauses.push(explicit.to_string());
    }
    for range in affected.ranges {
        if explicit.is_none() {
            bounds.range_clauses.extend(format_range_events(&range.events));
        }
        if let Some(semver_range) = semver_range_from_osv(&range) {
            bounds.ranges.push(semver_range);
        }
    }
    bounds.versions.extend(affected.versions);
}

fn collect_affected_bounds(affected_list: Vec<OsvAffected>) -> AffectedBounds {
    let mut bounds =
        AffectedBounds { versions: HashSet::new(), ranges: Vec::new(), range_clauses: Vec::new() };
    for affected in affected_list {
        process_one_affected(affected, &mut bounds);
    }
    bounds
}

pub(super) fn advisory_from_record(
    record: &OsvRecord,
    affected_list: Vec<OsvAffected>,
) -> Option<Advisory> {
    let bounds = collect_affected_bounds(affected_list);
    if bounds.versions.is_empty() && bounds.ranges.is_empty() {
        return None;
    }

    let vulnerable_versions =
        build_vulnerable_versions_str(&bounds.range_clauses, &bounds.versions);
    let bulk = build_bulk_advisory(record, vulnerable_versions);

    Some(Advisory {
        id: truncate_advisory_id(&record.id),
        versions: bounds.versions,
        ranges: bounds.ranges,
        bulk,
    })
}

fn build_bulk_advisory(record: &OsvRecord, vulnerable_versions: String) -> BulkAdvisory {
    let severity = normalize_severity(
        record.database_specific.as_ref().and_then(|specific| specific.severity.as_deref()),
    );
    let cwe = record.database_specific
        .as_ref()
        .map(|specific| specific.cwe_ids.clone())
        .unwrap_or_default();
    BulkAdvisory {
        id: derive_numeric_id(&record.id),
        url: advisory_url(record),
        title: record.summary.clone().unwrap_or_else(|| record.id.clone()),
        severity,
        vulnerable_versions,
        cwe,
    }
}

fn build_vulnerable_versions_str(range_clauses: &[String], versions: &HashSet<String>) -> String {
    if !range_clauses.is_empty() {
        range_clauses.join(" || ")
    } else if !versions.is_empty() {
        let mut sorted: Vec<&str> = versions
            .iter()
            .map(String::as_str)
            .collect();
        sorted.sort_unstable();
        sorted.join(" || ")
    } else {
        "*".to_string()
    }
}

fn advisory_url(record: &OsvRecord) -> String {
    if record.id.starts_with("GHSA-") {
        return format!("https://github.com/advisories/{}", record.id);
    }
    for reference in &record.references {
        if reference.kind == "ADVISORY" {
            return reference.url.clone();
        }
    }
    format!("https://github.com/advisories/{}", record.id)
}

fn normalize_severity(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return "moderate".to_string();
    };
    match raw.to_ascii_lowercase().as_str() {
        "info" => "info".to_string(),
        "low" => "low".to_string(),
        "moderate" | "medium" => "moderate".to_string(),
        "high" => "high".to_string(),
        "critical" => "critical".to_string(),
        _ => "moderate".to_string(),
    }
}

pub(super) fn derive_numeric_id(id: &str) -> u64 {
    if let Ok(num) = id.parse::<u64>()
        && num > 0
        && num <= 0x001F_FFFF_FFFF_FFFF
    {
        return num;
    }
    let mut hasher = Sha256::new();
    hasher.update(id.as_bytes());
    let hash = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&hash[..8]);
    let num = u64::from_be_bytes(bytes);
    (num & 0x001F_FFFF_FFFF_FFFF) | 1
}

fn sorted_format_events(events: &[OsvEvent]) -> Vec<(&OsvEvent, SemverEvent)> {
    let mut sorted: Vec<(&OsvEvent, SemverEvent)> = events
        .iter()
        .filter_map(|event| semver_event_from_osv(event).map(|parsed| (event, parsed)))
        .collect();
    sorted.sort_by(|left, right| left.1.cmp_events(&right.1));
    sorted
}

fn format_range_events(events: &[OsvEvent]) -> Vec<String> {
    let mut clauses = Vec::new();
    let mut current_intro: Option<&str> = None;

    for (event, _) in sorted_format_events(events) {
        if let Some(intro) = &event.introduced {
            current_intro = Some(intro.as_str());
        }
        if let Some(fixed) = &event.fixed {
            clauses.push(format_fixed_clause(current_intro, fixed));
            current_intro = None;
        } else if let Some(last) = &event.last_affected {
            clauses.push(format_last_affected_clause(current_intro, last));
            current_intro = None;
        }
    }

    if let Some(intro) = current_intro {
        clauses.push(format_open_clause(intro));
    }

    clauses
}

fn format_fixed_clause(intro: Option<&str>, fixed: &str) -> String {
    match intro {
        Some("0") | None => format!("<{fixed}"),
        Some(intro) => format!(">={intro} <{fixed}"),
    }
}

fn format_last_affected_clause(intro: Option<&str>, last: &str) -> String {
    match intro {
        Some("0") | None => format!("<={last}"),
        Some(intro) => format!(">={intro} <={last}"),
    }
}

fn format_open_clause(intro: &str) -> String {
    if intro == "0" { "*".to_string() } else { format!(">={intro}") }
}

pub(super) fn truncate_advisory_id(id: &str) -> String {
    if id.len() <= MAX_ADVISORY_ID_BYTES {
        return id.to_string();
    }
    let end = (0..=MAX_ADVISORY_ID_BYTES)
        .rev()
        .find(|&i| id.is_char_boundary(i))
        .unwrap_or(0);
    format!("{}…", &id[..end])
}

pub(super) fn semver_range_from_osv(range: &OsvRange) -> Option<SemverRange> {
    if range.kind != "SEMVER" && range.kind != "ECOSYSTEM" {
        return None;
    }
    let mut events = range.events
        .iter()
        .filter_map(semver_event_from_osv)
        .collect::<Vec<_>>();
    events.sort_by(SemverEvent::cmp_events);
    (!events.is_empty()).then_some(SemverRange { events })
}

pub(super) fn semver_event_from_osv(event: &OsvEvent) -> Option<SemverEvent> {
    if let Some(introduced) = &event.introduced {
        return parse_osv_version(introduced).map(SemverEvent::Introduced);
    }
    if let Some(fixed) = &event.fixed {
        return parse_osv_version(fixed).map(SemverEvent::Fixed);
    }
    if let Some(last_affected) = &event.last_affected {
        return parse_osv_version(last_affected).map(SemverEvent::LastAffected);
    }
    if let Some(limit) = &event.limit {
        return parse_osv_version(limit).map(SemverEvent::Limit);
    }
    None
}

pub(super) fn parse_osv_version(raw: &str) -> Option<Version> {
    if raw == "0" {
        return Version::parse("0.0.0-0").ok();
    }
    let parsed = Version::parse(raw).ok();
    if parsed.is_none() {
        const MAX_LOGGED_CHARS: usize = 64;
        let prefix: String = raw
            .chars()
            .take(MAX_LOGGED_CHARS)
            .collect();
        tracing::warn!(
            version_prefix = %prefix,
            version_len = raw.len(),
            "ignoring OSV range event with an unparsable version; that range will not be enforced",
        );
    }
    parsed
}
