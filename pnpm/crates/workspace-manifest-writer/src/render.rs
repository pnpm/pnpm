//! Value rendering and key-ordering helpers for the manifest writer.
//!
//! New scalars are rendered so that a value that *needs* quoting uses single
//! quotes while plain-safe values stay unquoted (`zoo: 4.0.0`,
//! `newPkg: ^2.0.0`). [`yaml_serde`] implements that exact policy
//! (`>=2.0.0` → `'>=2.0.0'`, `@scope/x` → `'@scope/x'`, otherwise plain), so
//! value text is delegated to it rather than re-derived. A document that
//! prefers double quotes gets them through [`AddedQuotes`] when it is written.
//! Key ordering is handled by [`detect_key_layout`] and [`sort_keys`].

use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::HashSet,
    ops::Range,
};

use serde_saphyr::granit_parser::{
    ScalarStyle,
    Scanner,
    StrInput,
    TokenType,
};

/// The quoting style for string scalars that require quotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuoteStyle {
    Single,
    Double,
}

/// How a map's existing keys were laid out, used to place new keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// Not sorted, new keys append at the end.
    Unordered,
    /// Alphabetical (lexicographic), new keys sort in.
    Alphabetical,
    /// A leading `packages` key, then alphabetical, `packages` stays first.
    PackagesFirst,
}

/// A plain code-unit comparison. Rust `str::cmp` compares by Unicode scalar,
/// which matches for the BMP identifiers used as catalog/field keys.
fn lex_cmp(left: &str, right: &str) -> Ordering {
    left.cmp(right)
}

/// Classify `keys` by their existing layout. Empty input is `PackagesFirst`,
/// the convention for brand-new manifests.
pub(crate) fn detect_key_layout(keys: &[String]) -> Layout {
    if keys.is_empty() {
        return Layout::PackagesFirst;
    }
    let packages_first = keys[0] == "packages";
    let start = usize::from(packages_first);
    for window in keys[start..].windows(2) {
        if lex_cmp(&window[0], &window[1]) == Ordering::Greater {
            return Layout::Unordered;
        }
    }
    if packages_first { Layout::PackagesFirst } else { Layout::Alphabetical }
}

/// Sort `keys` for a sorted layout, keeping `packages` first when present.
fn sort_keys(keys: &mut [String], layout: Layout) {
    match layout {
        Layout::PackagesFirst => {
            keys.sort_by(|left, right| match (left == "packages", right == "packages") {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => lex_cmp(left, right),
            });
        }
        Layout::Alphabetical => keys.sort_by(|left, right| lex_cmp(left, right)),
        Layout::Unordered => {}
    }
}

/// The order keys should appear in after adding `new_keys` to `existing`,
/// for a single level: existing order is preserved when no key is added;
/// otherwise the merged set is re-sorted for a sorted layout, or new keys are
/// appended for an unordered one.
pub(crate) fn target_order(existing: &[String], new_keys: &[String]) -> Vec<String> {
    if new_keys.is_empty() {
        return existing.to_vec();
    }
    let layout = detect_key_layout(existing);
    let mut merged: Vec<String> = existing
        .iter()
        .chain(new_keys)
        .cloned()
        .collect();
    if layout != Layout::Unordered {
        sort_keys(&mut merged, layout);
    }
    merged
}

/// Detect the dominant quote style of string scalars in a YAML document.
///
/// If double quotes are dominant, returns [`QuoteStyle::Double`]. If single
/// quotes are dominant, or if neither is used, or if counts are equal,
/// falls back to [`QuoteStyle::Single`].
pub(crate) fn detect_quote_style(text: &str) -> QuoteStyle {
    let mut single_count = 0usize;
    let mut double_count = 0usize;

    for token in Scanner::new(StrInput::new(text)).map_while(Result::ok) {
        let token = token.token_type();
        match token {
            TokenType::Scalar(ScalarStyle::SingleQuoted, _) => single_count += 1,
            TokenType::Scalar(ScalarStyle::DoubleQuoted, _) => double_count += 1,
            _ => {}
        }
    }

    if double_count > single_count { QuoteStyle::Double } else { QuoteStyle::Single }
}

/// Render a scalar string value (plain when safe, single-quoted otherwise)
/// by delegating to [`yaml_serde`].
pub(crate) fn render_value(value: &str) -> String {
    yaml_serde::to_string(&yaml_serde::Value::from(value))
        .expect("serializing a string scalar to YAML never fails")
        .trim_end()
        .to_string()
}

