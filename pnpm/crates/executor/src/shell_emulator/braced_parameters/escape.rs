//! Emitting backslash-escaped pairs and bare-word operators in forms
//! `deno_task_shell`'s parser accepts without losing quoting boundaries.

/// Which double-quoted construct owns the backslash at the cursor.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DoubleQuoteScope {
    /// A `"..."` literal in script text or inside an unquoted expansion word.
    Literal,
    /// The replacement `word` of `"${NAME:-word}"`, where `\}` also strips its
    /// backslash because `}` would otherwise have closed `${...}`.
    ParameterWord,
}

/// Emit the sequence for a `\` inside double quotes, given `rest` starting
/// immediately after the `\`, and return how many bytes of `rest` were consumed.
pub(super) fn push_double_quoted_backslash(
    expanded: &mut String,
    rest: &str,
    scope: DoubleQuoteScope,
) -> usize {
    // `deno_task_shell` does not pair `\\` inside double quotes (it leaves
    // `was_escape` set so `"a\\"` fails to close and `"a\\$VAR"` suppresses
    // `$VAR`). Splicing `"'\'"` keeps the entire run inside one quoted word
    // while handing the parser a single literal backslash.
    if rest.starts_with("\r\n") {
        expanded.push_str("\\\r\n");
        return 2;
    }
    match rest.chars().next() {
        Some('\\') => {
            expanded.push_str(r#""'\'""#);
            1
        }
        Some(escaped @ ('$' | '"' | '`' | '\n')) => {
            expanded.push('\\');
            expanded.push(escaped);
            1
        }
        Some('}') if scope == DoubleQuoteScope::ParameterWord => {
            expanded.push('}');
            1
        }
        Some(_) => {
            expanded.push('\\');
            0
        }
        None if scope == DoubleQuoteScope::ParameterWord => {
            expanded.push_str(r#""'\'""#);
            0
        }
        None => {
            expanded.push('\\');
            0
        }
    }
}

/// Emit the sequence for a `\` in bare `${NAME:-word}` text, given `rest`
/// starting immediately after the `\`, and return how many bytes of `rest`
/// were consumed.
pub(super) fn push_unquoted_word_backslash(expanded: &mut String, rest: &str) -> usize {
    if rest.starts_with("\r\n") {
        return 2;
    }
    if rest.starts_with('\n') {
        return 1;
    }
    let Some(escaped) = rest.chars().next() else {
        push_single_quoted_char(expanded, '\\');
        return 0;
    };
    push_single_quoted_char(expanded, escaped);
    escaped.len_utf8()
}

/// Emit the sequence for an unquoted `\` in script text, given `rest` starting
/// immediately after the `\`, updating `at_word_boundary` and returning how
/// many bytes of `rest` were consumed.
pub(super) fn push_unquoted_script_backslash(
    expanded: &mut String,
    rest: &str,
    at_word_boundary: &mut bool,
) -> usize {
    // `deno_task_shell` neither collapses unquoted `\\` before `$`/ordinary
    // text nor accepts unquoted `\'`, so `\\` and `\'` are rewritten into
    // quoted single-character segments while other `\x` pairs pass through
    // intact without triggering quotes, comments, or expansions.
    if rest.starts_with("\r\n") {
        expanded.push_str("\\\r\n");
        return 2;
    }
    match rest.chars().next() {
        Some('\n') => {
            expanded.push_str("\\\n");
            1
        }
        Some('\\') => {
            expanded.push_str(r"'\'");
            *at_word_boundary = false;
            1
        }
        Some('\'') => {
            expanded.push_str(r#""'""#);
            *at_word_boundary = false;
            1
        }
        Some(escaped) => {
            expanded.push('\\');
            expanded.push(escaped);
            *at_word_boundary = false;
            escaped.len_utf8()
        }
        None => {
            expanded.push('\\');
            *at_word_boundary = false;
            0
        }
    }
}

/// Append a character of an unquoted `word` that carries no meaning of its
/// own, escaping shell operators so the parser reads them as the text POSIX
/// says they are.
pub(super) fn push_bare_word_char(expanded: &mut String, character: char) {
    match character {
        // A newline ends a command, where POSIX only splits the word on it.
        // The character itself does not survive the split, so a space stands
        // in for it and leaves the same two fields behind.
        '\n' => expanded.push(' '),
        // Double quotes rather than a backslash: the parser reads a run of
        // backslashes before an operator by its own rules, so an escape here
        // would depend on what the word happens to put in front of it.
        // Command substitution `$(...)` is rewritten before reaching here, so
        // bare parentheses are literal word text too. `#` joins them because
        // it opens a comment at the start of a word, and a carriage return
        // because the parser splits the word on it where POSIX keeps it as the
        // ordinary character it is.
        ';' | '&' | '|' | '<' | '>' | '(' | ')' | '#' | '\r' => {
            expanded.push('"');
            expanded.push(character);
            expanded.push('"');
        }
        _ => expanded.push(character),
    }
}

/// Quote a single character so the parser reads it as literal word text.
fn push_single_quoted_char(expanded: &mut String, escaped: char) {
    // Single quotes are the form the parser reads as literal whatever it holds;
    // the apostrophe itself is quoted with double quotes instead.
    if escaped == '\'' {
        expanded.push_str(r#""'""#);
        return;
    }
    expanded.push('\'');
    expanded.push(escaped);
    expanded.push('\'');
}
