//! What the emitter threads from a map to its values: the document's
//! format, and whether a value is written on its key's line.

/// Keys whose collection value always renders on a single line (flow style).
/// Mirrors the fork's [`SINGLE_LINE_KEYS`][fork-single-line-keys].
///
/// [fork-single-line-keys]: https://cdn.jsdelivr.net/npm/@zkochan/js-yaml@0.0.11/lib/dumper.js
const SINGLE_LINE_KEYS: [&str; 4] = ["cpu", "engines", "os", "libc"];

pub(super) fn is_single_line_key(key: &str) -> bool {
    SINGLE_LINE_KEYS.contains(&key)
}

/// Whether a map value renders on a single line. `resolution` is single-line
/// except for the nested `variations`/`binary` shapes (detected by the `type`
/// discriminator pnpm's tagged resolutions carry).
pub(super) fn is_single_line_map(
    object_key: Option<&str>,
    map: &serde_json::Map<String, super::Value>,
) -> bool {
    match object_key {
        Some(key) if is_single_line_key(key) => true,
        Some("resolution") => {
            !matches!(map.get("type").and_then(super::Value::as_str), Some("variations" | "binary"))
        }
        _ => false,
    }
}

/// Which lockfile format the document is in. The next format writes some
/// nested maps on one line that today's format writes in block style.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Format {
    V9,
    Next,
}

/// What a value inherits from the map it is in.
#[derive(Clone, Copy)]
pub(super) struct Nesting {
    pub format: Format,
    pub values: ValueStyle,
}

/// How a block mapping's values render.
#[derive(Clone, Copy)]
pub(super) enum ValueStyle {
    Block,
    /// Each value that is a map is written on its key's line.
    Flow,
    /// Each value is a block map whose own values are written on their
    /// keys' lines.
    FlowBelow,
}

/// The maps the next lockfile format writes on their keys' lines: a
/// `peerDependencies` entry, which is the peer's `specifier`, `version`,
/// `optional` and `below`; an importer's dependency, which is its
/// `specifier` and `version`; and a catalog entry, the same pair. A map
/// among them that [names a snapshot](names_snapshot) is written in block
/// style instead. Today's format writes a `peerDependencies` entry as a
/// string, so that rule applies to both.
pub(super) fn one_line_values(object_key: Option<&str>, nesting: Nesting) -> ValueStyle {
    match (object_key, nesting.format, nesting.values) {
        (Some("peerDependencies"), _, _) => ValueStyle::Flow,
        (Some("dependencies" | "devDependencies" | "optionalDependencies"), Format::Next, _) => {
            ValueStyle::Flow
        }
        (Some("catalogs"), Format::Next, _) => ValueStyle::FlowBelow,
        (_, _, ValueStyle::FlowBelow) => ValueStyle::Flow,
        _ => ValueStyle::Block,
    }
}

/// Whether a map carries `with`, the bindings that name a snapshot. Such a
/// map is written in block style, one binding per line, where its siblings
/// are written on their keys' lines: its `version` and its bindings change
/// for different reasons, and on separate lines two branches that change
/// one each merge without a conflict.
pub(super) fn names_snapshot(value: &super::Value) -> bool {
    matches!(value, super::Value::Object(map) if map.contains_key("with"))
}
