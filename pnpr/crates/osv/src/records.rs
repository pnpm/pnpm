use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use super::{
    Advisory, BulkAdvisory, Cow, Deserialize, HashMap, HashSet, MAX_ADVISORY_ID_BYTES,
    MAX_EVENTS_PER_RANGE, MAX_EXPLICIT_RANGE_BYTES, MAX_RANGES_PER_AFFECTED,
    MAX_VERSIONS_PER_AFFECTED, RegistryError, SemverRange, invalid_config,
    ranges::{OsvRange, build_vulnerable_versions_str, parse_npm_range},
    severity::{OsvSeverity, SeverityLevel},
};

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
    #[serde(default)]
    pub(super) aliases: Option<Vec<String>>,
    #[serde(default)]
    pub(super) severity: Option<Vec<OsvSeverity>>,
}

#[derive(Deserialize)]
pub(super) struct OsvDatabaseSpecific {
    #[serde(default, deserialize_with = "deserialize_optional_string")]
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
    #[serde(default)]
    pub(super) ecosystem_specific: Option<OsvEcosystemSpecific>,
    #[serde(default)]
    pub(super) severity: Option<Vec<OsvSeverity>>,
}

#[derive(Deserialize)]
pub(super) struct OsvAffectedDatabaseSpecific {
    #[serde(default)]
    pub(super) last_known_affected_version_range: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub(super) severity: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct OsvEcosystemSpecific {
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub(super) severity: Option<String>,
}

fn deserialize_optional_string<'de, Deser>(
    deserializer: Deser,
) -> Result<Option<String>, Deser::Error>
where
    Deser: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(text)) => Some(text),
        _ => None,
    })
}

#[derive(Deserialize)]
pub(super) struct OsvPackage {
    pub(super) ecosystem: String,
    pub(super) name: String,
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
        if let Some(advisory) = advisory_from_record(&record, affected_list)? {
            packages
                .entry(name)
                .or_default()
                .push(advisory);
        }
    }
    Ok(())
}

pub(super) fn exceeds_affected_limits(affected: &OsvAffected) -> bool {
    let explicit_len = affected.database_specific
        .as_ref()
        .and_then(|specific| specific.last_known_affected_version_range.as_deref())
        .map_or(0, str::len);
    explicit_len > MAX_EXPLICIT_RANGE_BYTES
        || affected.versions.len() > MAX_VERSIONS_PER_AFFECTED
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
    max_severity: Option<SeverityLevel>,
}

fn process_one_affected(affected: OsvAffected, bounds: &mut AffectedBounds) {
    bounds.max_severity = [bounds.max_severity, affected_max_severity(&affected)]
        .into_iter()
        .flatten()
        .max();
    let explicit = affected.database_specific
        .as_ref()
        .and_then(|specific| specific.last_known_affected_version_range.as_deref())
        .map(str::trim)
        .filter(|range_str| !range_str.is_empty());
    if let Some(explicit) = explicit {
        bounds.range_clauses.push(explicit.to_string());
    }
    for range in &affected.ranges {
        if let Some(parsed) = parse_npm_range(range) {
            if explicit.is_none() {
                bounds.range_clauses.extend(parsed.format_clauses());
            }
            bounds.ranges.push(parsed.into_semver_range());
        }
    }
    bounds.versions.extend(affected.versions);
}

fn affected_max_severity(affected: &OsvAffected) -> Option<SeverityLevel> {
    let db_sev = affected.database_specific
        .as_ref()
        .and_then(|specific| specific.severity.as_deref())
        .and_then(SeverityLevel::from_label);
    let eco_sev = affected.ecosystem_specific
        .as_ref()
        .and_then(|specific| specific.severity.as_deref())
        .and_then(SeverityLevel::from_label);
    let vec_sev = affected.severity
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(SeverityLevel::from_osv_severity)
        .max();
    [db_sev, eco_sev, vec_sev]
        .into_iter()
        .flatten()
        .max()
}

