//! What the emitter threads from a map to its values: the document's
//! format, and whether a value is written on its key's line.

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
/// `peerDependencies` entry, which is the peer's `specifier` and `optional`;
/// an importer's dependency, which is its `specifier` and `version`; and a
/// catalog entry, the same pair. Today's format writes a `peerDependencies`
/// entry as a string, so that rule applies to both.
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
