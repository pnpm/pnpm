use std::collections::HashSet;

use node_semver::{Range, Version};
use serde::Deserialize;

use super::{
    MAX_RANGE_COVERAGE_CHECKS, MAX_RANGES_PER_AFFECTED, RegistryError, SemverEvent, SemverRange,
    invalid_config,
};

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

pub(super) struct ParsedNpmRange<'a> {
    events: Vec<(&'a str, SemverEvent)>,
    limit: Option<(&'a str, Version)>,
}

impl ParsedNpmRange<'_> {
    pub(super) fn format_clauses(&self) -> Vec<String> {
        format_sorted_events(&self.events, self.limit.as_ref())
    }

    pub(super) fn into_semver_range(self) -> SemverRange {
        let events = self.events
            .into_iter()
            .map(|(_, event)| event)
            .collect();
        let limit = self.limit.map(|(_, version)| version);
        SemverRange { events, limit }
    }
}

pub(super) fn parse_npm_range(range: &OsvRange) -> Option<ParsedNpmRange<'_>> {
    if range.kind != "SEMVER" && range.kind != "ECOSYSTEM" {
        return None;
    }
    let limit = resolve_range_limit(&range.events)?;
    let mut events: Vec<(&str, SemverEvent)> = range.events
        .iter()
        .filter_map(semver_event_from_osv)
        .collect();
    if events.is_empty() {
        return None;
    }
    events.sort_by(|left, right| left.1.cmp_events(&right.1));
    Some(ParsedNpmRange { events, limit })
}

fn resolve_range_limit(events: &[OsvEvent]) -> Option<Option<(&str, Version)>> {
    let mut has_limit_event = false;
    let mut unbounded = false;
    let mut max_limit: Option<(&str, Version)> = None;
    for raw in events
        .iter()
        .filter_map(|event| event.limit.as_deref())
        .map(str::trim)
    {
        has_limit_event = true;
        if raw.is_empty() || raw == "*" {
            unbounded = true;
            continue;
        }
        if let Some(parsed) = parse_osv_version(raw)
            && max_limit
                .as_ref()
                .is_none_or(|(_, cur)| parsed > *cur)
        {
            max_limit = Some((raw, parsed));
        }
    }
    if unbounded || !has_limit_event {
        return Some(None);
    }
    max_limit.map(Some)
}

fn semver_event_from_osv(event: &OsvEvent) -> Option<(&str, SemverEvent)> {
    if let Some(introduced) = event.introduced.as_deref().map(str::trim) {
        return parse_osv_version(introduced).map(|ver| (introduced, SemverEvent::Introduced(ver)));
    }
    if let Some(fixed) = event.fixed.as_deref().map(str::trim) {
        return parse_osv_version(fixed).map(|ver| (fixed, SemverEvent::Fixed(ver)));
    }
    if let Some(last_affected) = event.last_affected.as_deref().map(str::trim) {
        return parse_osv_version(last_affected)
            .map(|ver| (last_affected, SemverEvent::LastAffected(ver)));
    }
    None
}

