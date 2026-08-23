//! Format-preserving edits of single-line flow collections.
//!
//! `pnpm-workspace.yaml` is normally block style, and the line-oriented
//! splices in [`super::edit`] assume that shape. A hand-written flow value
//! (`overrides: { foo: 1.0.0 }`, `ignoreGhsas: [GHSA-1]`) has no line
//! entries for them to work with, so it is edited here instead: the
//! collection is rebuilt from its own entry texts with one entry inserted,
//! replaced, or dropped. Every untouched entry keeps its text (and so its
//! quoting), and everything outside the brackets — indentation, a trailing
//! comment — stays verbatim, which is what the TypeScript writer's yaml
//! library emits for the same edit.
//!
//! Only collections written on one line are edited. A flow collection
//! spanning several lines can carry comments between its entries, which
//! rebuilding onto one line would drop, so [`parse`] rejects it and the
//! caller refuses the write rather than silently discarding them.

use std::ops::Range;

/// Which brackets a flow collection uses.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Mapping,
    Sequence,
}

/// A parsed single-line flow collection.
pub(crate) struct Collection {
    kind: Kind,
    /// Byte range of the collection, brackets included.
    span: Range<usize>,
    entries: Vec<Entry>,
}

/// One entry of a flow collection.
struct Entry {
    /// Decoded key, for a mapping entry.
    key: Option<String>,
    /// Byte range of the entry's text, without the separating commas or the
    /// whitespace around it.
    span: Range<usize>,
    /// Byte range of a mapping entry's value text.
    value: Range<usize>,
}

impl Collection {
    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }

    /// The entry keys, in document order. Empty for a sequence.
    pub(crate) fn keys(&self) -> Vec<String> {
        self.entries.iter().filter_map(|entry| entry.key.clone()).collect()
    }

    fn position_of(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|entry| entry.key.as_deref() == Some(key))
    }

    /// Byte offset where `key`'s value starts, for descending into a
    /// collection nested in this one.
    pub(crate) fn value_start(&self, key: &str) -> Option<usize> {
        self.position_of(key).map(|position| self.entries[position].value.start)
    }
}

/// Parse the flow collection opening at `open` (a `{` or `[` byte).
/// `None` when it is not one this module can edit: a multi-line
/// collection, an unterminated one, or one holding an entry shape the
/// rebuild cannot reproduce (an empty entry, a mapping entry without a
/// value, an explicit `? key` entry).
pub(crate) fn parse(text: &str, open: usize) -> Option<Collection> {
    let kind = match text.as_bytes().get(open)? {
        b'{' => Kind::Mapping,
        b'[' => Kind::Sequence,
        _ => return None,
    };
    let close = closing_bracket(text, open)?;
    let entries = split_entries(text, open + 1..close)?
        .into_iter()
        .map(|span| entry(text, span, kind))
        .collect::<Option<Vec<Entry>>>()?;
    Some(Collection { kind, span: open..close + 1, entries })
}

/// Replace `key`'s value, or add the entry at the position the reorder pass
/// would choose, and return the document with the rebuilt collection
/// spliced in. `value_text` is already-rendered YAML.
pub(crate) fn upsert(text: &str, collection: &Collection, key: &str, value_text: &str) -> String {
    let mut entries: Vec<String> =
        collection.entries.iter().map(|entry| text[entry.span.clone()].to_string()).collect();
    if let Some(position) = collection.position_of(key) {
        let entry = &collection.entries[position];
        let key_text = text[entry.span.start..entry.value.start].trim_end();
        entries[position] = format!("{key_text} {value_text}");
    } else {
        let order = crate::render::target_order(&collection.keys(), &[key.to_string()]);
        let position =
            order.iter().position(|ordered| ordered == key).expect("key is in the order");
        entries.insert(position, format!("{}: {value_text}", crate::render::render_value(key)));
    }
    splice(text, collection, &entries)
}

/// Drop the entries whose key is in `keys` and return the document with the
/// rebuilt collection spliced in.
pub(crate) fn remove_keys(text: &str, collection: &Collection, keys: &[String]) -> String {
    let entries: Vec<String> = collection
        .entries
        .iter()
        .filter(|entry| !entry.key.as_ref().is_some_and(|key| keys.contains(key)))
        .map(|entry| text[entry.span.clone()].to_string())
        .collect();
    splice(text, collection, &entries)
}

/// Replace a flow sequence's items wholesale and return the document with
/// the rebuilt collection spliced in. `items` are already-rendered YAML.
pub(crate) fn set_items(text: &str, collection: &Collection, items: &[String]) -> String {
    splice(text, collection, items)
}

/// Render `items` as a flow sequence, for writing one as another
/// collection's entry value.
pub(crate) fn render_sequence(items: &[String]) -> String {
    render(Kind::Sequence, items)
}

/// Render `entries` between the brackets `kind` uses.
fn render(kind: Kind, entries: &[String]) -> String {
    let (open, close) = match kind {
        Kind::Mapping => ('{', '}'),
        Kind::Sequence => ('[', ']'),
    };
    if entries.is_empty() {
        format!("{open}{close}")
    } else {
        format!("{open} {} {close}", entries.join(", "))
    }
}

/// Render `entries` into `collection`'s brackets and splice the result over
/// the original collection text.
fn splice(text: &str, collection: &Collection, entries: &[String]) -> String {
    let rendered = render(collection.kind, entries);
    let mut out = text.to_string();
    out.replace_range(collection.span.clone(), &rendered);
    out
}

