use std::collections::HashSet;

use node_semver::{Range, Version};
use serde::Deserialize;

use super::{SemverEvent, SemverRange};

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
}

impl ParsedNpmRange<'_> {
    pub(super) fn format_clauses(&self) -> Vec<String> {
        format_sorted_events(&self.events)
    }

    pub(super) fn into_semver_range(self) -> SemverRange {
        let events = self.events
            .into_iter()
            .map(|(_, event)| event)
            .collect();
        SemverRange { events }
    }
}

pub(super) fn parse_npm_range(range: &OsvRange) -> Option<ParsedNpmRange<'_>> {
    if range.kind != "SEMVER" && range.kind != "ECOSYSTEM" {
        return None;
    }
    let mut events: Vec<(&str, SemverEvent)> = range.events
        .iter()
        .filter_map(semver_event_from_osv)
        .collect();
    if events.is_empty() {
        return None;
    }
    events.sort_by(|left, right| left.1.cmp_events(&right.1));
    Some(ParsedNpmRange { events })
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
    if let Some(limit) = event.limit.as_deref().map(str::trim) {
        return parse_osv_version(limit).map(|ver| (limit, SemverEvent::Limit(ver)));
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

fn format_sorted_events(events: &[(&str, SemverEvent)]) -> Vec<String> {
    let mut clauses = Vec::new();
    let mut current_intro: Option<(&str, &Version)> = None;
    for (raw, semver_event) in events {
        apply_event(raw, semver_event, &mut current_intro, &mut clauses);
    }
    if let Some((intro, _)) = current_intro {
        clauses.push(format_open_clause(intro));
    }
    clauses
}

fn apply_event<'a>(
    raw: &'a str,
    semver_event: &'a SemverEvent,
    current_intro: &mut Option<(&'a str, &'a Version)>,
    clauses: &mut Vec<String>,
) {
    match semver_event {
        SemverEvent::Introduced(version) => {
            current_intro.get_or_insert((raw, version));
        }
        SemverEvent::Fixed(end_version) | SemverEvent::Limit(end_version) => {
            if let Some((intro_raw, intro_version)) = current_intro.take()
                && intro_version < end_version
            {
                clauses.push(format_exclusive_clause(intro_raw, raw));
            }
        }
        SemverEvent::LastAffected(end_version) => {
            if let Some((intro_raw, intro_version)) = current_intro.take()
                && intro_version <= end_version
            {
                clauses.push(format_inclusive_clause(intro_raw, raw));
            }
        }
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
    range_clauses: &[String],
    versions: &HashSet<String>,
) -> String {
    let mut unique_clauses: Vec<&str> = Vec::new();
    for clause in range_clauses {
        if !unique_clauses.contains(&clause.as_str()) {
            unique_clauses.push(clause.as_str());
        }
    }
    let parsed_ranges: Vec<Range> = unique_clauses
        .iter()
        .filter_map(|clause| clause.parse::<Range>().ok())
        .collect();
    let mut uncovered: Vec<&str> = versions
        .iter()
        .map(|raw| raw.trim())
        .filter(|raw| is_safe_version_token(raw) && !is_covered_by_ranges(raw, &parsed_ranges))
        .collect();
    uncovered.sort_unstable();
    uncovered.dedup();

    let mut parts = unique_clauses;
    parts.extend(uncovered);
    if parts.is_empty() { "*".to_string() } else { parts.join(" || ") }
}

fn is_safe_version_token(token: &str) -> bool {
    !token.is_empty()
        && !token.chars().any(char::is_whitespace)
        && !token.contains('|')
        && !token.starts_with(['<', '>', '^', '~', '*'])
}

fn is_covered_by_ranges(raw_version: &str, ranges: &[Range]) -> bool {
    let Ok(version) = Version::parse(raw_version) else {
        return false;
    };
    ranges
        .iter()
        .any(|range| version.satisfies(range))
}
