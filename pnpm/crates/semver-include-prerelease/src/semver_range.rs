//! Compatibility shims that make `node-semver` (Rust) range checks agree
//! with JavaScript's `semver.validRange`, which the TypeScript pnpm CLI
//! resolves manifests with.
//!
//! [`Range::parse`] always runs in loose mode: it rejects an empty comparator
//! set, drops unparsable whitespace-separated tokens inside a comparator set
//! when a sibling comparator parses, and accepts loose version spellings.
//! JavaScript `semver` separates those into loose and strict modes, and pnpm
//! relies on both. The functions below model each reading explicitly.

mod partial;

use std::borrow::Cow;

use node_semver::Range;
use partial::{
    Component, PartialVersion, canonical_strict_comparator, inc_num, invalid_xrange_order,
    parse_xrange_plain, strip_build_substrings, strip_gtlt_operator, trim_comparator_whitespace,
};

/// The range every any-version spelling normalizes to.
pub const ANY_VERSION_RANGE: &str = "*";

const NULL_SET_COMPARATOR: &str = "<0.0.0-0";

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
    desugar_strict_range(range).is_some()
}

/// Parses `range` under the strict [`is_valid_semver_range`] reading,
/// returning [`Range::any`] for an any-version range and `None` when any
/// comparator in the union is invalid in strict mode or exceeds
/// [`Range::parse`]'s integer limit.
#[must_use]
pub fn parse_strict_semver_range(range: &str) -> Option<Range> {
    let branches = desugar_strict_range(range)?
        .into_iter()
        .map(|comps| eval_branch(&comps))
        .collect::<Option<Vec<_>>>()?;
    if branches
        .iter()
        .any(|branch| matches!(branch, BranchRange::Any))
    {
        return Some(Range::any());
    }
    let bounded: Vec<Range> = branches
        .into_iter()
        .filter_map(|branch| match branch {
            BranchRange::Bounded(parsed) => Some(parsed),
            BranchRange::Any | BranchRange::Empty => None,
        })
        .collect();
    match bounded.len() {
        0 => Range::parse(NULL_SET_COMPARATOR).ok(),
        1 => bounded.into_iter().next(),
        _ => {
            let joined = bounded
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("||");
            Range::parse(&joined).ok()
        }
    }
}

enum BranchRange {
    Any,
    Empty,
    Bounded(Range),
}

fn eval_branch(comps: &[String]) -> Option<BranchRange> {
    let mut current: Option<Range> = None;
    let mut is_empty = false;
    for comp in comps {
        if comp.is_empty() {
            continue;
        }
        if comp == NULL_SET_COMPARATOR {
            is_empty = true;
            continue;
        }
        let parsed = Range::parse(comp).ok()?;
        current = match current {
            None => Some(parsed),
            Some(acc) => Some(
                acc.intersect(&parsed)
                    .unwrap_or_else(|| {
                        is_empty = true;
                        acc
                    }),
            ),
        };
    }
    if is_empty {
        return Some(BranchRange::Empty);
    }
    Some(match current {
        None => BranchRange::Any,
        Some(bounded) => BranchRange::Bounded(bounded),
    })
}

fn desugar_strict_range(range: &str) -> Option<Vec<Vec<String>>> {
    let collapsed = collapse_whitespace(range);
    collapsed
        .split("||")
        .map(|branch| desugar_branch(branch.trim_matches(' ')))
        .collect()
}

fn desugar_branch(trimmed: &str) -> Option<Vec<String>> {
    if trimmed.is_empty() {
        return Some(vec![String::new()]);
    }
    let without_build = strip_build_substrings(trimmed);
    let after_hyphen = expand_hyphen_range(&without_build)?;
    let trimmed_ops = trim_comparator_whitespace(&after_hyphen);
    let glued = trimmed_ops
        .replace("~> ", "~")
        .replace("~ ", "~")
        .replace("^ ", "^");
    let expanded = glued
        .split(' ')
        .map(expand_comparator)
        .collect::<Option<Vec<_>>>()?
        .join(" ");
    expanded
        .split(' ')
        .map(|comp| {
            let trimmed_comp = comp.trim_matches(' ');
            if trimmed_comp == ">=0.0.0" { "" } else { trimmed_comp }
        })
        .map(canonical_strict_comparator)
        .collect()
}

