//! Where an item, a signature, or a field ends in the token stream.

use crate::lexer::Token;

/// Where the scan of an item stops.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// Before a body or tuple fields: `{`, `(`, or `;` outside brackets.
    Body,
    /// At `{` or `;` outside brackets: a function's body, or the end of a
    /// type alias or a body-less trait method.
    Signature,
    /// At `=` or `;` outside brackets.
    Value,
    /// At `,` or a closing bracket outside the field's own brackets.
    FieldEnd,
    /// After the item's `{ … }` body, or at `;` before one.
    ItemEnd,
    /// Like `ItemEnd`, and also at `,` or a closing bracket, so a field
    /// ends where it does.
    Extent,
}

impl Stop {
    /// Whether `<` and `>` count as brackets. They do while reading a type,
    /// so a `{`, `=`, or `,` inside generics, as in `Wrapper<{ N }>` or
    /// `Iterator<Item = T>`, does not end the scan. A whole item's scan reads
    /// its value too, where `1 << 3` is a shift, so it leaves them out.
    const fn tracks_angles(self) -> bool {
        !matches!(self, Self::ItemEnd)
    }

    const fn ends_at(self, punct: char) -> bool {
        match self {
            Self::Body => matches!(punct, '{' | '(' | ';'),
            Self::Signature | Self::ItemEnd => matches!(punct, '{' | ';'),
            Self::Value => matches!(punct, '=' | ';'),
            Self::FieldEnd => punct == ',',
            Self::Extent => matches!(punct, '{' | ';' | ','),
        }
    }
}

/// The index where the scan from `start` stops under `stop`.
pub fn item_end(tokens: &[Token], start: usize, stop: Stop) -> usize {
    let end = boundary(tokens, start, stop);
    let opens_body =
        matches!(stop, Stop::ItemEnd | Stop::Extent) && tokens.get(end) == Some(&Token::Punct('{'));
    if opens_body { closing_brace(tokens, end) } else { end }
}

/// The first token from `start` that ends the item under `stop` outside
/// brackets, or the bracket that closes the one the item is in.
fn boundary(tokens: &[Token], start: usize, stop: Stop) -> usize {
    let mut nesting = Nesting::default();
    for (pos, token) in tokens.iter().enumerate().skip(start) {
        let Token::Punct(punct) = token else { continue };
        let closes = matches!(punct, ')' | ']' | '}');
        if nesting.is_flat() && (stop.ends_at(*punct) || closes) {
            return pos;
        }
        nesting.step(tokens, pos, stop.tracks_angles());
    }
    tokens.len()
}

/// The bracket nesting at a point of a scan.
#[derive(Default)]
pub struct Nesting {
    brackets: usize,
    angles: usize,
}

impl Nesting {
    /// Whether the scan is outside every bracket.
    pub const fn is_flat(&self) -> bool {
        self.brackets == 0 && self.angles == 0
    }

    /// Updates the nesting for the token at `pos`. With `angles`, a `<` or
    /// `>` outside other brackets counts as a generic bracket. Inside other
    /// brackets a `,` cannot end the scan, and a `<` there may be a shift,
    /// as in `[u8; 1 << 3]`.
    pub fn step(&mut self, tokens: &[Token], pos: usize, angles: bool) {
        let generic = angles && self.brackets == 0;
        match tokens[pos] {
            Token::Punct('(' | '[' | '{') => self.brackets += 1,
            Token::Punct(')' | ']' | '}') => self.brackets = self.brackets.saturating_sub(1),
            Token::Punct('<') if generic => self.angles += 1,
            Token::Punct('>') if generic && !is_arrow(tokens, pos) => {
                self.angles = self.angles.saturating_sub(1);
            }
            _ => {}
        }
    }
}

/// Whether the `>` at `pos` is the second half of `->` or `=>`.
fn is_arrow(tokens: &[Token], pos: usize) -> bool {
    pos > 0 && matches!(tokens[pos - 1], Token::Punct('-' | '='))
}

/// The index after the `}` that closes the `{` at `open`.
pub fn closing_brace(tokens: &[Token], open: usize) -> usize {
    let mut depth = 0usize;
    for (offset, token) in tokens[open..].iter().enumerate() {
        match token {
            Token::Punct('{') => depth += 1,
            Token::Punct('}') if depth == 1 => return open + offset + 1,
            Token::Punct('}') => depth -= 1,
            _ => {}
        }
    }
    tokens.len()
}