/// How a write quotes the scalars that edits added to a document.
///
/// Edits render a scalar that needs quoting with single quotes. When the
/// original document prefers double quotes (see [`detect_quote_style`]), the
/// written text has every single-quoted scalar the edits did not leave in
/// place rewritten with double quotes, so additions match the file while
/// single-quoted scalars a user wrote stay as written.
#[derive(Debug, Default)]
pub(crate) enum AddedQuotes {
    /// Keep the single quotes edits render, for a document that does not
    /// mostly use double quotes: a new, unquoted, or single-quoted one, or
    /// one evenly split between the two.
    #[default]
    Single,
    /// Use double quotes, except for the single-quoted scalars the edits left
    /// in place. Holds the original document's scalars by source text, in
    /// document order, to tell those apart from added ones.
    Double { original: Vec<String> },
}

impl AddedQuotes {
    /// Judge the style from the document `original`, before any edit.
    pub(crate) fn detect(original: &str) -> Self {
        if detect_quote_style(original) == QuoteStyle::Single {
            return Self::Single;
        }
        let original = scalars(original)
            .into_iter()
            .map(|scalar| scalar.source.to_string())
            .collect();
        Self::Double { original }
    }

    /// The edited document `text`, with added scalars in the detected style.
    pub(crate) fn apply(self, text: String) -> String {
        let Self::Double { original } = self else {
            return text;
        };
        let original: Vec<&str> = original
            .iter()
            .map(String::as_str)
            .collect();
        let edited = scalars(&text);
        if edited.iter().all(|scalar| scalar.single_quoted.is_none()) {
            return text;
        }
        let sources: Vec<&str> = edited
            .iter()
            .map(|scalar| scalar.source)
            .collect();
        let kept = kept_from(&original, &sources);
        double_quote_added(&text, &edited, &kept)
    }
}

/// `text` with every single-quoted scalar of `edited` that `kept` does not
/// mark rewritten with double quotes.
fn double_quote_added(text: &str, edited: &[Scalar<'_>], kept: &[bool]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for (scalar, &kept) in edited.iter().zip(kept) {
        let Some(value) = scalar.single_quoted.as_deref().filter(|_| !kept) else {
            continue;
        };
        // Double quotes would need escapes here, so the scalar keeps its
        // single quotes.
        if value.contains(['"', '\\']) || value.chars().any(char::is_control) {
            continue;
        }
        out.push_str(&text[copied..scalar.range.start]);
        out.push('"');
        out.push_str(value);
        out.push('"');
        copied = scalar.range.end;
    }
    out.push_str(&text[copied..]);
    out
}

/// A scalar token as written in a document.
struct Scalar<'text> {
    range: Range<usize>,
    /// The token's source text, quotes included.
    source: &'text str,
    /// The decoded value, for a single-quoted scalar.
    single_quoted: Option<Cow<'text, str>>,
}

/// Every scalar in `text`, keys included, in document order.
fn scalars(text: &str) -> Vec<Scalar<'_>> {
    Scanner::new(StrInput::new(text))
        .map_while(Result::ok)
        .filter_map(|token| {
            let (span, token) = token.into_parts();
            let TokenType::Scalar(style, value) = token else {
                return None;
            };
            let range = span.start.byte_offset()?..span.end.byte_offset()?;
            let single_quoted = matches!(style, ScalarStyle::SingleQuoted).then_some(value);
            Some(Scalar { source: &text[range.clone()], range, single_quoted })
        })
        .collect()
}

/// Which items of `edited` a shortest edit script from `original` keeps:
/// the scalars an edit left in place, told apart by position rather than by
/// text alone. The common prefix and suffix are matched first, so only the
/// scalars between the first and last change are aligned. When that alignment
/// needs more than [`MAX_ALIGNED_EDITS`] edits, a scalar of the region counts
/// as kept if the original region holds the same source text.
fn kept_from(original: &[&str], edited: &[&str]) -> Vec<bool> {
    let prefix = common_len(original.iter(), edited.iter());
    let suffix = common_len(original[prefix..].iter().rev(), edited[prefix..].iter().rev());
    let old = &original[prefix..original.len() - suffix];
    let changed = prefix..edited.len() - suffix;
    let new = &edited[changed.clone()];
    // A changed region without single-quoted scalars has nothing to convert,
    // and against an empty original region every scalar in it was added.
    let matched = if !new
        .iter()
        .any(|source| source.starts_with('\''))
    {
        vec![true; new.len()]
    } else if old.is_empty() {
        vec![false; new.len()]
    } else {
        myers_matches(old, new).unwrap_or_else(|| present_in(old, new))
    };
    let mut kept = vec![true; edited.len()];
    kept[changed].copy_from_slice(&matched);
    kept
}

