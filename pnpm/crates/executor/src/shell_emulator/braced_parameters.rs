//! Rewriting the POSIX `${...}` parameter expansions and backslash escapes
//! that the bundled shell parser does not recognize into forms it does.

mod escape;
mod scan;

use escape::DoubleQuoteScope;
use std::{cell::Cell, collections::HashMap, ops::ControlFlow};

/// Rewrite the POSIX `${...}` parameter expansions and backslash escapes (`\\`
/// and unquoted `\'`) that the bundled shell parser does not recognize into
/// forms it does.
///
/// **A value never reaches the rewritten text.** A parameter that has one
/// becomes a `$NAME` reference the shell expands after parsing, so shell
/// punctuation in an environment variable cannot turn into a command, a
/// redirection, or a substitution. Substituting the value here instead would
/// read the same and be a command injection.
///
/// Which side of a `${NAME:-word}` or `${NAME:+word}` is taken is read from
/// `env`, the environment the shell starts with, so a parameter that an
/// earlier command in the same script assigned is not seen. Forms no `$NAME`
/// reference can stand in for, `${NAME:=word}` and `${#NAME}` among them, are
/// left verbatim rather than guessed at.
pub(super) fn expand(script: &str, env: &HashMap<String, String>) -> String {
    let allowance = Cell::new(script.len().saturating_mul(scan::WASTED_SCAN_ALLOWANCE));
    let mut expanded = String::with_capacity(script.len());
    rewrite_into(
        &mut expanded,
        script,
        Rewrite::of_the_script(env, &allowance),
        StopAt::EndOfInput,
    );
    expanded
}

/// A script is free to nest `${NAME:-${NAME:-...}}` or `$(...)` as deeply as it
/// likes, and the rewrite follows one level per recursion, so a lifecycle
/// script could otherwise overflow the stack and abort the process. No real
/// script comes near this; past it an expansion is left verbatim.
pub(super) const MAX_EXPANSION_NESTING: u8 = 32;

/// Rewrite `script` into `expanded`, stopping at `stop_at`, and return how
/// many bytes of `script` were consumed.
fn rewrite_into(
    expanded: &mut String,
    script: &str,
    rewrite: Rewrite<'_>,
    stop_at: StopAt,
) -> usize {
    let mut cursor = Cursor {
        script,
        rewrite,
        stop_at,
        expanded,
        index: 0,
        mode: rewrite.landing.initial_mode(),
        at_word_boundary: true,
    };
    cursor.run();
    cursor.index
}

/// What one piece of text is rewritten against.
#[derive(Clone, Copy)]
struct Rewrite<'a> {
    env: &'a HashMap<String, String>,
    landing: Landing,
    /// How many nested expansions or command substitutions enclose this text.
    depth: u8,
    /// What is left of the allowance, shared by every scan in the script.
    /// Only a scan that finds no closing delimiter draws on it; once it runs
    /// out, an unclosed `${` or `` ` `` is left verbatim without scanning.
    wasted_scan_allowance: &'a Cell<usize>,
}

impl<'a> Rewrite<'a> {
    fn of_the_script(env: &'a HashMap<String, String>, allowance: &'a Cell<usize>) -> Self {
        Rewrite { env, landing: Landing::Script, depth: 0, wasted_scan_allowance: allowance }
    }

    /// The rewrite of the `word` of an expansion sitting at `mode` in this text.
    fn of_a_word_at(self, mode: Mode) -> Self {
        let landing =
            if mode.is_double_quoted() { Landing::QuotedWord } else { Landing::UnquotedWord };
        Rewrite { landing, depth: self.depth.saturating_add(1), ..self }
    }

    /// The rewrite of a command substitution (`$(...)` or `` `...` ``), which
    /// runs as its own script regardless of surrounding quotes or words.
    fn nested_script(self) -> Self {
        Rewrite { landing: Landing::Script, depth: self.depth.saturating_add(1), ..self }
    }
}

/// Where the text being rewritten ends up in the finished script, which is
/// what decides whether an operator in it is syntax or word text.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Landing {
    Script,
    /// POSIX makes an operator here part of the word, so it is escaped rather
    /// than spliced in as the command, pipeline, or redirection the parser
    /// would otherwise read.
    UnquotedWord,
    /// The surrounding double quotes already make an operator here literal.
    QuotedWord,
}

