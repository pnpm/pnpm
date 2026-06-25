//! Parser for the property-path mini-language used by `pnpm config get`.
//!
//! Port of the read half of pnpm's
//! [`@pnpm/object.property-path`](https://github.com/pnpm/pnpm/blob/8eb1be4988/object/property-path/src):
//! the tokenizer ([`tokenize`]), the parser ([`parse_property_path`]), and
//! [`get_object_value_by_property_path`]. The mutating `set`/`delete` halves
//! are not ported — the config command only reads through a property path and
//! uses [`parse_property_path`] to classify a key as simple vs. deep.
//!
//! Grammar (mirroring upstream's examples):
//! `foo.bar.baz`, `.foo.bar`, `foo.bar["baz"]`, `foo['bar'].baz`,
//! `["foo"].bar`, `foo[123]`.

use derive_more::{Display, Error};
use serde_json::Value;

/// One parsed property-path segment. A numeric literal keeps its numeric
/// identity (it is the only form that may index into an array), mirroring
/// upstream's `string | number` yield type.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Key(String),
    Index(f64),
}

/// Error raised while parsing a property path. Mirrors the `PnpmError`
/// subclasses in pnpm's `parse.ts` / token parsers; the codes match so
/// they remain part of the public contract.
#[derive(Debug, Display, Error, PartialEq)]
#[non_exhaustive]
pub enum ParsePropertyPathError {
    #[display("Unexpected token {token:?} in property path")]
    UnexpectedToken { token: String },

    #[display("Unexpected identifier {token} in property path")]
    UnexpectedIdentifier { token: String },

    #[display("Unexpected literal {token:?} in property path")]
    UnexpectedLiteral { token: String },

    #[display("The property path does not end properly")]
    UnexpectedEndOfInput,

    #[display("Numeric suffix {suffix:?} is not supported")]
    UnsupportedNumericSuffix { suffix: String },

    #[display("pnpm's string literal doesn't support {sequence:?}")]
    UnsupportedEscapeSequence { sequence: String },

    #[display("Input ends without closing quote ({quote})")]
    IncompleteStringLiteral { quote: char },
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Dot,
    OpenBracket,
    CloseBracket,
    Identifier(String),
    NumericLiteral(f64),
    StringLiteral(String),
    Whitespace,
    Unexpected(String),
}

/// Tokenize `source`, returning the token stream or the first parse error.
fn tokenize(source: &str) -> Result<Vec<Token>, ParsePropertyPathError> {
    let mut tokens = Vec::new();
    let mut rest = source;
    while !rest.is_empty() {
        let (token, remaining) = parse_token(rest)?;
        tokens.push(token);
        rest = remaining;
    }
    Ok(tokens)
}

fn parse_token(source: &str) -> Result<(Token, &str), ParsePropertyPathError> {
    if let Some(result) = parse_exact(source) {
        return Ok(result);
    }
    if let Some(result) = parse_identifier(source) {
        return Ok(result);
    }
    if let Some(result) = parse_numeric_literal(source)? {
        return Ok(result);
    }
    if let Some(result) = parse_string_literal(source)? {
        return Ok(result);
    }
    if let Some(result) = parse_whitespace(source) {
        return Ok(result);
    }
    // Unexpected: a single (char-boundary-safe) unit, mirroring upstream's
    // `source.slice(0, 1)`.
    let mut indices = source.char_indices();
    indices.next();
    let split = indices.next().map_or(source.len(), |(i, _)| i);
    let (head, tail) = source.split_at(split);
    Ok((Token::Unexpected(head.to_string()), tail))
}

fn parse_exact(source: &str) -> Option<(Token, &str)> {
    for (prefix, token) in
        [('.', Token::Dot), ('[', Token::OpenBracket), (']', Token::CloseBracket)]
    {
        if let Some(rest) = source.strip_prefix(prefix) {
            return Some((token, rest));
        }
    }
    None
}

