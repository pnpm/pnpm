use super::flow;

/// How the value a key path names is written.
pub(crate) enum Inline {
    /// A block body on the following lines — or no such key at all. The
    /// line-oriented splices in this module apply.
    Block,
    /// An inline value none of the splices can edit: a multi-line flow
    /// collection (whose interleaved comments a rebuild would drop), an
    /// alias, or a scalar where a collection belongs.
    Unsupported,
    /// A flow collection written on one line, edited by [`crate::flow`].
    Flow(flow::Collection),
}

/// Classify how the mapping at `path` is written.
pub(super) fn locate_mapping(text: &str, path: &[&str]) -> Inline {
    locate_inline(text, path, flow::Kind::Mapping)
}

/// Classify how the sequence at `path` is written.
pub(super) fn locate_sequence(text: &str, path: &[&str]) -> Inline {
    locate_inline(text, path, flow::Kind::Sequence)
}

/// Classify how the value of `path` is written. A flow collection of a kind
/// other than `expected` is reported as unsupported: no writer here can put
/// a mapping entry into a sequence, or the reverse.
fn locate_inline(text: &str, path: &[&str], expected: flow::Kind) -> Inline {
    let Some(offset) = inline_value_start(text, path) else { return Inline::Block };
    match flow::parse(text, offset) {
        Some(collection) if collection.kind() == expected => Inline::Flow(collection),
        Some(_) | None => Inline::Unsupported,
    }
}

/// Whether the value at `path` is an inline shape no writer can edit. A
/// caller refuses the whole write rather than corrupt it.
pub(crate) fn has_unsupported_inline_value(text: &str, path: &[&str]) -> bool {
    document_root_is_inline(text)
        || (matches!(locate_mapping(text, path), Inline::Unsupported)
            && matches!(locate_sequence(text, path), Inline::Unsupported))
}

/// Whether the document itself is written as a flow collection
/// (`{ overrides: { foo: 1.0.0 } }`). Its keys are then not top-level lines
/// at all, so neither the splices here nor a new top-level block can
/// address them.
pub(crate) fn document_root_is_inline(text: &str) -> bool {
    text.lines()
        .find(|line| structural_indent(line).is_some())
        .is_some_and(|line| line.trim_start().starts_with(['{', '[']))
}

/// The keys of the mapping at `path`, whether it is written in block or
/// single-line flow style. Empty for a mapping with no entries, an
/// unsupported inline value, or an absent key.
pub(super) fn mapping_keys(text: &str, path: &[&str]) -> Vec<String> {
    match locate_mapping(text, path) {
        Inline::Flow(collection) => collection.keys(),
        Inline::Unsupported => Vec::new(),
        Inline::Block => locate(text, path)
            .map(|mapping| mapping.entries.into_iter().map(|entry| entry.key).collect())
            .unwrap_or_default(),
    }
}

/// Byte offset of the value of the key `path` names, when that value sits
/// on the key's own line. `None` when the key is absent or its value is a
/// block body on the lines below.
fn inline_value_start(text: &str, path: &[&str]) -> Option<usize> {
    let (key, parent) = path.split_last()?;
    if parent.is_empty() {
        let span = top_level_span(text, key)?;
        return inline_value_on_line(text, span.key_line_start);
    }
    if let Some(entry) = locate(text, parent)
        .and_then(|mapping| mapping.entries.into_iter().find(|entry| entry.key == *key))
    {
        return inline_value_on_line(text, entry.line_start);
    }
    // The parent has no line entries of its own when it is itself written
    // inline, and then the key lives among its flow entries.
    match locate_mapping(text, parent) {
        Inline::Flow(collection) => collection.value_start(key),
        Inline::Block | Inline::Unsupported => None,
    }
}

/// Byte offset of the value written after the `key:` on the line starting
/// at `line_start`. `None` when the line carries no value (a bare `key:`,
/// optionally followed by a comment), which makes it block style.
fn inline_value_on_line(text: &str, line_start: usize) -> Option<usize> {
    let line_end = text[line_start..].find('\n').map_or(text.len(), |offset| line_start + offset);
    let content = &text[line_start..line_end];
    let indent = content.len() - content.trim_start().len();
    let colon = indent + structural_colon_index(&content[indent..])?;
    let after = &content[colon + 1..];
    let value = after.trim_start();
    if value.is_empty() || value.starts_with('#') {
        return None;
    }
    Some(line_start + colon + 1 + (after.len() - value.len()))
}

// ---------------------------------------------------------------------------
// Line-oriented scanning of the block-style YAML pnpm writes.
// ---------------------------------------------------------------------------

/// A located mapping and its direct child entries.
pub(super) struct Mapping {
    /// Byte offset where the mapping's body (its child lines) begins.
    pub(super) body_start: usize,
    /// Indentation (in spaces) of the mapping's direct child entries.
    pub(super) entry_indent: usize,
    /// Direct child key lines, in document order.
    pub(super) entries: Vec<EntryPos>,
}

