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
        .map(|offset| body_start + offset)
        .unwrap_or(script.len())
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

enum Scope {
    Parameter { in_double_quotes: bool, quote: Option<u8>, nested_braces: usize },
    CommandSubstitution { quote: Option<u8>, nested_parens: usize, at_word_boundary: bool },
}

impl Scope {
    fn parameter(in_double_quotes: bool) -> Self {
        Self::Parameter { in_double_quotes, quote: None, nested_braces: 0 }
    }

    fn command_substitution() -> Self {
        Self::CommandSubstitution { quote: None, nested_parens: 0, at_word_boundary: true }
    }

    fn quote(&self) -> Option<u8> {
        match self {
            Self::Parameter { quote, .. } | Self::CommandSubstitution { quote, .. } => *quote,
        }
    }

    fn set_quote(&mut self, next: Option<u8>) {
        match self {
            Self::Parameter { quote, .. } => *quote = next,
            Self::CommandSubstitution { quote, at_word_boundary, .. } => {
                *quote = next;
                *at_word_boundary = false;
            }
        }
    }

    fn clear_word_boundary(&mut self) {
        if let Self::CommandSubstitution { at_word_boundary, .. } = self {
            *at_word_boundary = false;
        }
    }
}

struct BraceScanner {
    current: Scope,
    enclosing: Vec<Scope>,
}

impl BraceScanner {
    fn new(in_double_quotes: bool) -> Self {
        Self { current: Scope::parameter(in_double_quotes), enclosing: Vec::new() }
    }

    fn step(&mut self, bytes: &[u8], byte: u8, index: &mut usize) -> bool {
        if let Some(open) = self.current.quote() {
            self.step_quoted(bytes, open, byte, index);
            return false;
        }
        match self.current {
            Scope::Parameter { in_double_quotes, .. } => {
                self.step_parameter(bytes, byte, in_double_quotes, index)
            }
            Scope::CommandSubstitution { at_word_boundary, .. } => {
                self.step_command_substitution(bytes, byte, at_word_boundary, index);
                false
            }
        }
    }

    fn step_quoted(&mut self, bytes: &[u8], open: u8, byte: u8, index: &mut usize) {
        if byte == open {
            self.current.set_quote(None);
            return;
        }
        // POSIX single quotes give `\`, `$`, and `` ` `` no special meaning, so
        // `'a\'` closes at the second `'`.
        if open != b'"' {
            return;
        }
        match byte {
            b'\\' => *index = (*index + 1).min(bytes.len()),
            b'$' => self.step_dollar(bytes, true, index),
            b'`' => {
                skip_to_closing_backtick(bytes, index);
            }
            _ => {}
        }
    }

    fn step_parameter(
        &mut self,
        bytes: &[u8],
        byte: u8,
        in_double_quotes: bool,
        index: &mut usize,
    ) -> bool {
        match byte {
            b'\\' => skip_escaped_bytes(bytes, index),
            b'`' => {
                skip_to_closing_backtick(bytes, index);
            }
            b'$' => self.step_dollar(bytes, in_double_quotes, index),
            b'"' => self.current.set_quote(Some(b'"')),
            b'\'' if !in_double_quotes => self.current.set_quote(Some(b'\'')),
            b'{' => self.push_nested_brace(),
            b'}' => return self.pop_brace_or_parameter(),
            _ => {}
        }
        false
    }

    fn step_command_substitution(
        &mut self,
        bytes: &[u8],
        byte: u8,
        at_word_boundary: bool,
        index: &mut usize,
    ) {
        match byte {
            b'\\' => self.skip_command_escape(bytes, index),
            b'`' => {
                skip_to_closing_backtick(bytes, index);
                self.current.clear_word_boundary();
            }
            b'#' if at_word_boundary => skip_comment_bytes(bytes, index),
            b'$' => {
                self.current.clear_word_boundary();
                self.step_dollar(bytes, false, index);
            }
            b'(' => self.push_nested_paren(),
            b')' => self.pop_paren_or_substitution(),
            b'"' | b'\'' => self.current.set_quote(Some(byte)),
            _ => self.update_word_boundary(byte),
        }
    }

    fn step_dollar(&mut self, bytes: &[u8], in_double_quotes: bool, index: &mut usize) {
        let next_scope = match bytes.get(*index) {
            Some(b'{') => Scope::parameter(in_double_quotes),
            Some(b'(') => Scope::command_substitution(),
            _ => return,
        };
        *index += 1;
        let previous = std::mem::replace(&mut self.current, next_scope);
        self.enclosing.push(previous);
    }

    fn push_nested_brace(&mut self) {
        if let Scope::Parameter { nested_braces, .. } = &mut self.current {
            *nested_braces += 1;
        }
    }

    fn pop_brace_or_parameter(&mut self) -> bool {
        if let Scope::Parameter { nested_braces, .. } = &mut self.current
            && *nested_braces > 0
        {
            *nested_braces -= 1;
            return false;
        }
        let Some(enclosing) = self.enclosing.pop() else {
            return true;
        };
        self.current = enclosing;
        false
    }

    fn push_nested_paren(&mut self) {
        if let Scope::CommandSubstitution { nested_parens, at_word_boundary, .. } =
            &mut self.current
        {
            *nested_parens += 1;
            *at_word_boundary = true;
        }
    }

    fn pop_paren_or_substitution(&mut self) {
        if let Scope::CommandSubstitution { nested_parens, at_word_boundary, .. } =
            &mut self.current
            && *nested_parens > 0
        {
            *nested_parens -= 1;
            *at_word_boundary = true;
            return;
        }
        if let Some(enclosing) = self.enclosing.pop() {
            self.current = enclosing;
        }
    }

    fn skip_command_escape(&mut self, bytes: &[u8], index: &mut usize) {
        if skip_line_continuation(bytes, index) {
            return;
        }
        *index = (*index + 1).min(bytes.len());
        self.current.clear_word_boundary();
    }

    fn update_word_boundary(&mut self, byte: u8) {
        if let Scope::CommandSubstitution { at_word_boundary, .. } = &mut self.current {
            *at_word_boundary =
                byte.is_ascii_whitespace() || matches!(byte, b';' | b'&' | b'|' | b'<' | b'>');
        }
    }
}

fn skip_escaped_bytes(bytes: &[u8], index: &mut usize) {
    if !skip_line_continuation(bytes, index) {
        *index = (*index + 1).min(bytes.len());
    }
}

fn skip_line_continuation(bytes: &[u8], index: &mut usize) -> bool {
    if bytes.get(*index) == Some(&b'\n') {
        *index += 1;
        return true;
    }
    if bytes[*index..].starts_with(b"\r\n") {
        *index += 2;
        return true;
    }
    false
}

fn skip_comment_bytes(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && !matches!(bytes[*index], b'\n' | b'\r') {
        *index += 1;
    }
}
