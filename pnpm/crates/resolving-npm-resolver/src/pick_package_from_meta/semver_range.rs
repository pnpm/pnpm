use super::{Arc, DashMap, LazyLock, Range, Version};

/// Process-global cache of parsed [`Range`]s keyed by their source
/// string. Most installs hit the same handful of ranges thousands of times
/// (the `*` from a CLI add, the `^X` from manifest entries, the few
/// dist-tag fall-backs in `preferred_version_selectors`), and reparsing
/// each is the picker's hottest cost. The cache stores `Option<Arc<Range>>`
/// so the parse error case ("range is unparsable") is memoized too —
/// pickers fall through to the next candidate without retrying the
/// parse.
///
/// `DashMap` (not `Mutex<HashMap>`) keeps lookups lock-free under the
/// fan-out the deps-resolver runs concurrently.
pub(super) static RANGE_CACHE: LazyLock<DashMap<String, Option<Arc<Range>>>> =
    LazyLock::new(DashMap::new);

pub(super) fn cached_range(range: &str) -> Option<Arc<Range>> {
    if let Some(entry) = RANGE_CACHE.get(range) {
        // `entry` is a `dashmap::Ref` guard around the stored
        // `Option<Arc<Range>>`. `value()` projects out the `&Option<...>`
        // so the clone runs on the inner value (Arc bump + Option clone),
        // not on the guard.
        return entry.value().clone();
    }
    let normalized = normalize_partial_lte_comparators(range);
    let parsed = Range::parse(&normalized).ok().map(Arc::new);
    RANGE_CACHE.insert(range.to_string(), parsed.clone());
    parsed
}

pub(super) fn normalize_partial_lte_comparators(range: &str) -> String {
    let tokens: Vec<&str> = range.split_whitespace().collect();
    let mut normalized = Vec::with_capacity(tokens.len());
    let mut cursor = 0;
    while cursor < tokens.len() {
        let token = tokens[cursor];
        if token == "<="
            && let Some(version) = tokens.get(cursor + 1)
            && let Some(comparator) = partial_lte_upper_bound(version)
        {
            normalized.push(comparator);
            cursor += 2;
            continue;
        }
        if let Some(version) = token.strip_prefix("<=")
            && let Some(comparator) = partial_lte_upper_bound(version)
        {
            normalized.push(comparator);
            cursor += 1;
            continue;
        }
        normalized.push(token.to_string());
        cursor += 1;
    }
    normalized.join(" ")
}

pub(super) fn partial_lte_upper_bound(version: &str) -> Option<String> {
    if version.contains('-') || version.contains('+') || version.contains('*') {
        return None;
    }
    let parts: Vec<&str> = version.split('.').collect();
    match parts.as_slice() {
        [major] if is_digits(major) => {
            let major: u64 = major.parse().ok()?;
            let next_major = major.checked_add(1)?;
            Some(format!("<{next_major}.0.0-0"))
        }
        [major, minor] if is_digits(major) && is_digits(minor) => {
            let minor: u64 = minor.parse().ok()?;
            let next_minor = minor.checked_add(1)?;
            Some(format!("<{major}.{next_minor}.0-0"))
        }
        _ => None,
    }
}

pub(super) fn is_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Check whether `version` satisfies `range` under node-semver's
/// loose grammar, reusing a cached [`Range`] parse when possible.
/// A parse failure on either input is treated as "doesn't satisfy"
/// so the picker can fall through to the next candidate instead of
/// crashing.
pub(super) fn semver_satisfies_loose(version: &str, range: &str) -> bool {
    let Ok(parsed_version) = Version::parse(version) else { return false };
    let Some(parsed_range) = cached_range(range) else { return false };
    parsed_version.satisfies(&parsed_range)
}

pub(super) fn max_satisfying<Raw: AsRef<str>>(versions: &[Raw], range: &str) -> Option<String> {
    let parsed_range = cached_range(range)?;
    let mut best: Option<(Version, String)> = None;
    for version in versions {
        let Ok(parsed) = Version::parse(version.as_ref()) else { continue };
        if !parsed.satisfies(&parsed_range) {
            continue;
        }
        match &best {
            Some((current, _)) if current >= &parsed => {}
            _ => best = Some((parsed, version.as_ref().to_string())),
        }
    }
    best.map(|(_, raw)| raw)
}

pub(super) fn min_satisfying<Raw: AsRef<str>>(versions: &[Raw], range: &str) -> Option<String> {
    let parsed_range = cached_range(range)?;
    let mut best: Option<(Version, String)> = None;
    for version in versions {
        let Ok(parsed) = Version::parse(version.as_ref()) else { continue };
        if !parsed.satisfies(&parsed_range) {
            continue;
        }
        match &best {
            Some((current, _)) if current <= &parsed => {}
            _ => best = Some((parsed, version.as_ref().to_string())),
        }
    }
    best.map(|(_, raw)| raw)
}