/// One direct child entry of a mapping.
pub(super) struct EntryPos {
    pub(super) key: String,
    /// Byte offset where this entry's line begins.
    pub(super) line_start: usize,
    /// Byte offset just past this entry's line (after its newline).
    pub(super) line_end: usize,
    /// Byte offset where this entry's whole sub-block ends (for nested maps).
    pub(super) block_end: usize,
}

/// Span of a top-level block keyed by `key`.
pub(super) struct TopLevelSpan {
    pub(super) key_line_start: usize,
    pub(super) block_end: usize,
}

pub(super) struct Line<'a> {
    pub(super) start: usize,
    /// Content without the trailing newline.
    pub(super) content: &'a str,
    /// Byte offset just past the line, including its newline.
    pub(super) end: usize,
}

pub(super) fn lines(text: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut offset = 0;
    for raw in text.split_inclusive('\n') {
        let content = raw.strip_suffix('\n').unwrap_or(raw);
        out.push(Line { start: offset, content, end: offset + raw.len() });
        offset += raw.len();
    }
    out
}

/// Indentation of a structural line, or `None` for blank and comment lines
/// (which don't terminate a block and aren't entries).
pub(super) fn structural_indent(content: &str) -> Option<usize> {
    let indent = content.len() - content.trim_start().len();
    let rest = &content[indent..];
    if rest.is_empty() || rest.starts_with('#') {
        return None;
    }
    Some(indent)
}

/// The mapping-key a structural line declares (`key:` or `key: value`), if any.
///
/// The key/value delimiter is the first `:` that ends the line or is followed
/// by whitespace — a `:` inside the value, or inside a key (quoted or not,
/// e.g. an `allowBuilds` artifact key like `foo@https://example.com/foo.tgz`),
/// is not the delimiter. Splitting on the first `:` would truncate such keys.
fn line_key(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    let delimiter = structural_colon_index(trimmed)?;
    let key = trimmed[..delimiter].trim_end();
    if key.is_empty() {
        return None;
    }
    Some(strip_quotes(key))
}

/// Byte offset of the YAML key/value delimiter in `line`: the first `:` that
/// ends the line or is followed by whitespace. A `:` inside a value or key
/// (e.g. `foo@https://...`) is not the delimiter.
pub(super) fn structural_colon_index(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    (0..bytes.len())
        .find(|&idx| bytes[idx] == b':' && bytes.get(idx + 1).is_none_or(u8::is_ascii_whitespace))
}

/// Byte offset of a value's trailing comment, if it has one.
///
/// A `#` opens a comment only when whitespace precedes it and it sits
/// outside a quoted scalar, so neither a `#` within the value nor one in
/// a quoted string is mistaken for a comment.
///
/// A quote delimits a scalar only when it opens the value: YAML has no
/// way to start quoting partway through, so `don't` is a plain scalar
/// holding an apostrophe, not an unterminated quote.
pub(super) fn comment_start(value: &str) -> Option<usize> {
    let scan_from = match value.as_bytes().first() {
        Some(&quote @ (b'"' | b'\'')) => closing_quote(value, quote)? + 1,
        _ => 0,
    };
    let bytes = value.as_bytes();
    (scan_from..bytes.len())
        .find(|&idx| bytes[idx] == b'#' && idx > 0 && bytes[idx - 1].is_ascii_whitespace())
}

/// Byte offset of the quote closing the scalar `value` opens with.
/// `None` when it is never closed, which leaves the value unparsable —
/// the caller then treats the whole of it as the value rather than
/// guessing where a comment might start.
///
/// Escaping differs by quote style: a double-quoted scalar escapes with
/// `\`, a single-quoted one by doubling the quote.
fn closing_quote(value: &str, quote: u8) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut idx = 1;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\\' if quote == b'"' => idx += 2,
            byte if byte == quote => {
                if quote == b'\'' && bytes.get(idx + 1) == Some(&quote) {
                    idx += 2;
                } else {
                    return Some(idx);
                }
            }
            _ => idx += 1,
        }
    }
    None
}

fn strip_quotes(key: &str) -> String {
    let bytes = key.as_bytes();
    if key.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        key[1..key.len() - 1].to_string()
    } else {
        key.to_string()
    }
}

/// Locate the mapping reached by following `path` from the document root.
pub(super) fn locate(text: &str, path: &[&str]) -> Option<Mapping> {
    let all = lines(text);
    let mut lo = 0usize;
    let mut hi = all.len();
    let mut base_indent = 0usize;

    for (depth, segment) in path.iter().enumerate() {
        let key_idx = (lo..hi).find(|&idx| {
            structural_indent(all[idx].content) == Some(base_indent)
                && line_key(all[idx].content).as_deref() == Some(*segment)
        })?;
        // The block ends at the next structural line indented at or below
        // `base_indent`.
        let block_end_idx = block_end(&all, key_idx, hi, base_indent);
        let child_indent = child_indent(&all, key_idx, block_end_idx, base_indent);

        if depth + 1 == path.len() {
            return Some(mapping_at(&all, key_idx, block_end_idx, child_indent));
        }

        lo = key_idx + 1;
        hi = block_end_idx;
        base_indent = child_indent;
    }
    None
}