fn expand_hyphen_range(branch: &str) -> Option<Cow<'_, str>> {
    let Some((from, to)) = branch.split_once(" - ") else {
        return Some(Cow::Borrowed(branch));
    };
    let (Some(left), Some(right)) = (parse_xrange_plain(from), parse_xrange_plain(to)) else {
        return Some(Cow::Borrowed(branch));
    };
    let lower = format_hyphen_lower(from, &left);
    let upper = format_hyphen_upper(to, &right)?;
    Some(Cow::Owned(format!("{lower} {upper}").trim_matches(' ').to_string()))
}

fn format_hyphen_lower(raw_from: &str, left: &PartialVersion<'_>) -> String {
    match (left.major, left.minor, left.patch) {
        (Component::Wildcard, _, _) => String::new(),
        (Component::Digits(maj), None | Some(Component::Wildcard), _) => format!(">={maj}.0.0"),
        (
            Component::Digits(maj),
            Some(Component::Digits(min)),
            None | Some(Component::Wildcard),
        ) => {
            format!(">={maj}.{min}.0")
        }
        (Component::Digits(_), Some(Component::Digits(_)), Some(Component::Digits(_))) => {
            format!(">={raw_from}")
        }
    }
}

fn format_hyphen_upper(raw_to: &str, right: &PartialVersion<'_>) -> Option<String> {
    match (right.major, right.minor, right.patch, right.pre) {
        (Component::Wildcard, _, _, _) => Some(String::new()),
        (Component::Digits(maj), None | Some(Component::Wildcard), _, _) => {
            Some(format!("<{}.0.0-0", inc_num(maj)?))
        }
        (
            Component::Digits(maj),
            Some(Component::Digits(min)),
            None | Some(Component::Wildcard),
            _,
        ) => Some(format!("<{maj}.{}.0-0", inc_num(min)?)),
        (
            Component::Digits(maj),
            Some(Component::Digits(min)),
            Some(Component::Digits(pat)),
            Some(pre),
        ) => Some(format!("<={maj}.{min}.{pat}-{pre}")),
        (Component::Digits(_), Some(Component::Digits(_)), Some(Component::Digits(_)), None) => {
            Some(format!("<={raw_to}"))
        }
    }
}

fn expand_comparator(comp: &str) -> Option<String> {
    let caret = replace_caret(comp)?;
    let tilde = caret
        .split(' ')
        .map(replace_tilde)
        .collect::<Option<Vec<_>>>()?
        .join(" ");
    let xrange = tilde
        .split(' ')
        .map(replace_xrange)
        .collect::<Option<Vec<_>>>()?
        .join(" ");
    Some(replace_first_star(xrange.trim_matches(' ')))
}

fn replace_caret(comp: &str) -> Option<String> {
    let Some(rest) = comp.strip_prefix('^') else {
        return Some(comp.to_string());
    };
    let Some(ver) = parse_xrange_plain(rest) else {
        return Some(comp.to_string());
    };
    match (ver.major, ver.minor, ver.patch, ver.pre) {
        (Component::Wildcard, _, _, _) => Some(String::new()),
        (Component::Digits(maj), None | Some(Component::Wildcard), _, _) => {
            Some(format!(">={maj}.0.0 <{}.0.0-0", inc_num(maj)?))
        }
        (
            Component::Digits(maj),
            Some(Component::Digits(min)),
            None | Some(Component::Wildcard),
            _,
        ) => Some(caret_partial_patch(maj, min)?),
        (
            Component::Digits(maj),
            Some(Component::Digits(min)),
            Some(Component::Digits(pat)),
            pre,
        ) => Some(caret_full(maj, min, pat, pre)?),
    }
}

fn caret_partial_patch(maj: &str, min: &str) -> Option<String> {
    if maj == "0" {
        Some(format!(">={maj}.{min}.0 <{maj}.{}.0-0", inc_num(min)?))
    } else {
        Some(format!(">={maj}.{min}.0 <{}.0.0-0", inc_num(maj)?))
    }
}

fn caret_full(maj: &str, min: &str, pat: &str, pre: Option<&str>) -> Option<String> {
    let lower = match pre {
        Some(pre) => format!(">={maj}.{min}.{pat}-{pre}"),
        None => format!(">={maj}.{min}.{pat}"),
    };
    let upper = if maj == "0" && min == "0" {
        format!("<{maj}.{min}.{}-0", inc_num(pat)?)
    } else if maj == "0" {
        format!("<{maj}.{}.0-0", inc_num(min)?)
    } else {
        format!("<{}.0.0-0", inc_num(maj)?)
    };
    Some(format!("{lower} {upper}"))
}

