//! The dotenv syntax `--env-file` reads.
//!
//! Each entry is `KEY=VALUE`, optionally prefixed with `export `. Blank
//! lines and lines starting with `#` are skipped.
//!
//! - An unquoted value runs to the end of the line or to a `#` that follows
//!   whitespace, and is trimmed. Every other character is kept as written,
//!   so `C:\Users\me`, `hello world` and `{"k":"v"}` load unchanged.
//! - A single-quoted value is literal and may span lines.
//! - A double-quoted value may span lines. `\n`, `\r`, `\t`, `\"`, `\\` and
//!   `\$` are escapes; any other backslash is kept.
//!
//! Unquoted and double-quoted values expand `$NAME` and `${NAME}`, where a
//! name is an ASCII letter or `_` followed by ASCII letters, digits or `_`.
//! A `$` that starts no name is kept, so `cost $5` loads unchanged. A name
//! the lookup does not define expands to an empty string.

use derive_more::{Display, Error};

/// A malformed entry. Only its line number is reported: env files hold
/// secrets, so the line itself must not reach stderr or CI logs.
#[derive(Debug, Display, Error, Clone, Copy, PartialEq, Eq)]
#[display("invalid dotenv entry on line {line}")]
pub struct ParseError {
    pub line: usize,
}

/// Reads the entries of one env file in order.
pub struct Parser<'content> {
    rest: &'content str,
    line: usize,
}

/// A run of literal text, or a variable to expand.
enum Piece {
    Text(String),
    Var(String),
}

/// The pieces of a value as it is scanned.
#[derive(Default)]
struct Pieces(Vec<Piece>);

impl<'content> Parser<'content> {
    pub fn new(content: &'content str) -> Self {
        Parser { rest: content, line: 1 }
    }

    /// The next `(key, value)` pair, or `None` at the end of the file.
    /// `lookup` resolves the variables in the value. The caller sets each
    /// entry before reading the next, so later entries can refer to it.
    pub fn next_entry<Lookup: Fn(&str) -> Option<String>>(
        &mut self,
        lookup: Lookup,
    ) -> Result<Option<(String, String)>, ParseError> {
        self.skip_blank_lines();
        if self.rest.is_empty() {
            return Ok(None);
        }
        let error = ParseError { line: self.line };
        let text = strip_export(trim_start_inline(self.rest));
        let (key, text) = split_key(text).ok_or(error)?;
        let text = trim_start_inline(text).strip_prefix('=').ok_or(error)?;
        let (pieces, rest) = parse_value(trim_start_inline(text)).ok_or(error)?;
        self.advance(rest);
        Ok(Some((key.to_owned(), pieces.expand(lookup))))
    }

    fn skip_blank_lines(&mut self) {
        while !self.rest.is_empty() {
            let (line, after) = split_line(self.rest);
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#') {
                return;
            }
            self.advance(after);
        }
    }

    /// Move to `rest`, a suffix of the unread input, counting the lines
    /// passed so errors report where an entry starts.
    fn advance(&mut self, rest: &'content str) {
        let consumed = &self.rest[..self.rest.len() - rest.len()];
        self.line += consumed.matches('\n').count();
        self.rest = rest;
    }
}

impl Pieces {
    fn push_char(&mut self, character: char) {
        match self.0.last_mut() {
            Some(Piece::Text(text)) => text.push(character),
            _ => self.0.push(Piece::Text(character.to_string())),
        }
    }

    /// Handle the text after a `$`, returning what follows the variable.
    fn push_dollar<'text>(&mut self, rest: &'text str) -> Option<&'text str> {
        if let Some(braced) = rest.strip_prefix('{') {
            let end = braced.find('}')?;
            let name = &braced[..end];
            if name.is_empty() || name_len(name) != name.len() {
                return None;
            }
            self.0.push(Piece::Var(name.to_owned()));
            return Some(&braced[end + 1..]);
        }
        let len = name_len(rest);
        if len == 0 {
            self.push_char('$');
        } else {
            self.0.push(Piece::Var(rest[..len].to_owned()));
        }
        Some(&rest[len..])
    }

    /// Handle the text after a `\` in a double-quoted value, returning what
    /// follows the escaped character.
    fn push_escape<'text>(&mut self, rest: &'text str) -> Option<&'text str> {
        let character = rest.chars().next()?;
        match character {
            'n' => self.push_char('\n'),
            'r' => self.push_char('\r'),
            't' => self.push_char('\t'),
            '"' | '\\' | '$' => self.push_char(character),
            _ => {
                self.push_char('\\');
                self.push_char(character);
            }
        }
        Some(&rest[character.len_utf8()..])
    }

    fn expand<Lookup: Fn(&str) -> Option<String>>(self, lookup: Lookup) -> String {
        let mut value = String::new();
        for piece in self.0 {
            match piece {
                Piece::Text(text) => value.push_str(&text),
                Piece::Var(name) => value.push_str(&lookup(&name).unwrap_or_default()),
            }
        }
        value
    }
}