/// Byte offset of the bracket closing the collection that opens at `open`,
/// or `None` when the collection is unterminated, spans several lines, or
/// runs into a comment (which continues to the end of the line, so the
/// collection cannot close before it).
fn closing_bracket(text: &str, open: usize) -> Option<usize> {
    let close = closing_bracket_across_lines(text, open)?;
    // A comment inside the collection runs to the end of its line, so a
    // collection holding one cannot close before a line break either.
    (!text[open..close].contains('\n')).then_some(close)
}

/// Byte offset of the bracket closing the collection that opens at `open`,
/// however many lines and comments it spans. `None` when it is
/// unterminated. Callers that only edit one-line collections use
/// [`closing_bracket`]; this one tells them where such a value ends so they
/// can replace or delete it whole.
pub(crate) fn closing_bracket_across_lines(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut idx = open;
    while idx < bytes.len() {
        match bytes[idx] {
            b'#' if idx > open && bytes[idx - 1].is_ascii_whitespace() => {
                idx = text[idx..].find('\n').map_or(bytes.len(), |offset| idx + offset + 1);
            }
            b'{' | b'[' => {
                depth += 1;
                idx += 1;
            }
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(idx);
                }
                idx += 1;
            }
            b'\'' | b'"' => idx = skip_quoted_scalar(text, idx)?,
            _ => idx += 1,
        }
    }
    None
}

/// Split a flow collection's interior at its entry-separating commas,
/// returning each entry's trimmed span. `None` when an entry is empty
/// (which the rebuild cannot reproduce) or a quoted scalar is unterminated;
/// a single trailing comma, which YAML allows, is not an empty entry.
fn split_entries(text: &str, interior: Range<usize>) -> Option<Vec<Range<usize>>> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut start = interior.start;
    let mut depth = 0usize;
    let mut idx = interior.start;
    while idx < interior.end {
        match bytes[idx] {
            b',' if depth == 0 => {
                spans.push(trim_span(text, start..idx)?);
                start = idx + 1;
                idx += 1;
            }
            b'{' | b'[' => {
                depth += 1;
                idx += 1;
            }
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                idx += 1;
            }
            b'\'' | b'"' => idx = skip_quoted_scalar(text, idx)?,
            _ => idx += 1,
        }
    }
    // A trailing comma leaves a blank final segment; anything else blank is
    // an empty entry, which YAML reads as a null node.
    let last = start..interior.end;
    if text[last.clone()].trim().is_empty() {
        if spans.is_empty() && !text[interior].trim().is_empty() {
            return None;
        }
    } else {
        spans.push(trim_span(text, last)?);
    }
    Some(spans)
}

/// `span` without the whitespace around it, or `None` when it holds none.
fn trim_span(text: &str, span: Range<usize>) -> Option<Range<usize>> {
    let slice = &text[span.clone()];
    let trimmed = slice.trim();
    if trimmed.is_empty() {
        return None;
    }
    let start = span.start + (slice.len() - slice.trim_start().len());
    Some(start..start + trimmed.len())
}

/// Describe the entry at `span`: for a mapping, its decoded key and the
/// span of its value; for a sequence, the item alone.
fn entry(text: &str, span: Range<usize>, kind: Kind) -> Option<Entry> {
    if kind == Kind::Sequence {
        return Some(Entry { key: None, span: span.clone(), value: span });
    }
    let delimiter = key_delimiter(text, span.clone())?;
    let key = decode_key(text[span.start..delimiter].trim_end())?;
    let value = trim_span(text, delimiter + 1..span.end)?;
    Some(Entry { key: Some(key), span, value })
}

/// Byte offset of the `:` separating a flow mapping entry's key from its
/// value. In a flow context the `:` may follow a quoted key directly
/// (`{"a":"b"}`); elsewhere it is the delimiter only when whitespace or the
/// end of the entry follows it. `None` for a valueless entry (`{a, b}`) or
/// an explicit `? key : value` one.
fn key_delimiter(text: &str, span: Range<usize>) -> Option<usize> {
    if text[span.clone()].starts_with('?') {
        return None;
    }
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut idx = span.start;
    let mut after_quoted_key = false;
    while idx < span.end {
        match bytes[idx] {
            b':' if depth == 0
                && (after_quoted_key
                    || bytes.get(idx + 1).is_none_or(u8::is_ascii_whitespace)
                    || idx + 1 == span.end) =>
            {
                return Some(idx);
            }
            b'{' | b'[' => {
                depth += 1;
                idx += 1;
            }
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                idx += 1;
            }
            b'\'' | b'"' => {
                idx = skip_quoted_scalar(text, idx)?;
                after_quoted_key = depth == 0;
                continue;
            }
            _ => idx += 1,
        }
        after_quoted_key = false;
    }
    None
}

/// Decode a key's text into the string it denotes: a plain scalar denotes
/// itself, a quoted one is unquoted and unescaped. `None` when a quoted key
/// does not decode.
fn decode_key(key_text: &str) -> Option<String> {
    match key_text.as_bytes().first() {
        Some(b'\'' | b'"') => serde_saphyr::from_str::<String>(key_text).ok(),
        _ => Some(key_text.to_string()),
    }
}

/// Byte offset just past the quoted scalar starting at `open` (a `'` or `"`
/// byte), honoring the style's escape (`''` doubling, `\"`/`\\`). `None`
/// when unterminated.
fn skip_quoted_scalar(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let quote = bytes[open];
    let mut idx = open + 1;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\\' if quote == b'"' => idx += 2,
            byte if byte == quote => {
                if quote == b'\'' && bytes.get(idx + 1) == Some(&b'\'') {
                    idx += 2;
                } else {
                    return Some(idx + 1);
                }
            }
            _ => idx += 1,
        }
    }
    None
}
