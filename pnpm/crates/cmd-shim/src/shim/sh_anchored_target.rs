use super::relocatable::BASEDIR_ABS_PRELUDE;

const QUOTED_BASEDIR_ABS: &str = r#""$basedir_abs"/"#;
const LEGACY_QUOTED_BASEDIR_ABS: &str = r#""$basedir_abs/"#;

#[must_use]
pub fn is_sh_shim_basedir_anchor_current(existing: &str, expected: &str) -> bool {
    existing.contains(BASEDIR_ABS_PRELUDE) == expected.contains(BASEDIR_ABS_PRELUDE)
        && anchored_target(existing) == anchored_target(expected)
}

fn anchored_target(shim: &str) -> Option<&str> {
    let marker = shim.find("# cmd-shim-target=")?;
    ExecTargetScanner::new(&shim[..marker]).find_target()
}

struct ExecTargetScanner<'a> {
    header: &'a str,
    index: usize,
    line_start: usize,
    quote: Option<u8>,
    in_exec: bool,
}

impl<'a> ExecTargetScanner<'a> {
    fn new(header: &'a str) -> Self {
        Self { header, index: 0, line_start: 0, quote: None, in_exec: false }
    }

    fn find_target(mut self) -> Option<&'a str> {
        while self.index < self.header.len() {
            if let Some(target) = self.anchored_word() {
                return Some(target);
            }
            self.advance();
        }
        None
    }

    fn anchored_word(&self) -> Option<&'a str> {
        if self.quote.is_some() || !self.in_exec {
            return None;
        }
        let rest = &self.header[self.index..];
        if let Some(target) = rest.strip_prefix(QUOTED_BASEDIR_ABS) {
            return sh_single_quoted_word(target);
        }
        rest.strip_prefix(LEGACY_QUOTED_BASEDIR_ABS).and_then(legacy_double_quoted_word)
    }

    fn advance(&mut self) {
        match self.quote {
            Some(b'\'') => self.advance_single_quote(),
            Some(delimiter @ (b'"' | b'`')) => self.advance_delimited_quote(delimiter),
            Some(_) => unreachable!(),
            None => self.advance_unquoted(),
        }
    }

    fn advance_single_quote(&mut self) {
        let byte = self.header.as_bytes()[self.index];
        if byte == b'\'' {
            self.quote = None;
            self.index += 1;
        } else if byte == b'\n' {
            self.line_start = self.index + 1;
            self.index += 1;
        } else {
            self.index += self.current_char_len();
        }
    }

    fn advance_delimited_quote(&mut self, delimiter: u8) {
        let byte = self.header.as_bytes()[self.index];
        if byte == b'\\' {
            let next_index = self.index + 1;
            let next = self.header[next_index..]
                .chars()
                .next()
                .map_or(0, char::len_utf8);
            if self.header.as_bytes().get(next_index) == Some(&b'\n') {
                self.line_start = next_index + 1;
            }
            self.index = next_index + next;
        } else if byte == delimiter {
            self.quote = None;
            self.index += 1;
        } else if byte == b'\n' {
            self.line_start = self.index + 1;
            self.index += 1;
        } else {
            self.index += self.current_char_len();
        }
    }

    fn advance_unquoted(&mut self) {
        let byte = self.header.as_bytes()[self.index];
        if byte == b'\\' {
            let next_index = self.index + 1;
            let next = self.header[next_index..]
                .chars()
                .next()
                .map_or(0, char::len_utf8);
            if self.header.as_bytes().get(next_index) == Some(&b'\n') {
                self.line_start = next_index + 1;
            }
            self.index = next_index + next;
        } else if byte == b'\n' {
            self.line_start = self.index + 1;
            self.in_exec = false;
            self.index += 1;
        } else if self.starts_shell_comment() {
            self.index = self.header[self.index..]
                .find('\n')
                .map_or(self.header.len(), |newline| self.index + newline);
        } else if matches!(byte, b'\'' | b'"' | b'`') {
            self.quote = Some(byte);
            self.index += 1;
        } else if self.starts_exec_command() {
            self.in_exec = true;
            self.index += "exec ".len();
        } else {
            self.index += self.current_char_len();
        }
    }

    fn starts_exec_command(&self) -> bool {
        self.header[self.line_start..self.index].trim().is_empty()
            && self.header[self.index..].starts_with("exec ")
    }

    fn starts_shell_comment(&self) -> bool {
        self.header.as_bytes()[self.index] == b'#'
            && (self.index == self.line_start
                || self.header[self.line_start..self.index]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace))
    }

    fn current_char_len(&self) -> usize {
        self.header[self.index..]
            .chars()
            .next()
            .unwrap()
            .len_utf8()
    }
}

fn legacy_double_quoted_word(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 1 + text[index + 1..]
                .chars()
                .next()?
                .len_utf8();
        } else if bytes[index] == b'"' {
            return Some(&text[..index]);
        } else {
            index += text[index..].chars().next()?.len_utf8();
        }
    }
    None
}

fn sh_single_quoted_word(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'\'') {
        return None;
    }
    let mut index = 1;
    let mut quoted = true;
    while index < bytes.len() {
        match (quoted, bytes[index]) {
            (true, b'\'') => quoted = false,
            (true, _) => index += text[index..].chars().next()?.len_utf8() - 1,
            (false, b'\\') if bytes.get(index + 1) == Some(&b'\'') => index += 1,
            (false, b'\'') => quoted = true,
            (false, byte) if byte.is_ascii_whitespace() => return text.get(..index),
            (false, _) => return None,
        }
        index += 1;
    }
    (!quoted).then_some(text)
}
