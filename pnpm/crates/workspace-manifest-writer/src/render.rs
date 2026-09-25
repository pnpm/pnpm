//! Value rendering and key-ordering helpers for the manifest writer.
//!
//! New scalars are rendered so that a value that *needs* quoting uses single
//! quotes while plain-safe values stay unquoted (`zoo: 4.0.0`,
//! `newPkg: ^2.0.0`). [`yaml_serde`] implements that exact policy
//! (`>=2.0.0` → `'>=2.0.0'`, `@scope/x` → `'@scope/x'`, otherwise plain), so
//! value text is delegated to it rather than re-derived. Key ordering is
//! handled by [`detect_key_layout`] and [`sort_keys`].

use std::cmp::Ordering;

use serde_saphyr::granit_parser::{ScalarStyle, Scanner, StrInput, Token, TokenType};

/// The quoting style for string scalars that require quotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuoteStyle {
    Single,
    Double,
}

/// How a map's existing keys were laid out, used to place new keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// Not sorted — new keys append at the end.
    Unordered,
    /// Alphabetical (lexicographic) — new keys sort in.
    Alphabetical,
    /// A leading `packages` key, then alphabetical — `packages` stays first.
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

    for Token(_span, token) in Scanner::new(StrInput::new(text)) {
        match token {
            TokenType::Scalar(ScalarStyle::SingleQuoted, _) => single_count += 1,
            TokenType::Scalar(ScalarStyle::DoubleQuoted, _) => double_count += 1,
            _ => {}
        }
    }

    if double_count > single_count { QuoteStyle::Double } else { QuoteStyle::Single }
}

/// Render a scalar string value — plain when safe, single-quoted otherwise —
/// by delegating to [`yaml_serde`].
pub(crate) fn render_value(value: &str) -> String {
    yaml_serde::to_string(&yaml_serde::Value::from(value))
        .expect("serializing a string scalar to YAML never fails")
        .trim_end()
        .to_string()
}

/// Render a scalar string value using the specified [`QuoteStyle`].
///
/// Plain-safe values stay unquoted. Values that require quoting use double
/// quotes when `quote_style` is [`QuoteStyle::Double`], and single quotes
/// when [`QuoteStyle::Single`].
pub(crate) fn render_value_with_quotes(value: &str, quote_style: QuoteStyle) -> String {
    let rendered = render_value(value);
    match quote_style {
        QuoteStyle::Single => rendered,
        QuoteStyle::Double => {
            if rendered.starts_with('\'') && rendered.ends_with('\'') && rendered.len() >= 2 {
                serde_json::to_string(value)
                    .expect("serializing a string scalar to JSON never fails")
            } else {
                rendered
            }
        }
    }
}
