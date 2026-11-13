//! Forward scans that locate the closing delimiter of `${...}`, `` `...` ``,
//! and `#` comments within the shared [`WASTED_SCAN_ALLOWANCE`].

use std::cell::Cell;

/// How much fruitless scanning a script is allowed, as a multiple of its own
/// length. A delimiter that never closes is scanned to the end of the script,
/// so a script of nothing but `${` or `` ` `` would otherwise cost the square
/// of its length. Finding the closing delimiter costs nothing from this,
/// however far away it is.
pub(super) const WASTED_SCAN_ALLOWANCE: usize = 2;

/// Whether the bundled shell parser reads `byte` as part of a `$NAME`.
pub(super) fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Byte index of the line break ending a `#` comment whose body starts at
/// `body_start`, or `script.len()` when the comment runs to EOF.
pub(super) fn comment_end(script: &str, body_start: usize) -> usize {
    script[body_start..]
        .find(['\n', '\r'])
        .map_or(script.len(), |offset| body_start + offset)
}

/// Byte index of the `` ` `` closing the backtick substitution at `start`, or
/// `None` when none closes it.
pub(super) fn backtick_end(script: &str, start: usize, allowance: &Cell<usize>) -> Option<usize> {
    if allowance.get() == 0 {
        return None;
    }
    let bytes = script.as_bytes();
    let body_start = start + 1;
    let mut index = body_start;
    if skip_to_closing_backtick(bytes, &mut index) {
        return Some(index - 1);
    }
    deduct_allowance(allowance, bytes.len() - body_start);
    None
}

/// The index of the `}` closing the `${` at `start`, or `None` when the script
/// has none. Within a double-quoted word an apostrophe is an ordinary
/// character unless nested inside `$(...)`, so `"${NAME:-it's fine}"` closes
/// where it looks like it does while `"${NAME:-$(echo '}')}"` reads past the
/// quoted brace.
pub(super) fn braced_parameter_end(
    script: &str,
    start: usize,
    in_double_quotes: bool,
    allowance: &Cell<usize>,
) -> Option<usize> {
    if allowance.get() == 0 {
        return None;
    }
    let bytes = script.as_bytes();
    let body_start = start + 2;
    let mut scanner = BraceScanner::new(in_double_quotes);
    let mut index = body_start;

    while index < bytes.len() {
        let byte = bytes[index];
        index += 1;
        if scanner.step(bytes, byte, &mut index) {
            return Some(index - 1);
        }
    }

    deduct_allowance(allowance, bytes.len() - body_start);
    None
}

fn deduct_allowance(allowance: &Cell<usize>, scanned: usize) {
    allowance.set(allowance.get().saturating_sub(scanned));
}

/// Advance `index` past the closing `` ` ``, skipping backslash-escaped pairs.
/// Returns `true` when the closing `` ` `` was found.
fn skip_to_closing_backtick(bytes: &[u8], index: &mut usize) -> bool {
    while *index < bytes.len() {
        let byte = bytes[*index];
        *index += 1;
        match byte {
            b'\\' => *index = (*index + 1).min(bytes.len()),
            b'`' => return true,
            _ => {}
        }
    }
    false
}

struct ParenFrame {
    in_double_quotes: bool,
    quote: Option<u8>,
    nested_parens: usize,
}

struct BraceScanner {
    in_double_quotes: bool,
    quote: Option<u8>,
    brace_depth: usize,
    nested_parens: usize,
    substitutions: Vec<ParenFrame>,
    at_word_boundary: bool,
}

impl BraceScanner {
    fn new(in_double_quotes: bool) -> Self {
        Self {
            in_double_quotes,
            quote: None,
            brace_depth: 1,
            nested_parens: 0,
            substitutions: Vec::new(),
            at_word_boundary: false,
        }
    }

    fn step(&mut self, bytes: &[u8], byte: u8, index: &mut usize) -> bool {
        if let Some(open) = self.quote {
            self.step_quoted(bytes, open, byte, index);
            return false;
        }
        self.step_unquoted(bytes, byte, index)
    }

    fn step_quoted(&mut self, bytes: &[u8], open: u8, byte: u8, index: &mut usize) {
        if byte == open {
            self.quote = None;
            return;
        }
        // POSIX single quotes give `\`, `$`, and `` ` `` no special meaning, so
        // `'a\'` closes at the second `'`.
        if open != b'"' {
            return;
        }
        match byte {
            b'\\' => *index = (*index + 1).min(bytes.len()),
            b'$' if bytes.get(*index) == Some(&b'(') => self.enter_command_substitution(index),
            b'`' => {
                skip_to_closing_backtick(bytes, index);
            }
            _ => {}
        }
    }

    fn step_unquoted(&mut self, bytes: &[u8], byte: u8, index: &mut usize) -> bool {
        match byte {
            b'\\' => self.skip_escaped(bytes, index),
            b'`' => {
                skip_to_closing_backtick(bytes, index);
                self.at_word_boundary = false;
            }
            b'#' if !self.substitutions.is_empty() && self.at_word_boundary => {
                skip_comment_bytes(bytes, index);
            }
            b'$' if bytes.get(*index) == Some(&b'(') => self.enter_command_substitution(index),
            b'(' if !self.substitutions.is_empty() => {
                self.nested_parens += 1;
                self.at_word_boundary = true;
            }
            b')' if !self.substitutions.is_empty() => self.step_closing_paren(),
            b'"' => {
                self.quote = Some(b'"');
                self.at_word_boundary = false;
            }
            b'\'' if !self.in_double_quotes => {
                self.quote = Some(b'\'');
                self.at_word_boundary = false;
            }
            _ => return self.step_brace_or_plain(byte),
        }
        false
    }

    fn enter_command_substitution(&mut self, index: &mut usize) {
        if self.substitutions.len() >= usize::from(super::MAX_EXPANSION_NESTING) {
            return;
        }
        *index += 1;
        self.substitutions.push(ParenFrame {
            in_double_quotes: self.in_double_quotes,
            quote: self.quote,
            nested_parens: self.nested_parens,
        });
        self.in_double_quotes = false;
        self.quote = None;
        self.nested_parens = 0;
        self.at_word_boundary = true;
    }

    fn step_closing_paren(&mut self) {
        if self.nested_parens > 0 {
            self.nested_parens -= 1;
            self.at_word_boundary = true;
            return;
        }
        if let Some(enclosing) = self.substitutions.pop() {
            self.in_double_quotes = enclosing.in_double_quotes;
            self.quote = enclosing.quote;
            self.nested_parens = enclosing.nested_parens;
            self.at_word_boundary = false;
            return;
        }
        self.at_word_boundary = true;
    }

    fn skip_escaped(&mut self, bytes: &[u8], index: &mut usize) {
        if bytes.get(*index) == Some(&b'\n') {
            *index += 1;
            return;
        }
        if bytes[*index..].starts_with(b"\r\n") {
            *index += 2;
            return;
        }
        *index = (*index + 1).min(bytes.len());
        self.at_word_boundary = false;
    }

    fn step_brace_or_plain(&mut self, byte: u8) -> bool {
        if self.substitutions.is_empty() {
            if byte == b'{' {
                self.brace_depth += 1;
            } else if byte == b'}' {
                self.brace_depth -= 1;
                return self.brace_depth == 0;
            }
        }
        self.at_word_boundary = byte.is_ascii_whitespace() || matches!(byte, b';' | b'&' | b'|');
        false
    }
}

fn skip_comment_bytes(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && !matches!(bytes[*index], b'\n' | b'\r') {
        *index += 1;
    }
}
