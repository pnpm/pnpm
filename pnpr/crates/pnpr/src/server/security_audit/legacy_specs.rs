use node_semver::{Range, Version};
use serde_json::{Map, Value};

pub(super) const REDACTED_SCOPE_PREFIX: &str = "pnpr-redacted-scope-";

pub(super) fn node_modules_segments(key: &str) -> Vec<&str> {
    let Some((_, after_first)) = key.split_once("node_modules/") else {
        return Vec::new();
    };
    after_first
        .split("/node_modules/")
        .map(|segment| segment.trim_matches('/'))
        .filter(|segment| !segment.is_empty())
        .collect()
}

pub(super) fn entry_bool(entry: &Map<String, Value>, key: &str) -> bool {
    entry
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(super) fn parse_npm_alias(spec: &str) -> Option<(&str, Option<&str>)> {
    let rest = spec.strip_prefix("npm:")?.trim();
    if rest.is_empty() {
        return None;
    }
    parse_name_and_spec(rest)
}

pub(super) fn parse_name_and_spec(input: &str) -> Option<(&str, Option<&str>)> {
    if let Some(after_at) = input.strip_prefix('@') {
        let slash = after_at.find('/')?;
        let search_from = 1 + slash + 1;
        return match input[search_from..].find('@') {
            Some(at_offset) => {
                let split_at = search_from + at_offset;
                Some((&input[..split_at], Some(&input[split_at + 1..])))
            }
            None => Some((input, None)),
        };
    }
    match input.find('@') {
        Some(0) | None => (!input.is_empty()).then_some((input, None)),
        Some(split_at) => Some((&input[..split_at], Some(&input[split_at + 1..]))),
    }
}

pub(super) fn is_safe_semver_version(raw: &str) -> bool {
    let trimmed = raw.trim();
    !trimmed.is_empty()
        && !trimmed.contains('/')
        && !trimmed.contains(':')
        && Version::parse(trimmed).is_ok()
}

pub(super) fn is_safe_semver_range(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains(':') {
        return false;
    }
    trimmed == "*" || trimmed.eq_ignore_ascii_case("latest") || Range::parse(trimmed).is_ok()
}