fn parse_osv_version(raw: &str) -> Option<Version> {
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

fn format_sorted_events(
    events: &[(&str, SemverEvent)],
    limit: Option<&(&str, Version)>,
) -> Vec<String> {
    let mut clauses = Vec::new();
    let mut current_intro: Option<(&str, &Version)> = None;
    for (raw, semver_event) in events {
        apply_event(raw, semver_event, limit, &mut current_intro, &mut clauses);
    }
    if let Some((intro_raw, intro_version)) = current_intro
        && let Some(clause) = format_trailing_clause(intro_raw, intro_version, limit)
    {
        clauses.push(clause);
    }
    clauses
}

fn apply_event<'a>(
    raw: &'a str,
    semver_event: &'a SemverEvent,
    limit: Option<&(&'a str, Version)>,
    current_intro: &mut Option<(&'a str, &'a Version)>,
    clauses: &mut Vec<String>,
) {
    match semver_event {
        SemverEvent::Introduced(version) => {
            current_intro.get_or_insert((raw, version));
        }
        SemverEvent::Fixed(end_version) => {
            if let Some((intro_raw, intro_version)) = current_intro.take()
                && let Some(clause) =
                    format_capped_fixed(intro_raw, intro_version, raw, end_version, limit)
            {
                clauses.push(clause);
            }
        }
        SemverEvent::LastAffected(last_version) => {
            if let Some((intro_raw, intro_version)) = current_intro.take()
                && let Some(clause) =
                    format_capped_last(intro_raw, intro_version, raw, last_version, limit)
            {
                clauses.push(clause);
            }
        }
    }
}

fn format_capped_fixed(
    intro_raw: &str,
    intro_version: &Version,
    end_raw: &str,
    end_version: &Version,
    limit: Option<&(&str, Version)>,
) -> Option<String> {
    let (eff_raw, eff_version) = match limit {
        Some((cap_raw, cap_version)) if cap_version < end_version => (*cap_raw, cap_version),
        _ => (end_raw, end_version),
    };
    (intro_version < eff_version).then(|| format_exclusive_clause(intro_raw, eff_raw))
}

fn format_capped_last(
    intro_raw: &str,
    intro_version: &Version,
    last_raw: &str,
    last_version: &Version,
    limit: Option<&(&str, Version)>,
) -> Option<String> {
    if let Some((cap_raw, cap_version)) = limit
        && cap_version <= last_version
    {
        return (intro_version < cap_version).then(|| format_exclusive_clause(intro_raw, cap_raw));
    }
    (intro_version <= last_version).then(|| format_inclusive_clause(intro_raw, last_raw))
}

fn format_trailing_clause(
    intro_raw: &str,
    intro_version: &Version,
    limit: Option<&(&str, Version)>,
) -> Option<String> {
    match limit {
        Some((cap_raw, cap_version)) => {
            (intro_version < cap_version).then(|| format_exclusive_clause(intro_raw, cap_raw))
        }
        None => Some(format_open_clause(intro_raw)),
    }
}

fn format_exclusive_clause(intro: &str, end: &str) -> String {
    if intro == "0" { format!("<{end}") } else { format!(">={intro} <{end}") }
}

fn format_inclusive_clause(intro: &str, last: &str) -> String {
    if intro == "0" { format!("<={last}") } else { format!(">={intro} <={last}") }
}

fn format_open_clause(intro: &str) -> String {
    if intro == "0" { "*".to_string() } else { format!(">={intro}") }
}

pub(super) fn build_vulnerable_versions_str(
    advisory_id: &str,
    range_clauses: &[String],
    versions: &HashSet<String>,
) -> Result<String, RegistryError> {
    let unique_clauses = dedup_range_clauses(range_clauses);
    validate_coverage_budget(advisory_id, &unique_clauses, versions.len())?;
    let parsed_ranges: Vec<Range> = unique_clauses
        .iter()
        .filter_map(|clause| clause.parse::<Range>().ok())
        .collect();
    let mut uncovered: Vec<&str> = versions
        .iter()
        .filter_map(|raw| uncovered_exact_version(raw, &parsed_ranges))
        .collect();
    uncovered.sort_unstable();
    uncovered.dedup();

    let mut parts = unique_clauses;
    parts.extend(uncovered);
    Ok(if parts.is_empty() { "<0.0.0-0".to_string() } else { parts.join(" || ") })
}

fn dedup_range_clauses(range_clauses: &[String]) -> Vec<&str> {
    let mut seen_clauses = HashSet::with_capacity(range_clauses.len());
    let mut unique_clauses = Vec::with_capacity(range_clauses.len());
    for clause in range_clauses {
        if seen_clauses.insert(clause.as_str()) {
            unique_clauses.push(clause.as_str());
        }
    }
    unique_clauses
}

fn validate_coverage_budget(
    advisory_id: &str,
    unique_clauses: &[&str],
    version_count: usize,
) -> Result<(), RegistryError> {
    let total_alternatives: usize = unique_clauses
        .iter()
        .map(|clause| clause.split("||").count())
        .fold(0_usize, usize::saturating_add);
    if total_alternatives > MAX_RANGES_PER_AFFECTED
        || version_count.saturating_mul(total_alternatives) > MAX_RANGE_COVERAGE_CHECKS
    {
        return Err(invalid_config(format!(
            "OSV record {advisory_id} exceeds the {MAX_RANGE_COVERAGE_CHECKS} version-range comparison limit",
        )));
    }
    Ok(())
}

fn uncovered_exact_version<'a>(raw: &'a str, ranges: &[Range]) -> Option<&'a str> {
    let trimmed = raw.trim();
    if !trimmed
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_digit())
        || trimmed.chars().any(char::is_whitespace)
    {
        return None;
    }
    let parsed = Version::parse(trimmed).ok()?;
    (!ranges
        .iter()
        .any(|range| parsed.satisfies(range)))
    .then_some(trimmed)
}
