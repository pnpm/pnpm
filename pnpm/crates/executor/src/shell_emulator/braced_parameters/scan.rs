//! Forward scans that locate the closing delimiter of `${...}`, `` `...` ``,
//! and `#` comments within the shared [`WASTED_SCAN_ALLOWANCE`].

use std::{cell::Cell, ops::ControlFlow};

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
    if skip_to_closing_backtick(bytes, &mut index).is_break() {
        return Some(index - 1);
    }
    deduct_allowance(allowance, bytes.len() - body_start);
    None
}

/// Whether a `${...}` sits inside enclosing double quotes, which decides
/// whether `'` inside its body is literal text or opens single quotes.
#[derive(Clone, Copy)]
pub(super) enum ParameterQuoting {
    Unquoted,
    DoubleQuoted,
}

/// The index of the `}` closing the `${` at `start`, or `None` when the script
/// has none. Within a double-quoted word an apostrophe is an ordinary
/// character unless nested inside `$(...)`, so `"${NAME:-it's fine}"` closes
/// where it looks like it does while `"${NAME:-$(echo '}')}"` reads past the
/// quoted brace.
pub(super) fn braced_parameter_end(
    script: &str,
    start: usize,
    quoting: ParameterQuoting,
    allowance: &Cell<usize>,
) -> Option<usize> {
    if allowance.get() == 0 {
        return None;
    }
    let bytes = script.as_bytes();
    let body_start = start + 2;
    let mut scanner = BraceScanner::new(quoting);
    let mut index = body_start;

    while index < bytes.len() {
        let byte = bytes[index];
        index += 1;
        if scanner.step(bytes, byte, &mut index).is_break() {
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
fn skip_to_closing_backtick(bytes: &[u8], index: &mut usize) -> ControlFlow<()> {
    while *index < bytes.len() {
        let byte = bytes[*index];
        *index += 1;
        match byte {
            b'\\' => *index = (*index + 1).min(bytes.len()),
            b'`' => return ControlFlow::Break(()),
            _ => {}
        }
    }
    ControlFlow::Continue(())
}

#[derive(Clone, Copy)]
pub(super) enum TokenPosition {
    WordStart,
    MidWord,
}

enum Scope {
    Parameter { quoting: ParameterQuoting, quote: Option<u8>, nested_braces: usize },
    CommandSubstitution { quote: Option<u8>, nested_parens: usize, position: TokenPosition },
}

impl Scope {
    fn parameter(quoting: ParameterQuoting) -> Self {
        Self::Parameter { quoting, quote: None, nested_braces: 0 }
    }

    fn command_substitution() -> Self {
        Self::CommandSubstitution {
            quote: None,
            nested_parens: 0,
            position: TokenPosition::WordStart,
        }
    }

    fn quote(&self) -> Option<u8> {
        match self {
            Self::Parameter { quote, .. } | Self::CommandSubstitution { quote, .. } => *quote,
        }
    }

    fn set_quote(&mut self, next: Option<u8>) {
        match self {
            Self::Parameter { quote, .. } => *quote = next,
            Self::CommandSubstitution { quote, position, .. } => {
                *quote = next;
                *position = TokenPosition::MidWord;
            }
        }
    }

    fn mark_mid_word(&mut self) {
        if let Self::CommandSubstitution { position, .. } = self {
            *position = TokenPosition::MidWord;
        }
    }
}

struct BraceScanner {
    current: Scope,
    enclosing: Vec<Scope>,
}

impl BraceScanner {
    fn new(quoting: ParameterQuoting) -> Self {
        Self { current: Scope::parameter(quoting), enclosing: Vec::new() }
    }

    fn step(&mut self, bytes: &[u8], byte: u8, index: &mut usize) -> ControlFlow<()> {
        if let Some(open) = self.current.quote() {
            self.step_quoted(bytes, open, byte, index);
            return ControlFlow::Continue(());
        }
        match self.current {
            Scope::Parameter { quoting, .. } => self.step_parameter(bytes, byte, quoting, index),
            Scope::CommandSubstitution { position, .. } => {
                self.step_command_substitution(bytes, byte, position, index);
                ControlFlow::Continue(())
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
            b'$' => self.step_dollar(bytes, ParameterQuoting::DoubleQuoted, index),
            b'`' => {
                let _ = skip_to_closing_backtick(bytes, index);
            }
            _ => {}
        }
    }

    fn step_parameter(
        &mut self,
        bytes: &[u8],
        byte: u8,
        quoting: ParameterQuoting,
        index: &mut usize,
    ) -> ControlFlow<()> {
        match byte {
            b'\\' => skip_escaped_bytes(bytes, index),
            b'`' => {
                let _ = skip_to_closing_backtick(bytes, index);
            }
            b'$' => self.step_dollar(bytes, quoting, index),
            b'"' => self.current.set_quote(Some(b'"')),
            b'\'' => match quoting {
                ParameterQuoting::Unquoted => self.current.set_quote(Some(b'\'')),
                ParameterQuoting::DoubleQuoted => {}
            },
            b'{' => self.push_nested_brace(),
            b'}' => return self.pop_brace_or_parameter(),
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn step_command_substitution(
        &mut self,
        bytes: &[u8],
        byte: u8,
        position: TokenPosition,
        index: &mut usize,
    ) {
        match byte {
            b'\\' => self.skip_command_escape(bytes, index),
            b'`' => {
                let _ = skip_to_closing_backtick(bytes, index);
                self.current.mark_mid_word();
            }
            b'#' => match position {
                TokenPosition::WordStart => skip_comment_bytes(bytes, index),
                TokenPosition::MidWord => self.current.mark_mid_word(),
            },
            b'$' => {
                self.current.mark_mid_word();
                self.step_dollar(bytes, ParameterQuoting::Unquoted, index);
            }
            b'(' => self.push_nested_paren(),
            b')' => self.pop_paren_or_substitution(),
            b'"' | b'\'' => self.current.set_quote(Some(byte)),
            _ => self.update_token_position(byte),
        }
    }

    fn step_dollar(&mut self, bytes: &[u8], quoting: ParameterQuoting, index: &mut usize) {
        let next_scope = match bytes.get(*index) {
            Some(b'{') => Scope::parameter(quoting),
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

    fn pop_brace_or_parameter(&mut self) -> ControlFlow<()> {
        if let Scope::Parameter { nested_braces, .. } = &mut self.current
            && *nested_braces > 0
        {
            *nested_braces -= 1;
            return ControlFlow::Continue(());
        }
        let Some(enclosing) = self.enclosing.pop() else {
            return ControlFlow::Break(());
        };
        self.current = enclosing;
        ControlFlow::Continue(())
    }

    fn push_nested_paren(&mut self) {
        if let Scope::CommandSubstitution { nested_parens, position, .. } = &mut self.current {
            *nested_parens += 1;
            *position = TokenPosition::WordStart;
        }
    }

    fn pop_paren_or_substitution(&mut self) {
        if let Scope::CommandSubstitution { nested_parens, position, .. } = &mut self.current
            && *nested_parens > 0
        {
            *nested_parens -= 1;
            *position = TokenPosition::WordStart;
            return;
        }
        if let Some(enclosing) = self.enclosing.pop() {
            self.current = enclosing;
        }
    }

    fn skip_command_escape(&mut self, bytes: &[u8], index: &mut usize) {
        if line_continuation_len(&bytes[*index..]) > 0 {
            *index += line_continuation_len(&bytes[*index..]);
            return;
        }
        *index = (*index + 1).min(bytes.len());
        self.current.mark_mid_word();
    }

    fn update_token_position(&mut self, byte: u8) {
        if let Scope::CommandSubstitution { position, .. } = &mut self.current {
            let is_boundary =
                byte.is_ascii_whitespace() || matches!(byte, b';' | b'&' | b'|' | b'<' | b'>');
            *position = if is_boundary { TokenPosition::WordStart } else { TokenPosition::MidWord };
        }
    }
}

fn skip_escaped_bytes(bytes: &[u8], index: &mut usize) {
    let continuation = line_continuation_len(&bytes[*index..]);
    if continuation > 0 {
        *index += continuation;
    } else {
        *index = (*index + 1).min(bytes.len());
    }
}

fn line_continuation_len(rest: &[u8]) -> usize {
    if rest.starts_with(b"\n") {
        return 1;
    }
    if rest.starts_with(b"\r\n") {
        return 2;
    }
    0
}

fn skip_comment_bytes(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && !matches!(bytes[*index], b'\n' | b'\r') {
        *index += 1;
    }
}
