//! Value rendering and key-ordering helpers ported from pnpm's writer.
//!
//! pnpm renders new scalars with eemeli/yaml's `singleQuote: true`: a value
//! that *needs* quoting uses single quotes, plain-safe values stay unquoted
//! (`zoo: 4.0.0`, `newPkg: ^2.0.0`). [`yaml_serde`] reproduces that exact
//! policy (`>=2.0.0` → `'>=2.0.0'`, `@scope/x` → `'@scope/x'`, otherwise
//! plain), so value text is delegated to it rather than re-derived. Key
//! ordering reuses
//! [`detectKeyLayout`](https://github.com/pnpm/pnpm/blob/e7e99f04e4/workspace/workspace-manifest-writer/src/index.ts#L317-L325)
//! and [`sortKeys`](https://github.com/pnpm/pnpm/blob/e7e99f04e4/workspace/workspace-manifest-writer/src/index.ts#L327-L332).

use std::cmp::Ordering;

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

/// pnpm's `lexCompare`: a plain code-unit comparison. Rust `str::cmp`
/// compares by Unicode scalar, which matches for the BMP identifiers used as
/// catalog/field keys.
fn lex_cmp(left: &str, right: &str) -> Ordering {
    left.cmp(right)
}

/// Classify `keys`, mirroring pnpm's `detectKeyLayout`. Empty input is
/// `PackagesFirst` (pnpm's convention for brand-new manifests).
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
/// mirroring pnpm's `reorderRecursive` for a single level: existing order is
/// preserved when no key is added; otherwise the merged set is re-sorted for a
/// sorted layout, or new keys are appended for an unordered one.
pub(crate) fn target_order(existing: &[String], new_keys: &[String]) -> Vec<String> {
    if new_keys.is_empty() {
        return existing.to_vec();
    }
    let layout = detect_key_layout(existing);
    let mut merged: Vec<String> = existing.iter().chain(new_keys).cloned().collect();
    if layout != Layout::Unordered {
        sort_keys(&mut merged, layout);
    }
    merged
}

/// Render a scalar string value the way pnpm's writer does — plain when safe,
/// single-quoted otherwise — by delegating to [`yaml_serde`].
pub(crate) fn render_value(value: &str) -> String {
    yaml_serde::to_string(&yaml_serde::Value::from(value))
        .expect("serializing a string scalar to YAML never fails")
        .trim_end()
        .to_string()
}
