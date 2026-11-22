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
    collections::HashMap,
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
/// written text has every single-quoted scalar the original did not already
/// hold rewritten with double quotes, so additions match the file while
/// single-quoted scalars a user wrote stay as written.
#[derive(Debug, Default)]
pub(crate) enum AddedQuotes {
    /// Keep the single quotes edits render, for a document that does not
    /// mostly use double quotes: a new, unquoted, or single-quoted one, or
    /// one evenly split between the two.
    #[default]
    Single,
    /// Use double quotes, except for the single-quoted scalars the original
    /// document held, counted by their source text.
    Double { existing_single: HashMap<String, usize> },
}

impl AddedQuotes {
    /// Judge the style from the document `original`, before any edit.
    pub(crate) fn detect(original: &str) -> Self {
        if detect_quote_style(original) == QuoteStyle::Single {
            return Self::Single;
        }
        let mut existing_single = HashMap::new();
        for (range, _) in single_quoted_scalars(original) {
            let source = original[range].to_string();
            *existing_single.entry(source).or_default() += 1;
        }
        Self::Double { existing_single }
    }

    /// The edited document `text`, with added scalars in the detected style.
    pub(crate) fn apply(self, text: String) -> String {
        let Self::Double { mut existing_single } = self else {
            return text;
        };
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        for (range, value) in single_quoted_scalars(&text) {
            if let Some(count) = existing_single.get_mut(&text[range.clone()])
                && *count > 0
            {
                *count -= 1;
                continue;
            }
            // Double quotes would need escapes here, so the scalar keeps its
            // single quotes.
            if value.contains(['"', '\\']) || value.chars().any(char::is_control) {
                continue;
            }
            out.push_str(&text[copied..range.start]);
            out.push('"');
            out.push_str(&value);
            out.push('"');
            copied = range.end;
        }
        out.push_str(&text[copied..]);
        out
    }
}

/// The byte range and decoded value of each single-quoted scalar in `text`,
/// keys included.
fn single_quoted_scalars(text: &str) -> Vec<(Range<usize>, Cow<'_, str>)> {
    Scanner::new(StrInput::new(text))
        .map_while(Result::ok)
        .filter_map(|token| {
            let (span, token) = token.into_parts();
            match token {
                TokenType::Scalar(ScalarStyle::SingleQuoted, value) => {
                    Some((span.start.byte_offset()?..span.end.byte_offset()?, value))
                }
                _ => None,
            }
        })
        .collect()
}
