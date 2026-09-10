use super::{
    HashMap, Inline, Line, Range, VecDeque, blank_run_start, flow, insertion_offset,
    leading_comment_start, lines, locate, locate_mapping, locate_sequence, render, splice,
    structural_indent, top_level_key_line,
};

/// Line-level reconciliation of the block sequence `key` toward `items`,
/// mirroring the TypeScript writer's node reuse: an entry whose value the
/// list already holds keeps its lines verbatim — comments included — a value
/// new to the list is rendered as a fresh line, and lines no entry claims are
/// dropped, where a whole-block re-render would drop every comment in the
/// block. `None` when the block is not a plain block sequence matching
/// `current` (no items on disk, inline or multi-line flow style, an item
/// count the decoded list disagrees with, or an entry whose value runs past
/// its own line), leaving the caller to fall back to the re-render.
pub(super) fn reconcile_sequence_items(
    text: &str,
    key: &str,
    current: &[String],
    items: &[String],
) -> Option<String> {
    let layout = item_layout(text, key, current)?;
    let body = rebuild_items(text, &layout, current, items);
    let mut out = text.to_string();
    out.replace_range(layout.spans.first()?.0..layout.spans.last()?.1, &body);
    Some(out)
}

/// Where a block sequence's items sit in the document, and how an item added
/// to it has to be written to match them.
struct ItemLayout {
    /// One span per item, in document order. A span opens at the comment and
    /// blank lines ahead of its item — the TypeScript writer attaches those
    /// to the entry below, so pruning the entry above must leave them be —
    /// and closes where the next span opens. The first span opens at its own
    /// line, since comments between the key and the list belong to no entry,
    /// and the last closes before the blank run that separates the block from
    /// what follows it.
    spans: Vec<(usize, usize)>,
    /// Indentation of the item lines.
    indent: usize,
    /// The line ending the block is written with.
    newline: &'static str,
}

/// The [`ItemLayout`] of the top-level block sequence `key`. `None` unless
/// the block holds one `- item` line per entry of `current`, each carrying
/// its whole value.
fn item_layout(text: &str, key: &str, current: &[String]) -> Option<ItemLayout> {
    let all = lines(text);
    let key_idx = top_level_key_line(&all, key)?;
    let block_end_idx = (key_idx + 1..all.len())
        .find(|&idx| structural_indent(all[idx].content) == Some(0))
        .unwrap_or(all.len());
    let (indent, item_idxs) = item_lines(&all, key_idx + 1..block_end_idx, current)?;
    let block_items_end = blank_run_start(
        text,
        all.get(leading_comment_start(&all, key_idx + 1, block_end_idx))
            .map_or(text.len(), |line| line.start),
    );
    let starts: Vec<usize> =
        item_idxs
            .iter()
            .enumerate()
            .map(|(position, &idx)| {
                if position == 0 { all[idx].start } else { all[comment_run_start(&all, idx)].start }
            })
            .collect();
    Some(ItemLayout {
        spans: starts
            .iter()
            .enumerate()
            .map(|(position, &start)| {
                (start, starts.get(position + 1).copied().unwrap_or(block_items_end))
            })
            .collect(),
        indent,
        newline: if text[all[key_idx].start..block_items_end].contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        },
    })
}

