//! Rewriting the POSIX `${...}` parameter expansions and backslash escapes
//! that the bundled shell parser does not recognize into forms it does.

mod ast;
mod escape;
mod mode;
mod scan;

use mode::{
    Landing,
    Mode,
    Quote,
};
use scan::TokenPosition;
use std::{
    cell::{
        Cell,
        RefCell,
    },
    collections::HashSet,
    ops::ControlFlow,
};

/// Parse shell text with runtime parameter expansions. The lexical rewrite
/// never substitutes environment values into shell source.
pub(super) fn parse(script: &str) -> Result<deno_task_shell::parser::SequentialList, String> {
    let prepared = prepare(script);
    let mut list =
        deno_task_shell::parser::parse(&prepared.text).map_err(|error| error.to_string())?;
    if !prepared.parameters.is_empty() {
        ast::resolve(&mut list, &prepared);
    }
    Ok(list)
}

pub(super) struct PreparedScript {
    pub(super) text: String,
    prefix: String,
    parameters: Vec<Parameter>,
}

struct Parameter {
    name: String,
    empty_is_unset: bool,
    alternate: bool,
    word: String,
    quoted: bool,
}

pub(super) fn prepare(script: &str) -> PreparedScript {
    let allowance = Cell::new(script.len().saturating_mul(scan::WASTED_SCAN_ALLOWANCE));
    let prefix = parameter_prefix(script);
    let parameters = RefCell::new(Vec::new());
    let mut text = String::with_capacity(script.len());
    rewrite_into(
        &mut text,
        script,
        Rewrite {
            parameters: &parameters,
            prefix: &prefix,
            landing: Landing::Script,
            depth: 0,
            wasted_scan_allowance: &allowance,
        },
        StopAt::EndOfInput,
    );
    PreparedScript { text, prefix, parameters: parameters.into_inner() }
}

fn parameter_prefix(script: &str) -> String {
    let occupied: HashSet<&str> = script
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter_map(|word| {
            word.strip_prefix("__PNPM_PARAMETER_")?
                .split_once('_')
                .map(|(id, _)| id)
        })
        .collect();
    let identifier = (0..=occupied.len())
        .find(|identifier| !occupied.contains(identifier.to_string().as_str()))
        .expect("there are more candidates than occupied identifiers");
    format!("__PNPM_PARAMETER_{identifier}_")
}

/// A script is free to nest `${NAME:-${NAME:-...}}` or `$(...)` as deeply as it
/// likes, and the rewrite follows one level per recursion, so a lifecycle
/// script could otherwise overflow the stack and abort the process. No real
/// script comes near this; past it an expansion is left verbatim.
const MAX_EXPANSION_NESTING: u8 = 32;

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
        position: TokenPosition::WordStart,
    };
    cursor.run();
    cursor.index
}

/// What one piece of text is rewritten against.
#[derive(Clone, Copy)]
struct Rewrite<'a> {
    parameters: &'a RefCell<Vec<Parameter>>,
    prefix: &'a str,
    landing: Landing,
    /// How many nested expansions or command substitutions enclose this text.
    depth: u8,
    /// What is left of the allowance, shared by every scan in the script.
    /// Only a scan that finds no closing delimiter draws on it; once it runs
    /// out, an unclosed `${` or `` ` `` is left verbatim without scanning.
    wasted_scan_allowance: &'a Cell<usize>,
}

impl Rewrite<'_> {
    /// The rewrite of the `word` of an expansion sitting at `mode` in this text.
    fn of_a_word_at(self, mode: Mode) -> Self {
        Rewrite { landing: mode.word_landing(), depth: self.depth.saturating_add(1), ..self }
    }

    /// The rewrite of a command substitution (`$(...)` or `` `...` ``), which
    /// runs as its own script regardless of surrounding quotes or words.
    fn nested_script(self) -> Self {
        Rewrite { landing: Landing::Script, depth: self.depth.saturating_add(1), ..self }
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
    position: TokenPosition,
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
            self.position = TokenPosition::MidWord;
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
            '"' => match self.mode {
                Mode::QuotedWord => self.expanded.push_str(r#""""#),
                Mode::Script(Quote::Unquoted | Quote::Single | Quote::Double)
                | Mode::UnquotedWord(Quote::Unquoted | Quote::Single | Quote::Double) => {
                    self.mode = self.mode.with_quote(Quote::Unquoted);
                    self.position = TokenPosition::MidWord;
                    self.expanded.push('"');
                }
            },
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
                    &mut self.position,
                );
            }
            '#' => match self.position {
                TokenPosition::WordStart => self.push_comment(),
                TokenPosition::MidWord => self.expanded.push('#'),
            },
            '\'' => {
                self.mode = Mode::Script(Quote::Single);
                self.position = TokenPosition::MidWord;
                self.expanded.push('\'');
            }
            '"' => {
                self.mode = Mode::Script(Quote::Double);
                self.position = TokenPosition::MidWord;
                self.expanded.push('"');
            }
            '$' => {
                self.push_dollar();
                self.position = TokenPosition::MidWord;
            }
            '`' => {
                self.push_backticks();
                self.position = TokenPosition::MidWord;
            }
            '(' | ')' => return self.step_parenthesis(character),
            _ => {
                self.position = token_position_after(character);
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
        self.position = TokenPosition::WordStart;
        self.expanded.push(character);
        let nested_parens = match &mut self.stop_at {
            StopAt::EndOfInput => return ControlFlow::Continue(()),
            StopAt::ClosingParen { nested_parens } => nested_parens,
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
            Some(b'(') if self.rewrite.depth <= MAX_EXPANSION_NESTING => {
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
        if self.rewrite.depth > MAX_EXPANSION_NESTING {
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

fn token_position_after(character: char) -> TokenPosition {
    let is_boundary = character.is_ascii_whitespace()
        || matches!(character, ';' | '&' | '|' | '<' | '>' | '(' | ')');
    if is_boundary { TokenPosition::WordStart } else { TokenPosition::MidWord }
}

/// Append the replacement for the `${...}` that starts at `start` to
/// `expanded` and return the index just past what was consumed.
fn push_braced_parameter(
    expanded: &mut String,
    script: &str,
    start: usize,
    word: Rewrite<'_>,
) -> usize {
    let quoting = word.landing.parameter_quoting();
    let Some(close) =
        scan::braced_parameter_end(script, start, quoting, word.wasted_scan_allowance)
    else {
        expanded.push('$');
        return start + 1;
    };
    let Some(replacement) = expand_parameter(&script[start + 2..close], word) else {
        escape::push_parameter_literal(expanded, &script[start..=close], word.landing);
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

/// Register a supported parameter expansion and return its placeholder.
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
    let alternate = match operator.as_bytes().first()? {
        b'-' => false,
        b'+' => true,
        _ => return None,
    };
    let mut expanded = String::with_capacity(word.len());
    rewrite_into(&mut expanded, word, rewrite, StopAt::EndOfInput);
    let mut parameters = rewrite.parameters.borrow_mut();
    let name_ref = format!("${}{}", rewrite.prefix, parameters.len());
    parameters.push(Parameter {
        name: name.to_string(),
        empty_is_unset: empty_counts_as_unset,
        alternate,
        word: expanded,
        quoted: matches!(rewrite.landing, Landing::QuotedWord),
    });
    Some(name_ref)
}
