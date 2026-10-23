//! Reading the `unsafe` in a source file, and the opt-out above it.

use super::{Kind, Occurrence, lexical};
use std::path::{Path, PathBuf};

/// Every `unsafe` in one file.
///
/// A line scan, not a parse: it is the shape of the thing the policy
/// governs, and the workspace's `unsafe_code` lint is what actually stops
/// an `unsafe` compiling. What this adds is reading the source rather than
/// the lint level, so an `unsafe` a hand-written `allow` would let through
/// is still found.
pub(super) fn occurrences(root: &Path, source: &Path) -> Vec<Occurrence> {
    let text = std::fs::read_to_string(source)
        .unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
    let lines: Vec<&str> = text.lines().collect();
    // Carried rather than chained: a string or comment opened on one line
    // decides how the next one reads, which a per-line filter cannot see.
    let mut carry = lexical::Carry::Code;
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let (code, next) = lexical::blank_quoted(line, carry);
        carry = next;
        if opens_unsafe(&code) {
            found.push(Occurrence {
                file: relative(root, source),
                line: index + 1,
                text: (*line).trim().to_owned(),
                kind: kind(&code),
                has_safety_comment: has_safety_comment(&lines[..index]),
            });
        }
    }
    found
}

/// Whether a line opens an `unsafe` block, function, implementation or
/// `extern` declaration.
///
/// The word has to start a token, so `unsafe_code` and `unsafePerm` do not
/// match, and it has to be followed by what makes it an operation rather
/// than prose, so a comment or a string mentioning it does not either.
fn opens_unsafe(line: &str) -> bool {
    let trimmed = line.trim_start();
    // A comment or a block-comment continuation describes an `unsafe`
    // rather than being one. The continuation is a `*` that ends the line
    // or is followed by a space or the `/` that closes the comment, which
    // is what tells it from a dereference such as `*slot = unsafe { x }`.
    let continues_block_comment = trimmed
        .strip_prefix('*')
        .is_some_and(|rest| {
            rest.is_empty() || rest.starts_with('/') || rest.starts_with(char::is_whitespace)
        });
    if trimmed.starts_with("//") || continues_block_comment {
        return false;
    }
    unsafe_tokens(line)
        .any(|rest| {
            rest.starts_with('{')
                || rest.starts_with("fn ")
                || rest.starts_with("impl ")
                || rest.starts_with("trait ")
                || rest.starts_with("extern")
        })
}

/// What the `unsafe` on a line is.
fn kind(line: &str) -> Kind {
    let mut kind = Kind::Operation;
    for rest in unsafe_tokens(line) {
        let Some(rest) = rest.strip_prefix("extern") else { continue };
        // `unsafe extern {` or `unsafe extern "abi" {` declares an ABI;
        // `unsafe extern "abi" fn` states one signature.
        let declares = rest
            .trim_start()
            .trim_start_matches(|character: char| character != '{' && character != 'f')
            .starts_with('{');
        kind = if declares { Kind::ForeignAbiBlock } else { Kind::ForeignSignature };
    }
    kind
}

/// What follows each `unsafe` token on a line, with the leading space
/// taken off.
///
/// A token inside a string literal is left out, because the scanner's own
/// fixtures are lines of Rust quoted as data and a diagnostic can quote
/// one too.
fn unsafe_tokens(line: &str) -> impl Iterator<Item = &str> {
    line.match_indices("unsafe")
        .filter(move |(index, _)| !preceded_by_word(line, *index) && !quoted(line, *index))
        .map(move |(index, token)| line[index + token.len()..].trim_start())
}

/// Whether the character before `index` continues an identifier, which
/// makes the match part of a longer name such as `unsafe_code`.
fn preceded_by_word(line: &str, index: usize) -> bool {
    line[..index]
        .chars()
        .next_back()
        .is_some_and(|character| character.is_alphanumeric() || character == '_')
}

/// Whether `index` falls inside a string literal, which an odd number of
/// unescaped quotes before it means.
///
/// A line-local count, not a lexer: a raw string or a multi-line literal
/// can fool it. What that costs is a wrong answer on a line that mentions
/// `unsafe` inside such a literal, and the attribute check is what
/// actually keeps `unsafe` out of a crate.
fn quoted(line: &str, index: usize) -> bool {
    let before = &line[..index];
    if opens_raw_string(before) {
        return true;
    }
    let mut quotes = 0;
    let mut escaped = false;
    for character in before.chars() {
        match character {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => quotes += 1,
            _ => {}
        }
    }
    quotes % 2 == 1
}

/// Whether a raw string opens in `before`.
///
/// A raw string takes no escapes, so the quote count above cannot be read
/// through one. Everything after the opener on the line is treated as
/// inside it, which is why this is checked first.
fn opens_raw_string(before: &str) -> bool {
    before
        .match_indices('r')
        .filter(|(index, _)| !preceded_by_word(before, *index))
        .any(|(index, _)| before[index + 1..].trim_start_matches('#').starts_with('"'))
}

/// Whether the comment block above an `unsafe` carries a safety line: one
/// whose first word is the `SAFETY` marker.
///
/// The whole block, not just the line above, because a safety argument
/// that names every precondition rarely fits on one. Three things may sit
/// between the comment and the `unsafe`: a blank line; an attribute, since
/// an `#[expect(...)]` or a `#[cfg(...)]` often does; and a line ending in
/// `=`, which is where the formatter wraps a long `let` whose value is the
/// `unsafe`.
fn has_safety_comment(above: &[&str]) -> bool {
    above
        .iter()
        .rev()
        .map(|line| line.trim())
        .skip_while(|line| line.is_empty() || line.starts_with('#') || line.ends_with('='))
        .take_while(|line| line.starts_with("//"))
        .any(|line| line.starts_with("// SAFETY:"))
}

/// Whether a crate root opts out of the `unsafe_code` lint.
pub(super) fn opts_out_of_unsafe_code(root: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(root) else {
        return false;
    };
    text.lines().any(opts_out)
}

/// Whether a line is an inner attribute opting out of the `unsafe_code`
/// lint.
///
/// The attribute has to open the line, which leaves out a doc comment
/// describing one and a fixture quoting one as data. It has to be the
/// inner form too: an outer `#[allow(unsafe_code)]` covers one item rather
/// than the crate, and an `unsafe` it lets through is caught by the scan
/// that reads the source instead of the lint level.
fn opts_out(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("#![allow(unsafe_code") || line.starts_with("#![expect(unsafe_code")
}

pub(super) fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_path_buf()
}

#[cfg(test)]
mod tests;
