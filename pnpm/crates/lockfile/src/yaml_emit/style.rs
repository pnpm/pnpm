//! The rendering choices the emitter threads through every node.

/// Whether a collection may use YAML block style here. Every value nested
/// inside a flow collection stays flow, however deep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CollectionStyle {
    Block,
    Flow,
}

/// Where a block collection's first entry goes. `OnKeyLine` omits the
/// leading newline because the key the collection is bound to already
/// opened the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FirstEntry {
    OnKeyLine,
    OnOwnLine,
}

/// Whether a blank line separates the entries of a block mapping. The
/// lockfile separates its top-level sections and the entries of
/// `packages`, `importers` and `snapshots`; everything nested inside them
/// is tight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EntrySpacing {
    Tight,
    BlankLine,
}

/// Whether a scalar may use a style that spans lines. A key, and any value
/// inside a single-line mapping, has nowhere to put a line break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScalarLines {
    OneLine,
    Multiline,
}

impl ScalarLines {
    /// The lines a mapping's keys and values may use, given whether the
    /// mapping itself renders on one line.
    pub(super) fn of_mapping(single_line: bool) -> Self {
        if single_line { ScalarLines::OneLine } else { ScalarLines::Multiline }
    }
}
