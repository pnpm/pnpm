//! The fingerprint that marks a database with the schema of its tables.

use super::{MARKER_TABLE, TABLES};
use std::sync::LazyLock;

/// The fingerprint of the schema this pnpr reads: the SHA-256, in hex, of
/// [`script`] without its comments and with its whitespace normalized
/// by [`canonical`]. Reformatting or commenting the statements leaves it
/// unchanged; any other edit changes it.
pub(in super::super) fn fingerprint() -> &'static str {
    static FINGERPRINT: LazyLock<String> = LazyLock::new(|| fingerprint_of(&script()));
    &FINGERPRINT
}

/// Every statement of the schema: the metadata's [`TABLES`] and the
/// [`MARKER_TABLE`].
pub(in super::super) fn script() -> String {
    format!("{TABLES}\n{MARKER_TABLE};")
}

pub(in super::super) fn fingerprint_of(script: &str) -> String {
    let canonical = statements(script)
        .map(|statement| canonical(&statement))
        .collect::<Vec<_>>()
        .join(";\n");
    pnpm_crypto_hash::create_hex_hash(&canonical)
}

/// `statement` with its insignificant whitespace normalized: a run of
/// whitespace between two words becomes one space, and whitespace beside
/// punctuation or a quoted name or value is dropped, so `catalogs (` and
/// `catalogs(` read the same. A quoted name or value is kept verbatim.
pub(in super::super) fn canonical(statement: &str) -> String {
    let mut canonical = String::with_capacity(statement.len());
    let mut rest = statement;
    let mut after_word = false;
    while let Some(first) = rest.chars().next() {
        if first.is_whitespace() {
            rest = rest.trim_start();
            if after_word && rest.starts_with(is_word) {
                canonical.push(' ');
            }
            continue;
        }
        let token = token(rest);
        canonical.push_str(token);
        rest = &rest[token.len()..];
        after_word = is_word(first);
    }
    canonical
}

/// Whether `character` belongs to a word: a keyword, a name, or a number.
fn is_word(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// The token `text` starts with: a word, a quoted name or value with its
/// quotes, or one character of punctuation.
fn token(text: &str) -> &str {
    let Some(first) = text.chars().next() else {
        return text;
    };
    if let Some(close) = closing_quote(first) {
        return quoted(text, first.len_utf8(), close);
    }
    let end = if is_word(first) {
        text.find(|character| !is_word(character))
            .unwrap_or(text.len())
    } else {
        first.len_utf8()
    };
    &text[..end]
}

/// The character that closes a quote `character` opens, if it opens one.
fn closing_quote(character: char) -> Option<char> {
    match character {
        '\'' | '"' | '`' => Some(character),
        '[' => Some(']'),
        _ => None,
    }
}

/// The quoted text `text` starts with, through its closing quote. A doubled
/// closing quote escapes it. An unclosed quote runs to the end of `text`.
fn quoted(text: &str, open: usize, close: char) -> &str {
    let mut end = open;
    loop {
        let Some(offset) = text[end..].find(close) else {
            return text;
        };
        end += offset + close.len_utf8();
        if !text[end..].starts_with(close) {
            return &text[..end];
        }
        end += close.len_utf8();
    }
}

/// The statements of `script`, without its comments. Each runs on its own:
/// a libSQL client re-renders the statements of a batch, and its rendering
/// drops the comma between `STRICT` and `WITHOUT ROWID`. A `;` or a comment
/// marker inside a quoted name or value is part of it.
pub(in super::super) fn statements(script: &str) -> impl Iterator<Item = String> {
    let mut statements = Vec::new();
    let mut statement = String::new();
    let mut rest = script;
    while !rest.is_empty() {
        let (piece, after) = piece(rest);
        rest = after;
        match piece {
            Piece::Text(text) => statement.push_str(text),
            Piece::Comment => statement.push(' '),
            Piece::End => finish(&mut statements, &mut statement),
        }
    }
    finish(&mut statements, &mut statement);
    statements.into_iter()
}

/// What a script holds at some point.
enum Piece<'a> {
    /// A token of a statement.
    Text(&'a str),
    /// A comment, which separates tokens like whitespace.
    Comment,
    /// The `;` that ends a statement.
    End,
}

/// The piece `text` starts with, and the text after it.
fn piece(text: &str) -> (Piece<'_>, &str) {
    if let Some(after) = text.strip_prefix(';') {
        return (Piece::End, after);
    }
    if text.starts_with("--") {
        let end = text.find('\n').unwrap_or(text.len());
        return (Piece::Comment, &text[end..]);
    }
    if let Some(comment) = text.strip_prefix("/*") {
        let after = comment
            .find("*/")
            .map_or("", |end| &comment[end + "*/".len()..]);
        return (Piece::Comment, after);
    }
    let token = token(text);
    (Piece::Text(token), &text[token.len()..])
}

/// Add `statement` to `statements` unless it is blank, and clear it.
fn finish(statements: &mut Vec<String>, statement: &mut String) {
    let trimmed = statement.trim();
    if !trimmed.is_empty() {
        statements.push(trimmed.to_string());
    }
    statement.clear();
}