/// Which items of `new` also occur somewhere in `old`.
fn present_in(old: &[&str], new: &[&str]) -> Vec<bool> {
    let old: HashSet<&str> = old.iter().copied().collect();
    new.iter()
        .map(|source| old.contains(source))
        .collect()
}

/// How many leading items `left` and `right` share.
fn common_len<'item>(
    left: impl Iterator<Item = &'item &'item str>,
    right: impl Iterator<Item = &'item &'item str>,
) -> usize {
    left.zip(right)
        .take_while(|(left, right)| left == right)
        .count()
}

/// The most scalars an alignment may add or remove. Its trace holds one row
/// of `2 * MAX_ALIGNED_EDITS + 3` offsets per edit, so this bounds its memory
/// to about 16 MB.
const MAX_ALIGNED_EDITS: usize = 1000;

/// Which items of `new` Myers' shortest edit script from `old` keeps, or
/// `None` when the script needs more than [`MAX_ALIGNED_EDITS`] edits.
fn myers_matches(old: &[&str], new: &[&str]) -> Option<Vec<bool>> {
    let trace = myers_trace(old, new)?;
    Some(myers_backtrack(old, new, &trace))
}

/// The furthest-reaching offsets into `old` on each diagonal, recorded before
/// each round of Myers' greedy search, up to the round that reaches the end of
/// both sequences. Diagonal `k` (offset into `old` minus offset into `new`)
/// sits at index `k + center`, where `center` is half the row length.
fn myers_trace(old: &[&str], new: &[&str]) -> Option<Vec<Vec<usize>>> {
    let limit = (old.len() + new.len()).min(MAX_ALIGNED_EDITS) as isize;
    let center = limit + 1;
    let mut furthest = vec![0usize; (2 * center + 1) as usize];
    let mut trace = Vec::new();
    for edits in 0..=limit {
        trace.push(furthest.clone());
        for diagonal in (-edits..=edits).step_by(2) {
            let end = myers_step(old, new, &furthest, center, diagonal, edits);
            furthest[(center + diagonal) as usize] = end;
            if end >= old.len() && (end as isize - diagonal) as usize >= new.len() {
                return Some(trace);
            }
        }
    }
    None
}

/// The furthest offset into `old` that round `edits` reaches on `diagonal`:
/// one edit from a neighboring diagonal, then the run of matches after it.
fn myers_step(
    old: &[&str],
    new: &[&str],
    furthest: &[usize],
    center: isize,
    diagonal: isize,
    edits: isize,
) -> usize {
    let start = if comes_from_above(furthest, center, diagonal, edits) {
        furthest[(center + diagonal + 1) as usize]
    } else {
        furthest[(center + diagonal - 1) as usize] + 1
    };
    start + matching_run(old, new, start, (start as isize - diagonal) as usize)
}

/// Whether the best path to `diagonal` after `edits` edits ends with an
/// insertion from the diagonal above rather than a deletion from the one
/// below.
fn comes_from_above(furthest: &[usize], center: isize, diagonal: isize, edits: isize) -> bool {
    diagonal == -edits
        || (diagonal != edits
            && furthest[(center + diagonal - 1) as usize]
                < furthest[(center + diagonal + 1) as usize])
}

/// How many items `old` from `old_at` and `new` from `new_at` share.
fn matching_run(old: &[&str], new: &[&str], old_at: usize, new_at: usize) -> usize {
    let old = old.get(old_at..).unwrap_or_default();
    let new = new.get(new_at..).unwrap_or_default();
    common_len(old.iter(), new.iter())
}

/// Walk `trace` back from the end of both sequences, marking each item of
/// `new` a diagonal run matched.
fn myers_backtrack(old: &[&str], new: &[&str], trace: &[Vec<usize>]) -> Vec<bool> {
    let center = (trace[0].len() as isize - 1) / 2;
    let mut matched = vec![false; new.len()];
    let (mut old_at, mut new_at) = (old.len(), new.len());
    for (edits, furthest) in trace.iter().enumerate().skip(1).rev() {
        let edits = edits as isize;
        let diagonal = old_at as isize - new_at as isize;
        let previous = if comes_from_above(furthest, center, diagonal, edits) {
            diagonal + 1
        } else {
            diagonal - 1
        };
        let previous_old = furthest[(center + previous) as usize];
        let previous_new = (previous_old as isize - previous) as usize;
        let run = (old_at - previous_old).min(new_at - previous_new);
        matched[new_at - run..new_at].fill(true);
        (old_at, new_at) = (previous_old, previous_new);
    }
    // What is left before the first edit is a run both sequences share.
    matched[..new_at].fill(true);
    matched
}
