use std::collections::HashSet;

use node_semver::{
    Range,
    Version,
};
use serde_json::{
    Map,
    Value,
};

pub(super) const REDACTED_SCOPE_PREFIX: &str = "pnpr-redacted-scope-";

pub(super) struct ScopeIndexPool {
    reserved: HashSet<usize>,
    free: Vec<usize>,
    probe: usize,
}

impl ScopeIndexPool {
    pub(super) fn new(reserved: HashSet<usize>) -> Self {
        Self { reserved, free: Vec::new(), probe: 1 }
    }

    pub(super) fn scope_index_at(&mut self, ordinal: usize) -> usize {
        while self.free.len() <= ordinal {
            let candidate = self.probe;
            self.probe = self.probe.saturating_add(1);
            if !self.reserved.contains(&candidate) {
                self.free.push(candidate);
            }
        }
        self.free[ordinal]
    }
}

pub(super) struct LocalScopePool {
    local_reserved: HashSet<usize>,
    local_free: Vec<usize>,
    global_ordinal: usize,
}

impl LocalScopePool {
    pub(super) fn new(local_reserved: HashSet<usize>) -> Self {
        Self { local_reserved, local_free: Vec::new(), global_ordinal: 0 }
    }

    pub(super) fn scope_index_at(
        &mut self,
        ordinal: usize,
        mut global_at: impl FnMut(usize) -> usize,
    ) -> usize {
        while self.local_free.len() <= ordinal {
            let candidate = global_at(self.global_ordinal);
            self.global_ordinal = self.global_ordinal.saturating_add(1);
            if !self.local_reserved.contains(&candidate) {
                self.local_free.push(candidate);
            }
        }
        self.local_free[ordinal]
    }
}

pub(super) fn collect_reserved_scope_indices<'a>(
    names: impl IntoIterator<Item = &'a str>,
    reserved: &mut HashSet<usize>,
) {
    for name in names {
        if let Some(digits) = name.strip_prefix(REDACTED_SCOPE_PREFIX)
            && let Ok(idx) = digits.parse::<usize>()
        {
            reserved.insert(idx);
        }
    }
}

const MAX_SEMVER_BYTES: usize = 128;
const MAX_RANGE_BYTES: usize = 256;
const MAX_SRI_BYTES: usize = 512;

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
    if trimmed.is_empty() || trimmed.len() > MAX_SEMVER_BYTES {
        return false;
    }
    let valid_chars = trimmed
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'));
    valid_chars && is_strict_version_token(trimmed) && Version::parse(trimmed).is_ok()
}

pub(super) fn is_safe_semver_range(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_RANGE_BYTES {
        return false;
    }
    if trimmed == "*" || trimmed.eq_ignore_ascii_case("latest") {
        return true;
    }
    let all_clauses_valid = trimmed
        .split("||")
        .map(str::trim)
        .all(is_strict_range_clause);
    all_clauses_valid && Range::parse(trimmed).is_ok()
}

fn is_strict_range_clause(clause: &str) -> bool {
    if clause.is_empty() {
        return false;
    }
    clause.split_whitespace().all(is_strict_comparator_token)
}

fn is_strict_comparator_token(token: &str) -> bool {
    if matches!(token, "*" | "x" | "X" | "-" | "<" | "<=" | ">" | ">=" | "=" | "^" | "~" | "~>") {
        return true;
    }
    let rest = strip_comparator_operator(token);
    is_strict_version_or_xrange_token(rest)
}

fn strip_comparator_operator(token: &str) -> &str {
    for prefix in ["<=", ">=", "~>", "<", ">", "=", "^", "~"] {
        if let Some(rest) = token.strip_prefix(prefix) {
            return strip_optional_v_prefix(rest);
        }
    }
    strip_optional_v_prefix(token)
}

fn strip_optional_v_prefix(token: &str) -> &str {
    if let Some(rest) = token.strip_prefix(['v', 'V'])
        && rest
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_digit())
    {
        return rest;
    }
    token
}

fn is_strict_version_token(token: &str) -> bool {
    let split_idx = token
        .find(['-', '+'])
        .unwrap_or(token.len());
    let core = &token[..split_idx];
    let mut parts = core.split('.');
    let valid_core = (0..3).all(|_| {
        parts
            .next()
            .filter(|part| !part.is_empty())
            .map(str::bytes)
            .is_some_and(|mut bytes| bytes.all(|b| b.is_ascii_digit()))
    });
    valid_core && parts.next().is_none()
}

fn is_strict_version_or_xrange_token(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if let Some(split_idx) = token.find(['-', '+']) {
        return split_idx > 0 && is_strict_version_token(token) && Version::parse(token).is_ok();
    }
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() > 3 {
        return false;
    }
    parts
        .into_iter()
        .all(|part| {
            matches!(part, "*" | "x" | "X")
                || (!part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        })
}

pub(super) fn is_safe_sri_integrity(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_SRI_BYTES {
        return false;
    }
    trimmed.split_ascii_whitespace().all(is_single_sri_digest)
}

fn is_single_sri_digest(token: &str) -> bool {
    let Some((algo, digest)) = token.split_once('-') else {
        return false;
    };
    if !matches!(algo, "sha1" | "sha256" | "sha384" | "sha512") {
        return false;
    }
    if !(16..=128).contains(&digest.len()) {
        return false;
    }
    let unpadded = digest.trim_end_matches('=');
    let pad_len = digest.len() - unpadded.len();
    !unpadded.is_empty()
        && pad_len <= 2
        && unpadded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
}