impl Landing {
    fn initial_mode(self) -> Mode {
        match self {
            Landing::Script => Mode::Script(Quote::Unquoted),
            Landing::UnquotedWord => Mode::UnquotedWord(Quote::Unquoted),
            Landing::QuotedWord => Mode::QuotedWord,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quote {
    Unquoted,
    Single,
    Double,
}

/// Active lexical context inside [`Cursor`]. [`Mode::QuotedWord`] stays inside
/// its enclosing double quotes for the entire `word`, so only [`Mode::Script`]
/// and [`Mode::UnquotedWord`] transition between quotes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Script(Quote),
    UnquotedWord(Quote),
    QuotedWord,
}

impl Mode {
    fn is_double_quoted(self) -> bool {
        match self {
            Mode::Script(Quote::Double) | Mode::UnquotedWord(Quote::Double) | Mode::QuotedWord => {
                true
            }
            Mode::Script(Quote::Unquoted | Quote::Single)
            | Mode::UnquotedWord(Quote::Unquoted | Quote::Single) => false,
        }
    }

    fn with_quote(self, quote: Quote) -> Self {
        match self {
            Mode::Script(_) => Mode::Script(quote),
            Mode::UnquotedWord(_) => Mode::UnquotedWord(quote),
            Mode::QuotedWord => Mode::QuotedWord,
        }
    }

    fn double_quote_scope(self) -> DoubleQuoteScope {
        match self {
            Mode::QuotedWord => DoubleQuoteScope::ParameterWord,
            Mode::Script(_) | Mode::UnquotedWord(_) => DoubleQuoteScope::Literal,
        }
    }
}

enum StopAt {
    EndOfInput,
    ClosingParen { nested_parens: usize },
}

struct Cursor<'a, 'out> {
    script: &'a str,
    rewrite: Rewrite<'a>,
    stop_at: StopAt,
    expanded: &'out mut String,
    index: usize,
    mode: Mode,
    at_word_boundary: bool,
}

impl Cursor<'_, '_> {
    fn run(&mut self) {
        while self.index < self.script.len() {
            let character = self.script[self.index..]
                .chars()
                .next()
                .expect("index sits on a character boundary");
            self.index += character.len_utf8();
            if self.step(character).is_break() {
                break;
            }
        }
    }

    fn step(&mut self, character: char) -> ControlFlow<()> {
        match self.mode {
            Mode::Script(Quote::Single) | Mode::UnquotedWord(Quote::Single) => {
                self.step_single_quoted(character);
            }
            Mode::Script(Quote::Double) | Mode::UnquotedWord(Quote::Double) | Mode::QuotedWord => {
                self.step_double_quoted(character);
            }
            Mode::UnquotedWord(Quote::Unquoted) => self.step_unquoted_word(character),
            Mode::Script(Quote::Unquoted) => return self.step_unquoted_script(character),
        }
        ControlFlow::Continue(())
    }

    fn step_single_quoted(&mut self, character: char) {
        if character == '\'' {
            self.mode = self.mode.with_quote(Quote::Unquoted);
            self.at_word_boundary = false;
        }
        self.expanded.push(character);
    }

    fn step_double_quoted(&mut self, character: char) {
        match character {
            '\\' => {
                self.index += escape::push_double_quoted_backslash(
                    self.expanded,
                    &self.script[self.index..],
                    self.mode.double_quote_scope(),
                );
            }
            // Within `"${NAME:-"word"}"`, nested quotes delimit `word` while
            // the outer double quotes stay open across the whole expansion.
            // Splicing `""` closes and reopens the quoted run so `$FOO` in
            // `"${MISSING:-"$FOO"suffix}"` does not merge with `suffix`.
            '"' if self.mode == Mode::QuotedWord => self.expanded.push_str(r#""""#),
            '"' => {
                self.mode = self.mode.with_quote(Quote::Unquoted);
                self.at_word_boundary = false;
                self.expanded.push('"');
            }
            '$' => self.push_dollar(),
            '`' => self.push_backticks(),
            _ => self.expanded.push(character),
        }
    }

    fn step_unquoted_word(&mut self, character: char) {
        match character {
            '\\' => {
                self.index +=
                    escape::push_unquoted_word_backslash(self.expanded, &self.script[self.index..]);
            }
            '\'' => {
                self.mode = Mode::UnquotedWord(Quote::Single);
                self.expanded.push('\'');
            }
            '"' => {
                self.mode = Mode::UnquotedWord(Quote::Double);
                self.expanded.push('"');
            }
            '$' => self.push_dollar(),
            '`' => self.push_backticks(),
            _ => escape::push_bare_word_char(self.expanded, character),
        }
    }

    fn step_unquoted_script(&mut self, character: char) -> ControlFlow<()> {
        match character {
            '\\' => {
                self.index += escape::push_unquoted_script_backslash(
                    self.expanded,
                    &self.script[self.index..],
                    &mut self.at_word_boundary,
                );
            }
            '#' if self.at_word_boundary => self.push_comment(),
            '\'' => {
                self.mode = Mode::Script(Quote::Single);
                self.at_word_boundary = false;
                self.expanded.push('\'');
            }
            '"' => {
                self.mode = Mode::Script(Quote::Double);
                self.at_word_boundary = false;
                self.expanded.push('"');
            }
            '$' => {
                self.push_dollar();
                self.at_word_boundary = false;
            }
            '`' => {
                self.push_backticks();
                self.at_word_boundary = false;
            }
            '(' | ')' => return self.step_parenthesis(character),
            _ => {
                self.at_word_boundary = is_word_boundary(character);
                self.expanded.push(character);
            }
        }
        ControlFlow::Continue(())
    }

    fn push_comment(&mut self) {
        let end = scan::comment_end(self.script, self.index);
        self.expanded.push('#');
        self.expanded.push_str(&self.script[self.index..end]);
        self.index = end;
    }

    fn step_parenthesis(&mut self, character: char) -> ControlFlow<()> {
        self.at_word_boundary = true;
        self.expanded.push(character);
        let StopAt::ClosingParen { nested_parens } = &mut self.stop_at else {
            return ControlFlow::Continue(());
        };
        if character == '(' {
            *nested_parens += 1;
            return ControlFlow::Continue(());
        }
        if *nested_parens == 0 {
            return ControlFlow::Break(());
        }
        *nested_parens -= 1;
        ControlFlow::Continue(())
    }

    fn push_dollar(&mut self) {
        match self.script
            .as_bytes()
            .get(self.index)
            .copied()
        {
            Some(b'{') => {
                let start = self.index - 1;
                self.index = push_braced_parameter(
                    self.expanded,
                    self.script,
                    start,
                    self.rewrite.of_a_word_at(self.mode),
                );
            }
            Some(b'(') if self.rewrite.depth < MAX_EXPANSION_NESTING => {
                self.index += 1;
                self.expanded.push_str("$(");
                self.index += rewrite_into(
                    self.expanded,
                    &self.script[self.index..],
                    self.rewrite.nested_script(),
                    StopAt::ClosingParen { nested_parens: 0 },
                );
            }
            _ => self.expanded.push('$'),
        }
    }

    fn push_backticks(&mut self) {
        if self.rewrite.depth >= MAX_EXPANSION_NESTING {
            self.expanded.push('`');
            return;
        }
        let start = self.index - 1;
        let allowance = self.rewrite.wasted_scan_allowance;
        let Some(close) = scan::backtick_end(self.script, start, allowance) else {
            self.expanded.push('`');
            return;
        };
        self.expanded.push('`');
        rewrite_into(
            self.expanded,
            &self.script[self.index..close],
            self.rewrite.nested_script(),
            StopAt::EndOfInput,
        );
        self.expanded.push('`');
        self.index = close + 1;
    }
}

fn is_word_boundary(character: char) -> bool {
    character.is_ascii_whitespace() || matches!(character, ';' | '&' | '|' | '(' | ')')
}

/// Append the replacement for the `${...}` that starts at `start` to
/// `expanded` and return the index just past what was consumed.
fn push_braced_parameter(
    expanded: &mut String,
    script: &str,
    start: usize,
    word: Rewrite<'_>,
) -> usize {
    let in_double_quotes = word.landing == Landing::QuotedWord;
    let Some(close) =
        scan::braced_parameter_end(script, start, in_double_quotes, word.wasted_scan_allowance)
    else {
        expanded.push('$');
        return start + 1;
    };
    let Some(replacement) = expand_parameter(&script[start + 2..close], word) else {
        expanded.push_str(&script[start..=close]);
        return close + 1;
    };

    expanded.push_str(&replacement);
    // `$NAME` swallows every name byte after it, so an expansion glued to more
    // of the same word is closed off with an empty string, which joins the
    // word without contributing to it.
    if script
        .as_bytes()
        .get(close + 1)
        .copied()
        .is_some_and(scan::is_name_byte)
    {
        expanded.push_str(r#""""#);
    }
    close + 1
}

/// The replacement text for the body of a `${...}`, or `None` for a body that
/// no `$NAME` reference can stand in for.
fn expand_parameter(body: &str, rewrite: Rewrite<'_>) -> Option<String> {
    if rewrite.depth > MAX_EXPANSION_NESTING {
        return None;
    }
    let name_length = body
        .bytes()
        .take_while(|byte| scan::is_name_byte(*byte))
        .count();
    let (name, operator) = body.split_at(name_length);
    if name.is_empty() {
        return None;
    }
    if operator.is_empty() {
        return Some(format!("${name}"));
    }

    // A leading `:` makes an empty value count as unset, as POSIX specifies.
    let (operator, empty_counts_as_unset) = match operator.strip_prefix(':') {
        Some(rest) => (rest, true),
        None => (operator, false),
    };
    let word = operator.get(1..)?;
    let has_value = rewrite.env
        .get(name)
        .is_some_and(|value| !empty_counts_as_unset || !value.is_empty());

    match (operator.as_bytes().first()?, has_value) {
        (b'-', true) => Some(format!("${name}")),
        (b'-', false) | (b'+', true) => {
            let mut expanded = String::with_capacity(word.len());
            rewrite_into(&mut expanded, word, rewrite, StopAt::EndOfInput);
            Some(expanded)
        }
        (b'+', false) => Some(String::new()),
        _ => None,
    }
}
