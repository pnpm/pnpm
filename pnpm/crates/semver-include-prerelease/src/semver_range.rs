//! Compatibility shims that make `node-semver` (Rust) range checks agree
//! with JavaScript's `semver.validRange`, which the TypeScript pnpm CLI
//! resolves manifests with.
//!
//! [`Range::parse`] always runs in loose mode: it rejects an empty comparator
//! set, drops unparsable whitespace-separated tokens inside a comparator set
//! when a sibling comparator parses, and accepts loose version spellings.
//! JavaScript `semver` separates those into loose and strict modes, and pnpm
//! relies on both. The functions below model each reading explicitly.

use std::borrow::Cow;

use node_semver::Range;
use partial::{
    Component, XRangeOrder, is_strict_plain_token, parse_components, parse_partial_version,
    split_prerelease, strip_build_metadata,
};

mod partial;

/// The range every any-version spelling normalizes to.
pub const ANY_VERSION_RANGE: &str = "*";

/// Whether `range` resolves to the unbounded range, matching JS
/// `semver.validRange(range, /* loose */ true) === "*"`.
///
/// Loose mode discards the comparator sets it cannot parse, so a single
/// empty member makes the whole union unbounded however its siblings are
/// spelled. This is the mode `version-selector-type` runs in, and so the
/// reading a version-selector classifier wants.
#[must_use]
pub fn is_any_version_range(range: &str) -> bool {
    range
        .split("||")
        .any(|comparator_set| comparator_set.trim_matches(is_semver_whitespace).is_empty())
}

/// The Rust equivalent of JS `semver.validRange(range) != null`, the strict
/// reading under which any unparsable comparator or comparator set invalidates
/// the whole range.
///
/// Belongs wherever the TypeScript CLI calls `semver.validRange` without
/// `loose`: the specifier-shape disambiguations that decide whether a
/// body is a version range or a package name.
#[must_use]
pub fn is_valid_semver_range(range: &str) -> bool {
    range.split("||").all(is_valid_comparator_set)
}

/// Parses `range` under the strict [`is_valid_semver_range`] reading,
/// returning [`Range::any`] for an any-version range and `None` when any
/// comparator in the union is invalid in strict mode.
#[must_use]
pub fn parse_strict_semver_range(range: &str) -> Option<Range> {
    if !is_valid_semver_range(range) {
        return None;
    }
    if is_any_version_range(range) {
        return Some(Range::any());
    }
    let normalized = normalize_wildcard_comparators(range);
    Range::parse(normalized.as_ref()).ok()
}

fn is_semver_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\n' | '\x0b' | '\x0c' | '\r' | ' ' | '\u{00a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}',
    )
}

fn is_valid_comparator_set(raw: &str) -> bool {
    let trimmed = raw.trim_matches(is_semver_whitespace);
    if trimmed.is_empty() {
        return true;
    }
    if let Some((left, right)) = split_hyphen_endpoints(trimmed) {
        return is_valid_hyphen_endpoint(left, HyphenSide::Lower)
            && is_valid_hyphen_endpoint(right, HyphenSide::Upper);
    }
    let mut rest = trimmed;
    while !rest.is_empty() {
        rest = rest.trim_start_matches(is_semver_whitespace);
        if rest.is_empty() {
            break;
        }
        let Some(next) = consume_comparator(rest) else {
            return false;
        };
        rest = next;
    }
    true
}

fn split_hyphen_endpoints(trimmed: &str) -> Option<(&str, &str)> {
    for (idx, ch) in trimmed.char_indices() {
        if ch != '-' {
            continue;
        }
        let before = &trimmed[..idx];
        let after = &trimmed[idx + 1..];
        if before
            .chars()
            .next_back()
            .is_some_and(is_semver_whitespace)
            && after
                .chars()
                .next()
                .is_some_and(is_semver_whitespace)
        {
            return Some((
                before.trim_end_matches(is_semver_whitespace),
                after.trim_start_matches(is_semver_whitespace),
            ));
        }
    }
    None
}

#[derive(Clone, Copy)]
enum HyphenSide {
    Lower,
    Upper,
}

fn is_valid_hyphen_endpoint(side: &str, position: HyphenSide) -> bool {
    if side.is_empty() {
        return false;
    }
    let stripped = strip_v_or_eq_prefix(side);
    if stripped.contains(is_semver_whitespace) {
        return false;
    }
    let Some(parsed) = parse_partial_version(stripped, XRangeOrder::Relaxed) else {
        return false;
    };
    let expands_prerelease_upper = match position {
        HyphenSide::Lower => false,
        HyphenSide::Upper => parsed.has_prerelease,
    };
    if parsed.is_full_numeric && !expands_prerelease_upper {
        return !side.contains(is_semver_whitespace) && is_strict_plain_token(side);
    }
    true
}

fn consume_comparator(rest: &str) -> Option<&str> {
    if let Some(after_caret) = rest.strip_prefix('^') {
        return consume_relaxed_comparator(after_caret);
    }
    if let Some(after_tilde) = rest.strip_prefix('~') {
        let after_tilde = after_tilde.trim_start_matches(is_semver_whitespace);
        let after_gt = after_tilde.strip_prefix('>').unwrap_or(after_tilde);
        return consume_relaxed_comparator(after_gt);
    }
    if let Some((op, after_op)) = strip_primitive_operator(rest) {
        return consume_primitive_operand(op, after_op);
    }
    consume_plain_or_xrange_token(rest)
}