/// The value at the start of `text`, and the input after its line.
fn parse_value(text: &str) -> Option<(Pieces, &str)> {
    match text.chars().next() {
        Some('\'') => single_quoted(&text[1..]),
        Some('"') => double_quoted(&text[1..]),
        _ => unquoted(text),
    }
}

fn single_quoted(text: &str) -> Option<(Pieces, &str)> {
    let end = text.find('\'')?;
    let pieces = Pieces(vec![Piece::Text(text[..end].to_owned())]);
    Some((pieces, end_of_line(&text[end + 1..])?))
}

fn double_quoted(text: &str) -> Option<(Pieces, &str)> {
    let mut pieces = Pieces::default();
    let mut rest = text;
    loop {
        let character = rest.chars().next()?;
        rest = &rest[character.len_utf8()..];
        match character {
            '"' => return Some((pieces, end_of_line(rest)?)),
            '\\' => rest = pieces.push_escape(rest)?,
            '$' => rest = pieces.push_dollar(rest)?,
            _ => pieces.push_char(character),
        }
    }
}

fn unquoted(text: &str) -> Option<(Pieces, &str)> {
    let (line, after) = split_line(text);
    let mut pieces = Pieces::default();
    let mut rest = strip_comment(line).trim_end();
    while let Some(character) = rest.chars().next() {
        rest = &rest[character.len_utf8()..];
        if character == '$' {
            rest = pieces.push_dollar(rest)?;
        } else {
            pieces.push_char(character);
        }
    }
    Some((pieces, after))
}

/// The input after the current line, if the rest of the line is blank or
/// a comment.
fn end_of_line(text: &str) -> Option<&str> {
    let (line, after) = split_line(text);
    let line = line.trim();
    (line.is_empty() || line.starts_with('#')).then_some(after)
}

/// Cut an unquoted value at a `#` that starts it or follows whitespace.
fn strip_comment(line: &str) -> &str {
    let comment = line
        .match_indices('#')
        .find(|(index, _)| *index == 0 || line[..*index].ends_with([' ', '\t']));
    comment.map_or(line, |(index, _)| &line[..index])
}

/// The current line without its line ending, and the input after it.
fn split_line(text: &str) -> (&str, &str) {
    let (line, after) = text
        .split_once('\n')
        .unwrap_or((text, ""));
    (line.strip_suffix('\r').unwrap_or(line), after)
}

fn split_key(text: &str) -> Option<(&str, &str)> {
    if !text.starts_with(|character: char| character.is_ascii_alphabetic() || character == '_') {
        return None;
    }
    let end = text
        .find(|character: char| {
            !(character.is_ascii_alphanumeric() || character == '_' || character == '.')
        })
        .unwrap_or(text.len());
    Some(text.split_at(end))
}

/// Strip an `export ` prefix. In `export = value`, `export` is the key.
fn strip_export(text: &str) -> &str {
    match text.strip_prefix("export") {
        Some(rest)
            if rest.starts_with([' ', '\t']) && !trim_start_inline(rest).starts_with('=') =>
        {
            trim_start_inline(rest)
        }
        _ => text,
    }
}

fn trim_start_inline(text: &str) -> &str {
    text.trim_start_matches([' ', '\t'])
}

/// The length of the variable name at the start of `text`, or 0.
fn name_len(text: &str) -> usize {
    if !text.starts_with(|character: char| character.is_ascii_alphabetic() || character == '_') {
        return 0;
    }
    text.find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests;