/// The indentation of `body`'s block-sequence item lines and their indices,
/// paired one to one with `current`. `None` unless every entry of `current`
/// is one whole `- item` line, since anything else breaks the pairing or
/// leaves part of a value where the span logic would read comments.
fn item_lines(
    all: &[Line<'_>],
    body: Range<usize>,
    current: &[String],
) -> Option<(usize, Vec<usize>)> {
    let indent = body.clone().find_map(|idx| structural_indent(all[idx].content))?;
    let item_idxs: Vec<usize> = body
        .filter(|&idx| {
            structural_indent(all[idx].content) == Some(indent)
                && is_sequence_item_line(all[idx].content)
        })
        .collect();
    if item_idxs.is_empty() || item_idxs.len() != current.len() {
        return None;
    }
    item_idxs
        .iter()
        .zip(current)
        .all(|(&idx, entry)| holds_whole_value(all[idx].content, entry))
        .then_some((indent, item_idxs))
}

/// The item lines of `layout` rebuilt as `items`: an entry whose value
/// `current` already holds is copied out of `text` with the comments its span
/// carries, and every other entry is rendered afresh.
fn rebuild_items(text: &str, layout: &ItemLayout, current: &[String], items: &[String]) -> String {
    // First-come claim of each surviving entry's lines, like the TypeScript
    // writer's node reuse: duplicate values claim their lines in order.
    let mut unclaimed: HashMap<&str, VecDeque<usize>> = HashMap::with_capacity(current.len());
    for (idx, value) in current.iter().enumerate() {
        unclaimed.entry(value.as_str()).or_default().push_back(idx);
    }
    let indent = " ".repeat(layout.indent);
    let mut body = String::new();
    for item in items {
        if let Some(idx) = unclaimed.get_mut(item.as_str()).and_then(VecDeque::pop_front) {
            body.push_str(&text[layout.spans[idx].0..layout.spans[idx].1]);
        } else {
            body.push_str(&indent);
            body.push_str("- ");
            body.push_str(&render::render_value(item));
        }
        // A document that ends without a newline leaves its last span without
        // one, which would splice the entry after it onto that same line.
        if !body.ends_with('\n') {
            body.push_str(layout.newline);
        }
    }
    body
}

/// Whether a structural line carries a block-sequence item (`- value`).
fn is_sequence_item_line(content: &str) -> bool {
    let trimmed = content.trim_start();
    trimmed == "-" || trimmed.starts_with("- ")
}

/// Whether the sequence-item line `content` carries the whole of `entry`.
///
/// A value that runs past its item line — a block scalar's body, a quoted
/// scalar broken across lines — can hold blank and `#`-leading lines, which
/// the span logic would take for the comments of the entry below and drop
/// with it, silently rewriting this entry's value. A `trustPolicyExclude`
/// entry truncated that way can widen into the bare `*` that excludes every
/// package. Parsing the line on its own settles it: a value the line does not
/// finish parses to something else, or not at all.
fn holds_whole_value(content: &str, entry: &str) -> bool {
    let Some(value) = content.trim_start().strip_prefix('-') else {
        return false;
    };
    yaml_serde::from_str::<String>(value).is_ok_and(|parsed| parsed == entry)
}

/// The first line of the run of comment and blank lines immediately ahead of
/// the item line at `idx`: the run belongs to the entry below it, so it opens
/// that entry's span. Only valid while a structural line sits above the run —
/// the previous item, in the one caller.
fn comment_run_start(all: &[Line<'_>], idx: usize) -> usize {
    let mut start = idx;
    while structural_indent(all[start - 1].content).is_none() {
        start -= 1;
    }
    start
}

/// Render a top-level block whose value is a block sequence (`key:` then
/// `  - item` lines).
pub(super) fn render_top_level_sequence(key: &str, items: &[String]) -> String {
    let mut block = String::new();
    block.push_str(key);
    block.push_str(":\n");
    for item in items {
        block.push_str("  - ");
        block.push_str(&render::render_value(item));
        block.push('\n');
    }
    block
}

/// Upsert a `key:` entry whose value is a block sequence (`items`) into the
/// existing top-level mapping `block_name`, creating or replacing the entry
/// in the position the reorder pass would choose. The mapping at
/// `block_name` must already exist. Used to write `auditConfig.ignoreGhsas`.
pub(super) fn upsert_sequence_entry(
    text: &str,
    block_name: &str,
    key: &str,
    items: &[String],
) -> String {
    let rendered_items: Vec<String> = items.iter().map(|item| render::render_value(item)).collect();
    if let Inline::Flow(collection) = locate_sequence(text, &[block_name, key]) {
        return flow::set_items(text, &collection, &rendered_items);
    }
    if let Inline::Flow(collection) = locate_mapping(text, &[block_name]) {
        return flow::upsert(text, &collection, key, &flow::render_sequence(&rendered_items));
    }
    let mapping = locate(text, &[block_name]).expect("block exists");
    let rendered = render_block_sequence_entry(mapping.entry_indent, key, items);

    if let Some(entry) = mapping.entries.iter().find(|entry| entry.key == key) {
        let mut out = text.to_string();
        out.replace_range(entry.line_start..entry.block_end, &rendered);
        return out;
    }
    splice(text, insertion_offset(&mapping, key), &rendered)
}

fn render_block_sequence_entry(entry_indent: usize, key: &str, items: &[String]) -> String {
    let item_indent = entry_indent + 2;
    let mut rendered = String::new();
    rendered.push_str(&" ".repeat(entry_indent));
    rendered.push_str(&render::render_value(key));
    rendered.push_str(":\n");
    for item in items {
        rendered.push_str(&" ".repeat(item_indent));
        rendered.push_str("- ");
        rendered.push_str(&render::render_value(item));
        rendered.push('\n');
    }
    rendered
}