fn consume_relaxed_comparator(rest: &str) -> Option<&str> {
    let stripped = strip_v_or_eq_prefix(rest.trim_start_matches(is_semver_whitespace));
    let (token, remaining) = split_first_token(stripped);
    parse_partial_version(token, XRangeOrder::Relaxed).map(|_| remaining)
}

#[derive(Clone, Copy)]
enum PrimitiveOp {
    Gte,
    Lte,
    Gt,
    Lt,
    Eq,
}

fn strip_primitive_operator(rest: &str) -> Option<(PrimitiveOp, &str)> {
    for (prefix, op) in [
        (">=", PrimitiveOp::Gte),
        ("<=", PrimitiveOp::Lte),
        (">", PrimitiveOp::Gt),
        ("<", PrimitiveOp::Lt),
        ("=", PrimitiveOp::Eq),
    ] {
        if let Some(after) = rest.strip_prefix(prefix) {
            return Some((op, after));
        }
    }
    None
}

fn consume_primitive_operand(op: PrimitiveOp, after_op: &str) -> Option<&str> {
    let mut operand = after_op.trim_start_matches(is_semver_whitespace);
    if matches!(op, PrimitiveOp::Gt | PrimitiveOp::Lt)
        && let Some(after_eq) = operand.strip_prefix('=')
        && after_eq
            .chars()
            .next()
            .is_some_and(|ch| !is_semver_whitespace(ch))
    {
        operand = after_eq;
    }
    consume_plain_or_xrange_token(operand)
}

fn consume_plain_or_xrange_token(rest: &str) -> Option<&str> {
    let stripped = strip_v_or_eq_prefix(rest);
    let (token, remaining) = split_first_token(stripped);
    let parsed = parse_partial_version(token, XRangeOrder::Required)?;
    if parsed.is_full_numeric {
        let (raw_token, raw_remaining) = split_first_token(rest);
        return is_strict_plain_token(raw_token).then_some(raw_remaining);
    }
    Some(remaining)
}

fn strip_v_or_eq_prefix(mut text: &str) -> &str {
    loop {
        if let Some(after_v) = text.strip_prefix('v') {
            text = after_v;
        } else if let Some(after_eq) = text.strip_prefix('=') {
            text = after_eq.trim_start_matches(is_semver_whitespace);
        } else {
            return text;
        }
    }
}

fn split_first_token(text: &str) -> (&str, &str) {
    match text.find(is_semver_whitespace) {
        Some(end) => (&text[..end], &text[end..]),
        None => (text, ""),
    }
}

/// Rewrites `^`/`~`/`=` wildcard-major comparators (`^x`, `~x`, `=*`) to `*`
/// and `~<major>.<wildcard>.<patch>` (`~1.x.0`) to `~<major>`, which
/// [`Range::parse`] rejects or panics on.
fn normalize_wildcard_comparators(range: &str) -> Cow<'_, str> {
    if !range
        .bytes()
        .any(|byte| matches!(byte, b'x' | b'X' | b'*'))
        || !range
            .bytes()
            .any(|byte| matches!(byte, b'^' | b'~' | b'='))
    {
        return Cow::Borrowed(range);
    }
    Cow::Owned(
        range
            .split("||")
            .map(normalize_branch_wildcards)
            .collect::<Vec<_>>()
            .join(" || "),
    )
}

fn normalize_branch_wildcards(branch: &str) -> String {
    let trimmed = branch.trim_matches(is_semver_whitespace);
    if trimmed.is_empty() || split_hyphen_endpoints(trimmed).is_some() {
        return trimmed.to_string();
    }
    let mut out = Vec::new();
    let mut rest = trimmed;
    while !rest.is_empty() {
        rest = rest.trim_start_matches(is_semver_whitespace);
        if rest.is_empty() {
            break;
        }
        let next = consume_comparator(rest).expect("validated by is_valid_semver_range");
        let raw_comp = rest[..rest.len() - next.len()].trim_end_matches(is_semver_whitespace);
        out.push(rewrite_single_wildcard_comparator(raw_comp));
        rest = next;
    }
    out.join(" ")
}

fn rewrite_single_wildcard_comparator(comp: &str) -> String {
    if let Some(after_op) = comp
        .strip_prefix('^')
        .or_else(|| {
            comp.strip_prefix('~')
                .map(|after| {
                    let trimmed = after.trim_start_matches(is_semver_whitespace);
                    trimmed.strip_prefix('>').unwrap_or(trimmed)
                })
        })
        .or_else(|| comp.strip_prefix('='))
    {
        let stripped = strip_v_or_eq_prefix(after_op.trim_start_matches(is_semver_whitespace));
        let core = strip_build_metadata(stripped)
            .and_then(split_prerelease)
            .map(|(core, _)| core)
            .unwrap_or(stripped);
        if let Some((Component::Wildcard, _, _)) = parse_components(core) {
            return ANY_VERSION_RANGE.to_string();
        }
        if comp.starts_with('~')
            && let Some((Component::Number, Some(Component::Wildcard), Some(_))) =
                parse_components(core)
            && let Some((major, _)) = core.split_once('.')
        {
            return format!("~{major}");
        }
    }
    comp.to_string()
}

#[cfg(test)]
mod tests;
