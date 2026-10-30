//! Reading a line in the lexical state the line before it left.
//!
//! A string or a comment that spans lines makes its contents read as code
//! to anything that looks at one line alone. This module blanks what a line
//! quotes or comments out, so the scan sees only what the line executes.

/// What a line begins inside, for the constructs that span lines.
#[derive(Clone, Copy)]
pub(super) enum Carry {
    Code,
    BlockComment { depth: usize },
    RawText { hashes: usize },
    Text,
}

/// Blank what `line` quotes or comments out, given the state it begins in.
///
/// Delimiters stay and only the contents become spaces, so a line that
/// closes one construct and then opens an `unsafe` still reports it.
pub(super) fn blank_quoted(line: &str, carry: Carry) -> (String, Carry) {
    let chars: Vec<char> = line.chars().collect();
    let mut blanker = Blanker { chars: &chars, out: String::with_capacity(line.len()), index: 0 };
    let mut carry = carry;
    while blanker.index < chars.len() {
        carry = match carry {
            Carry::Code => blanker.step_code(),
            Carry::BlockComment { depth } => blanker.step_block_comment(depth),
            Carry::Text => blanker.step_text(),
            Carry::RawText { hashes } => blanker.step_raw_text(hashes),
        };
    }
    (blanker.out, carry)
}

/// One line, and how much of it has been read.
struct Blanker<'line> {
    chars: &'line [char],
    out: String,
    index: usize,
}

impl Blanker<'_> {
    /// Copy `width` characters through unchanged.
    fn keep(&mut self, width: usize) {
        self.out.extend(&self.chars[self.index..self.index + width]);
        self.index += width;
    }

    /// Replace `width` characters with spaces.
    fn blank(&mut self, width: usize) {
        self.out.extend(std::iter::repeat_n(' ', width));
        self.index += width;
    }

    fn step_code(&mut self) -> Carry {
        match opener_at(self.chars, self.index) {
            Opener::LineComment => {
                self.blank(self.chars.len() - self.index);
                Carry::Code
            }
            Opener::BlockComment => {
                self.keep(2);
                Carry::BlockComment { depth: 1 }
            }
            Opener::RawText { width, hashes } => {
                self.keep(width);
                Carry::RawText { hashes }
            }
            Opener::Text => {
                self.keep(1);
                Carry::Text
            }
            Opener::Character { width } => {
                self.keep(width);
                Carry::Code
            }
            Opener::Plain => {
                self.keep(1);
                Carry::Code
            }
        }
    }

    fn step_block_comment(&mut self, depth: usize) -> Carry {
        if self.starts_with('/', '*') {
            self.blank(2);
            return Carry::BlockComment { depth: depth + 1 };
        }
        if self.starts_with('*', '/') {
            self.blank(2);
            return if depth == 1 { Carry::Code } else { Carry::BlockComment { depth: depth - 1 } };
        }
        self.blank(1);
        Carry::BlockComment { depth }
    }

    fn step_text(&mut self) -> Carry {
        if self.chars[self.index] == '\\' && self.index + 1 < self.chars.len() {
            self.blank(2);
            return Carry::Text;
        }
        if self.chars[self.index] == '"' {
            self.keep(1);
            return Carry::Code;
        }
        self.blank(1);
        Carry::Text
    }

    fn step_raw_text(&mut self, hashes: usize) -> Carry {
        if closes_raw_text(self.chars, self.index, hashes) {
            self.keep(1 + hashes);
            return Carry::Code;
        }
        self.blank(1);
        Carry::RawText { hashes }
    }

    fn starts_with(&self, first: char, second: char) -> bool {
        self.chars[self.index] == first && self.chars.get(self.index + 1) == Some(&second)
    }
}

/// What begins at `index` in code.
enum Opener {
    LineComment,
    BlockComment,
    RawText {
        width: usize,
        hashes: usize,
    },
    Text,
    /// A character literal, which is not a quote.
    Character {
        width: usize,
    },
    Plain,
}

fn opener_at(chars: &[char], index: usize) -> Opener {
    let pair = |first: char, second: char| chars[index..].first_chunk() == Some(&[first, second]);
    if pair('/', '/') {
        return Opener::LineComment;
    }
    if pair('/', '*') {
        return Opener::BlockComment;
    }
    if let Some((width, hashes)) = raw_text_opener(chars, index) {
        return Opener::RawText { width, hashes };
    }
    if chars.get(index) == Some(&'"') {
        return Opener::Text;
    }
    character_literal_width(chars, index).map_or(Opener::Plain, |width| Opener::Character { width })
}

/// The opener of a raw string at `index`, as its width and its `#` count.
///
/// `b` first is the byte-string form. A preceding word character means the
/// `r` belongs to an identifier rather than to a literal.
fn raw_text_opener(chars: &[char], index: usize) -> Option<(usize, usize)> {
    if index > 0 && (chars[index - 1].is_alphanumeric() || chars[index - 1] == '_') {
        return None;
    }
    let mut cursor = index;
    if chars.get(cursor) == Some(&'b') {
        cursor += 1;
    }
    if chars.get(cursor) != Some(&'r') {
        return None;
    }
    cursor += 1;
    let hashes = chars[cursor..]
        .iter()
        .take_while(|character| **character == '#')
        .count();
    cursor += hashes;
    (chars.get(cursor) == Some(&'"')).then_some((cursor + 1 - index, hashes))
}

/// Whether a raw string needing `hashes` closes at `index`.
fn closes_raw_text(chars: &[char], index: usize, hashes: usize) -> bool {
    chars[index] == '"'
        && chars[index + 1..]
            .iter()
            .take(hashes)
            .filter(|character| **character == '#')
            .count()
            == hashes
}

/// The width of a character literal at `index`, or `None` for a lifetime,
/// whose quote opens nothing and so must not be read as one.
fn character_literal_width(chars: &[char], index: usize) -> Option<usize> {
    if chars.get(index) != Some(&'\'') {
        return None;
    }
    if chars.get(index + 1) == Some(&'\\') {
        let close = chars[index + 2..]
            .iter()
            .position(|character| *character == '\'')?;
        return Some(index + 2 + close + 1 - index);
    }
    (chars.get(index + 2) == Some(&'\'')).then_some(3)
}