fn collect_affected_bounds(affected_list: Vec<OsvAffected>) -> AffectedBounds {
    let mut bounds = AffectedBounds {
        versions: HashSet::new(),
        ranges: Vec::new(),
        range_clauses: Vec::new(),
        max_severity: None,
    };
    for affected in affected_list {
        process_one_affected(affected, &mut bounds);
    }
    bounds
}

pub(super) fn advisory_from_record(
    record: &OsvRecord,
    affected_list: Vec<OsvAffected>,
) -> Result<Option<Advisory>, RegistryError> {
    let id = truncate_advisory_id(&record.id);
    let bounds = collect_affected_bounds(affected_list);
    if bounds.versions.len() > MAX_VERSIONS_PER_AFFECTED
        || bounds.ranges.len() > MAX_RANGES_PER_AFFECTED
    {
        return Err(invalid_config(format!(
            "OSV record {id} has combined affected entries exceeding the version/range limits",
        )));
    }
    if bounds.versions.is_empty() && bounds.ranges.is_empty() {
        return Ok(None);
    }

    let vulnerable_versions =
        build_vulnerable_versions_str(&id, &bounds.range_clauses, &bounds.versions)?;
    let bulk = build_bulk_advisory(record, vulnerable_versions, bounds.max_severity);

    Ok(Some(Advisory { id, versions: bounds.versions, ranges: bounds.ranges, bulk }))
}

fn build_bulk_advisory(
    record: &OsvRecord,
    vulnerable_versions: String,
    affected_severity: Option<SeverityLevel>,
) -> BulkAdvisory {
    let severity = resolve_record_severity(record, affected_severity);
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

fn resolve_record_severity(record: &OsvRecord, affected_severity: Option<SeverityLevel>) -> String {
    let db_sev = record.database_specific
        .as_ref()
        .and_then(|specific| specific.severity.as_deref())
        .and_then(SeverityLevel::from_label);
    let vec_sev = record.severity
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(SeverityLevel::from_osv_severity)
        .max();
    [db_sev, vec_sev, affected_severity]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(SeverityLevel::Moderate)
        .as_str()
        .to_string()
}

fn advisory_url(record: &OsvRecord) -> String {
    if is_ghsa_id(&record.id) {
        return format_ghsa_url(&record.id);
    }
    if let Some(ghsa) = record.aliases
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|alias| is_ghsa_id(alias))
    {
        return format_ghsa_url(ghsa);
    }
    if let Some(reference) = record.references
        .iter()
        .find(|reference| reference.kind == "ADVISORY" && is_http_url(&reference.url))
    {
        return reference.url.clone();
    }
    let truncated = truncate_advisory_id(&record.id);
    let encoded_id = encode_url_path_segment(&truncated);
    format!("https://osv.dev/vulnerability/{encoded_id}")
}

fn format_ghsa_url(ghsa: &str) -> String {
    format!("https://github.com/advisories/GHSA-{}", ghsa[5..].to_ascii_lowercase())
}

fn is_ghsa_id(candidate: &str) -> bool {
    let bytes = candidate.as_bytes();
    if bytes.len() != 19 || !bytes[..5].eq_ignore_ascii_case(b"GHSA-") {
        return false;
    }
    bytes[9] == b'-'
        && bytes[14] == b'-'
        && is_ghsa_segment(&bytes[5..9])
        && is_ghsa_segment(&bytes[10..14])
        && is_ghsa_segment(&bytes[15..19])
}

fn is_ghsa_segment(segment: &[u8]) -> bool {
    segment
        .iter()
        .all(|byte| {
            matches!(
                byte.to_ascii_lowercase(),
                b'2'..=b'9'
                    | b'c'
                    | b'f'
                    | b'g'
                    | b'h'
                    | b'j'
                    | b'm'
                    | b'p'
                    | b'q'
                    | b'r'
                    | b'v'
                    | b'w'
                    | b'x',
            )
        })
}

fn is_http_url(raw_url: &str) -> bool {
    if raw_url.chars().any(char::is_control) {
        return false;
    }
    let Ok(parsed) = url::Url::parse(raw_url) else {
        return false;
    };
    matches!(parsed.scheme(), "http" | "https") && parsed.has_host()
}

fn encode_url_path_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for &byte in segment.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
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