pub(super) fn block_end(all: &[Line<'_>], key_idx: usize, hi: usize, base_indent: usize) -> usize {
    ((key_idx + 1)..hi)
        .find(|&idx| {
            structural_indent(all[idx].content).is_some_and(|indent| indent <= base_indent)
        })
        .unwrap_or(hi)
}

/// The child indent is whatever the block's first structural line
/// uses, not a hard-coded two spaces — so a manifest written with a
/// wider indent is still traversed correctly.
fn child_indent(
    all: &[Line<'_>],
    key_idx: usize,
    block_end_idx: usize,
    base_indent: usize,
) -> usize {
    (key_idx + 1..block_end_idx)
        .find_map(|idx| structural_indent(all[idx].content))
        .unwrap_or(base_indent + 2)
}

fn mapping_at(
    all: &[Line<'_>],
    key_idx: usize,
    block_end_idx: usize,
    child_indent: usize,
) -> Mapping {
    let body_start = all.get(key_idx + 1).map_or(all[key_idx].end, |line| line.start);
    let entries = collect_entries(all, key_idx + 1, block_end_idx, child_indent);
    Mapping { body_start, entry_indent: child_indent, entries }
}

/// Collect the direct child entries (key lines at `entry_indent`) within
/// `[from, to)`, recording where each entry's own sub-block ends.
fn collect_entries(all: &[Line<'_>], from: usize, to: usize, entry_indent: usize) -> Vec<EntryPos> {
    let mut entries = Vec::new();
    let mut idx = from;
    while idx < to {
        if structural_indent(all[idx].content) != Some(entry_indent) {
            idx += 1;
            continue;
        }
        let Some(key) = line_key(all[idx].content) else {
            idx += 1;
            continue;
        };
        let block_end_idx = ((idx + 1)..to)
            .find(|&next| {
                structural_indent(all[next].content).is_some_and(|indent| indent <= entry_indent)
            })
            .unwrap_or(to);
        let block_end = all.get(block_end_idx).map_or(all[to - 1].end, |line| line.start);
        entries.push(EntryPos {
            key,
            line_start: all[idx].start,
            line_end: all[idx].end,
            block_end,
        });
        idx = block_end_idx;
    }
    entries
}

/// The starting offset of a top-level key's line.
pub(super) fn top_level_span(text: &str, key: &str) -> Option<TopLevelSpan> {
    let all = lines(text);
    let key_idx = top_level_key_line(&all, key)?;
    // A flow collection written across several lines closes at column zero,
    // which would otherwise read as the next top-level key and leave the
    // closing bracket behind when the block is replaced or removed.
    let body_start = inline_value_last_line(text, &all, key_idx).unwrap_or(key_idx) + 1;
    let next_key_idx = (body_start..all.len())
        .find(|&idx| structural_indent(all[idx].content) == Some(0))
        .unwrap_or(all.len());
    let block_end_idx = leading_comment_start(&all, body_start, next_key_idx);
    let block_end = all
        .get(block_end_idx)
        .map_or_else(|| all.last().map_or(0, |line| line.end), |line| line.start);
    Some(TopLevelSpan { key_line_start: all[key_idx].start, block_end })
}

/// Index of the line declaring the top-level key `key`.
pub(super) fn top_level_key_line(all: &[Line<'_>], key: &str) -> Option<usize> {
    all.iter().position(|line| {
        structural_indent(line.content) == Some(0) && line_key(line.content).as_deref() == Some(key)
    })
}

/// Index of the line where the flow collection written inline on
/// `key_idx`'s line closes. `None` when that line carries no inline value,
/// its value is not a flow collection, or the collection never closes.
fn inline_value_last_line(text: &str, all: &[Line<'_>], key_idx: usize) -> Option<usize> {
    let open = inline_value_on_line(text, all[key_idx].start)?;
    if !text[open..].starts_with(['{', '[']) {
        return None;
    }
    let close = flow::closing_bracket_across_lines(text, open)?;
    all.iter().position(|line| line.start <= close && close < line.end)
}

pub(super) fn leading_comment_start(
    all: &[Line<'_>],
    block_start: usize,
    next_key_idx: usize,
) -> usize {
    if next_key_idx == all.len() {
        return next_key_idx;
    }
    let mut idx = next_key_idx;
    while idx > block_start && is_comment_line(all[idx - 1].content) {
        idx -= 1;
    }
    idx
}

fn is_comment_line(content: &str) -> bool {
    let trimmed = content.trim_start();
    !trimmed.is_empty() && trimmed.starts_with('#')
}