fn parse_identifier(source: &str) -> Option<(Token, &str)> {
    let mut chars = source.char_indices();
    let (_, first) = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let mut end = first.len_utf8();
    for (i, c) in chars {
        // `\w` in JS = [A-Za-z0-9_]
        if c.is_ascii_alphanumeric() || c == '_' {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    let (content, rest) = source.split_at(end);
    Some((Token::Identifier(content.to_string()), rest))
}

fn parse_numeric_literal(source: &str) -> Result<Option<(Token, &str)>, ParsePropertyPathError> {
    let mut chars = source.char_indices();
    let Some((_, first)) = chars.next() else {
        return Ok(None);
    };
    if !first.is_ascii_digit() {
        return Ok(None);
    }
    let mut end = 1;
    for (i, c) in chars {
        if c.is_ascii_digit() || c == '.' {
            end = i + c.len_utf8();
        } else if c.is_ascii_alphabetic() {
            // Forbid `0x1A`, `1e20`, `123n`, ... like upstream.
            return Err(ParsePropertyPathError::UnsupportedNumericSuffix { suffix: c.to_string() });
        } else {
            break;
        }
    }
    let (number_string, rest) = source.split_at(end);
    let number: f64 = number_string.parse().unwrap_or(f64::NAN);
    Ok(Some((Token::NumericLiteral(number), rest)))
}

fn parse_string_literal(source: &str) -> Result<Option<(Token, &str)>, ParsePropertyPathError> {
    let quote = match source.chars().next() {
        Some('"') => '"',
        Some('\'') => '\'',
        _ => return Ok(None),
    };
    let mut content = String::new();
    let mut escaped = false;
    let mut chars = source.char_indices();
    chars.next(); // consume opening quote
    for (i, c) in chars {
        if escaped {
            escaped = false;
            let real = match c {
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                'b' => '\u{08}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => {
                    return Err(ParsePropertyPathError::UnsupportedEscapeSequence {
                        sequence: other.to_string(),
                    });
                }
            };
            content.push(real);
            continue;
        }
        if c == quote {
            let rest = &source[i + c.len_utf8()..];
            return Ok(Some((Token::StringLiteral(content), rest)));
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        content.push(c);
    }
    Err(ParsePropertyPathError::IncompleteStringLiteral { quote })
}

fn parse_whitespace(source: &str) -> Option<(Token, &str)> {
    let trimmed = source.trim_start();
    if trimmed.len() == source.len() { None } else { Some((Token::Whitespace, trimmed)) }
}

/// Parse a property path string into its segments.
///
/// Mirrors pnpm's `parsePropertyPath` shift/reduce loop: a leading or
/// inter-segment `.`, bracketed string/number literals, and bare identifiers.
pub fn parse_property_path(property_path: &str) -> Result<Vec<Segment>, ParsePropertyPathError> {
    enum Stack {
        Dot,
        OpenBracket,
        Bracketed(Token),
    }
    let mut stack: Option<Stack> = None;
    let mut segments = Vec::new();

    for token in tokenize(property_path)? {
        match token {
            Token::Dot => match stack {
                None => stack = Some(Stack::Dot),
                _ => return Err(unexpected(&Token::Dot)),
            },
            Token::OpenBracket => match stack {
                None => stack = Some(Stack::OpenBracket),
                _ => return Err(unexpected(&Token::OpenBracket)),
            },
            Token::CloseBracket => {
                let Some(Stack::Bracketed(literal)) = stack else {
                    return Err(unexpected(&Token::CloseBracket));
                };
                segments.push(literal_to_segment(literal));
                stack = None;
            }
            Token::Identifier(ref content) => match stack {
                None | Some(Stack::Dot) => {
                    stack = None;
                    segments.push(Segment::Key(content.clone()));
                }
                _ => {
                    return Err(ParsePropertyPathError::UnexpectedIdentifier {
                        token: content.clone(),
                    });
                }
            },
            Token::NumericLiteral(_) | Token::StringLiteral(_) => match stack {
                Some(Stack::OpenBracket) => stack = Some(Stack::Bracketed(token)),
                _ => return Err(unexpected_literal(&token)),
            },
            Token::Whitespace => {}
            Token::Unexpected(ref content) => {
                return Err(ParsePropertyPathError::UnexpectedToken { token: content.clone() });
            }
        }
    }

    if stack.is_some() {
        return Err(ParsePropertyPathError::UnexpectedEndOfInput);
    }
    Ok(segments)
}

fn literal_to_segment(token: Token) -> Segment {
    match token {
        Token::NumericLiteral(n) => Segment::Index(n),
        Token::StringLiteral(s) => Segment::Key(s),
        _ => unreachable!("only literal tokens are pushed onto the bracket stack"),
    }
}

fn unexpected(token: &Token) -> ParsePropertyPathError {
    ParsePropertyPathError::UnexpectedToken { token: token_content(token) }
}

fn unexpected_literal(token: &Token) -> ParsePropertyPathError {
    ParsePropertyPathError::UnexpectedLiteral { token: token_content(token) }
}

fn token_content(token: &Token) -> String {
    match token {
        Token::Dot => ".".to_string(),
        Token::OpenBracket => "[".to_string(),
        Token::CloseBracket => "]".to_string(),
        Token::Identifier(s) | Token::StringLiteral(s) | Token::Unexpected(s) => s.clone(),
        Token::NumericLiteral(n) => number_to_string(*n),
        Token::Whitespace => " ".to_string(),
    }
}

/// Walk `value` along `property_path`, returning the value found there, or
/// `None` if any step meets a non-object/array, a missing key, or a
/// non-numeric segment indexing an array.
///
/// Mirrors pnpm's `getObjectValueByPropertyPath`.
#[must_use]
pub fn get_object_value_by_property_path<'a>(
    value: &'a Value,
    property_path: &[Segment],
) -> Option<&'a Value> {
    let mut current = value;
    for segment in property_path {
        match current {
            Value::Object(map) => {
                let key = match segment {
                    Segment::Key(key) => key.clone(),
                    Segment::Index(n) => number_to_string(*n),
                };
                current = map.get(&key)?;
            }
            Value::Array(items) => {
                // On an array, a non-numeric segment yields `undefined`
                // (`typeof name !== 'number'`).
                let Segment::Index(n) = segment else {
                    return None;
                };
                let index = array_index(*n)?;
                current = items.get(index)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

/// Convert a numeric segment to a valid array index, or `None` when it is not
/// a non-negative integer (`Object.hasOwn(array, name)` is false otherwise).
fn array_index(n: f64) -> Option<usize> {
    if n.fract() != 0.0 || n.is_sign_negative() || !n.is_finite() {
        return None;
    }
    Some(n as usize)
}

/// Render a JS number the way `String(number)` / object-key coercion would for
/// the integer values these paths use.
fn number_to_string(n: f64) -> String {
    if n.fract() == 0.0 && n.is_finite() { format!("{}", n as i64) } else { n.to_string() }
}

#[cfg(test)]
mod tests;