fn replace_tilde(comp: &str) -> Option<String> {
    let Some(rest) = comp
        .strip_prefix("~>")
        .or_else(|| comp.strip_prefix('~'))
    else {
        return Some(comp.to_string());
    };
    let Some(ver) = parse_xrange_plain(rest) else {
        return Some(comp.to_string());
    };
    match (ver.major, ver.minor, ver.patch, ver.pre) {
        (Component::Wildcard, _, _, _) => Some(String::new()),
        (Component::Digits(maj), None | Some(Component::Wildcard), _, _) => {
            Some(format!(">={maj}.0.0 <{}.0.0-0", inc_num(maj)?))
        }
        (
            Component::Digits(maj),
            Some(Component::Digits(min)),
            None | Some(Component::Wildcard),
            _,
        ) => Some(format!(">={maj}.{min}.0 <{maj}.{}.0-0", inc_num(min)?)),
        (
            Component::Digits(maj),
            Some(Component::Digits(min)),
            Some(Component::Digits(pat)),
            Some(pre),
        ) => Some(format!(">={maj}.{min}.{pat}-{pre} <{maj}.{}.0-0", inc_num(min)?)),
        (
            Component::Digits(maj),
            Some(Component::Digits(min)),
            Some(Component::Digits(pat)),
            None,
        ) => Some(format!(">={maj}.{min}.{pat} <{maj}.{}.0-0", inc_num(min)?)),
    }
}

fn replace_xrange(comp: &str) -> Option<String> {
    let trimmed = comp.trim_matches(' ');
    let (gtlt, after_op) = strip_gtlt_operator(trimmed);
    let Some(ver) = parse_xrange_plain(after_op) else {
        return Some(trimmed.to_string());
    };
    if invalid_xrange_order(&ver) {
        return Some(trimmed.to_string());
    }
    let is_wild = |part: Option<Component<'_>>| !matches!(part, Some(Component::Digits(_)));
    if !is_wild(Some(ver.major)) && !is_wild(ver.minor) && !is_wild(ver.patch) {
        return Some(trimmed.to_string());
    }
    let op = if gtlt == "=" { "" } else { gtlt };
    let Component::Digits(maj) = ver.major else {
        return Some(if matches!(op, ">" | "<") {
            NULL_SET_COMPARATOR.to_string()
        } else {
            ANY_VERSION_RANGE.to_string()
        });
    };
    format_numeric_xrange(op, maj, ver.minor)
}

fn format_numeric_xrange(op: &str, maj: &str, minor: Option<Component<'_>>) -> Option<String> {
    match (op, minor) {
        ("", None | Some(Component::Wildcard)) => {
            Some(format!(">={maj}.0.0 <{}.0.0-0", inc_num(maj)?))
        }
        ("", Some(Component::Digits(min))) => {
            Some(format!(">={maj}.{min}.0 <{maj}.{}.0-0", inc_num(min)?))
        }
        (">", None | Some(Component::Wildcard)) => Some(format!(">={}.0.0", inc_num(maj)?)),
        (">", Some(Component::Digits(min))) => Some(format!(">={maj}.{}.0", inc_num(min)?)),
        ("<=", None | Some(Component::Wildcard)) => Some(format!("<{}.0.0-0", inc_num(maj)?)),
        ("<=", Some(Component::Digits(min))) => Some(format!("<{maj}.{}.0-0", inc_num(min)?)),
        ("<", None | Some(Component::Wildcard)) => Some(format!("<{maj}.0.0-0")),
        ("<", Some(Component::Digits(min))) => Some(format!("<{maj}.{min}.0-0")),
        (_, None | Some(Component::Wildcard)) => Some(format!(">={maj}.0.0")),
        (_, Some(Component::Digits(min))) => Some(format!(">={maj}.{min}.0")),
    }
}

fn replace_first_star(comp: &str) -> String {
    let Some(star_idx) = comp.find('*') else {
        return comp.to_string();
    };
    let bytes = comp.as_bytes();
    let mut start = star_idx;
    if start > 0 && bytes[start - 1] == b' ' {
        start -= 1;
    }
    if start > 0 && bytes[start - 1] == b'=' {
        start -= 1;
    }
    if start > 0 && matches!(bytes[start - 1], b'<' | b'>') {
        start -= 1;
    }
    format!("{}{}", &comp[..start], &comp[star_idx + 1..])
}

fn collapse_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_ws = true;
    for ch in input.chars() {
        if is_semver_whitespace(ch) {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
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

#[cfg(test)]
mod tests;
