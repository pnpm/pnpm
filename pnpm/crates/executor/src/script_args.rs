use std::{ffi::OsStr, iter, path::Path};

/// How the extra arguments of a script are quoted, chosen by the shell
/// that parses the script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArgQuoting {
    /// `shlex`-style POSIX quoting, for `sh`, a configured non-cmd
    /// `scriptShell`, and the shell emulator.
    Posix,
    /// `cmd` quoting. `double_escape` escapes the arguments a second time
    /// for a script that starts with a batch file, which parses its
    /// arguments again.
    Cmd { double_escape: bool },
}

impl ArgQuoting {
    /// `cmd` quoting for `script`, escaping twice when the command that
    /// receives the arguments resolves to a `.cmd` or `.bat` file in `cwd`
    /// or on `search_path`.
    pub(crate) fn cmd(script: &str, search_path: &OsStr, cwd: &Path) -> Self {
        ArgQuoting::Cmd { double_escape: ends_with_batch_file(script, search_path, cwd) }
    }
}

/// Append `args` to `script` so that the script's command receives each of
/// them unchanged.
pub(crate) fn build_command(script: &str, args: &[String], quoting: ArgQuoting) -> String {
    if args.is_empty() {
        return script.to_string();
    }
    let quoted = args
        .iter()
        .map(|arg| match quoting {
            ArgQuoting::Posix => posix_quote(arg),
            ArgQuoting::Cmd { double_escape } => quote_for_cmd(arg, double_escape),
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("{script} {quoted}")
}

/// Quote a single argument the way the `shlex` npm package's `quote`
/// does: a string of only shell-safe characters is left as-is, anything
/// else is wrapped in single quotes with embedded quotes escaped as
/// `'"'"'`.
pub(crate) fn posix_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    let safe = arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "_@%+=:,./-".contains(ch));
    if safe { arg.to_string() } else { format!("'{}'", arg.replace('\'', r#"'"'"'"#)) }
}

/// Quote `arg` for a `cmd /d /s /c` command line the way npm does. The
/// argument is first quoted for the target program's C runtime, then every
/// character `cmd` would interpret is escaped with `^`.
///
/// `cmd` ends a command line at a line break, so line breaks are passed as
/// the two characters `\n` or `\r`.
pub(crate) fn quote_for_cmd(arg: &str, double_escape: bool) -> String {
    if arg.is_empty() {
        return r#""""#.to_string();
    }
    let arg = arg.replace('\r', r"\r").replace('\n', r"\n");
    let quoted =
        if arg.contains([' ', '\t', '\x0B', '"']) { quote_for_c_runtime(&arg) } else { arg };
    let escaped = caret_escape(&quoted);
    if double_escape { caret_escape(&escaped) } else { escaped }
}

fn caret_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len() * 2);
    for ch in text.chars() {
        if matches!(ch, ' ' | '!' | '%' | '^' | '&' | '(' | ')' | '<' | '>' | '|' | '"') {
            escaped.push('^');
        }
        escaped.push(ch);
    }
    escaped
}

/// Quote `arg` as <https://learn.microsoft.com/en-us/archive/blogs/twistylittlepassagesallalike/everyone-quotes-command-line-arguments-the-wrong-way>
/// describes.
fn quote_for_c_runtime(arg: &str) -> String {
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for ch in arg.chars() {
        if ch == '\\' {
            backslashes += 1;
            continue;
        }
        let count = if ch == '"' { backslashes * 2 + 1 } else { backslashes };
        quoted.push_str(&r"\".repeat(count));
        quoted.push(ch);
        backslashes = 0;
    }
    quoted.push_str(&r"\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

fn ends_with_batch_file(script: &str, search_path: &OsStr, cwd: &Path) -> bool {
    let command = first_word(last_command(script));
    // `cmd` searches the directory it runs in before `PATH`.
    let dirs = iter::once(cwd.to_path_buf()).chain(pnpm_fs::split_paths(search_path));
    let resolved = pnpm_fs::join_paths(dirs)
        .ok()
        .and_then(|dirs| which::which_in(&command, Some(dirs), cwd).ok())
        .map_or_else(|| command.to_lowercase(), |path| path.to_string_lossy().to_lowercase());
    resolved.ends_with(".cmd") || resolved.ends_with(".bat")
}

/// The last command of a chain such as `a && b | c`, which is the one that
/// receives the appended arguments.
fn last_command(script: &str) -> &str {
    let mut start = 0;
    let mut previous = None;
    for (index, ch) in active_chars(script) {
        // An `&` right after a `>` or `<` duplicates a handle, as in `2>&1`.
        let redirection = matches!(previous, Some('>' | '<'));
        if ch == Some('|') || (ch == Some('&') && !redirection) {
            start = index + 1;
        }
        previous = ch;
    }
    &script[start..]
}

/// Each character of `script` with its index, or `None` in place of a
/// quote, a `^`, or a character that quoting or a `^` makes literal.
fn active_chars(script: &str) -> impl Iterator<Item = (usize, Option<char>)> + '_ {
    let mut inside_quotes = false;
    let mut escaped = false;
    script
        .char_indices()
        .map(move |(index, ch)| {
            let active = !escaped && !inside_quotes && ch != '"' && ch != '^';
            if escaped {
                escaped = false;
            } else if ch == '"' {
                inside_quotes = !inside_quotes;
            } else {
                escaped = !inside_quotes && ch == '^';
            }
            (index, active.then_some(ch))
        })
}

fn first_word(command: &str) -> String {
    let mut inside_quotes = false;
    command
        .trim_start()
        .chars()
        .take_while(|&ch| {
            if ch == '"' {
                inside_quotes = !inside_quotes;
            }
            inside_quotes || !matches!(ch, ' ' | '\t')
        })
        .filter(|&ch| ch != '"')
        .collect()
}

#[cfg(test)]
mod tests;
