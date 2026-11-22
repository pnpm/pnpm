// Copyright 2018-2025 the Deno authors. MIT license.

use std::{
    ffi::{
        OsStr,
        OsString,
    },
    path::Path,
};

use futures::future::LocalBoxFuture;

use crate::shell::types::{
    ExecuteResult,
    ShellState,
};

use super::{
    ShellCommand,
    ShellCommandContext,
};

/// Implements POSIX `test` for a useful subset of operators: file/string
/// unary predicates, string/integer binary comparisons, and `!` negation.
///
/// Boolean composition with `-a` / `-o` / parens is intentionally not
/// supported — chain separate invocations with `&&` / `||` instead, which
/// is the bash-recommended idiom anyway.
///
/// The `[ ... ]` alias is not exposed as a command name because the shell's
/// glob expansion currently treats an unquoted `[` as the start of a
/// character class. Registering `[` would require teaching the glob
/// expander to fall back to literal on pattern compile errors.
pub struct TestCommand;

impl TestCommand {
    pub fn new_test() -> Self {
        Self
    }
}

impl ShellCommand for TestCommand {
    fn execute(&self, mut context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        let code = match evaluate(&context.args, &context.state) {
            Ok(true) => 0,
            Ok(false) => 1,
            Err(msg) => {
                let _ = context.stderr.write_line(&format!("test: {msg}"));
                2
            }
        };
        Box::pin(futures::future::ready(ExecuteResult::from_exit_code(code)))
    }
}

fn evaluate(args: &[OsString], state: &ShellState) -> Result<bool, String> {
    match args.len() {
        0 => Ok(false),
        1 => Ok(!args[0].is_empty()),
        2 => {
            if args[0] == "!" {
                Ok(args[1].is_empty())
            } else {
                unary(&args[0], &args[1], state)
            }
        }
        3 => {
            if args[0] == "!" {
                Ok(!unary(&args[1], &args[2], state)?)
            } else {
                binary(&args[0], &args[1], &args[2])
            }
        }
        4 if args[0] == "!" => Ok(!binary(&args[1], &args[2], &args[3])?),
        _ => Err("too many arguments".to_string()),
    }
}

fn unary(op: &OsStr, arg: &OsStr, state: &ShellState) -> Result<bool, String> {
    let op_str = op
        .to_str()
        .ok_or_else(|| format!("unary operator expected: {}", op.to_string_lossy()))?;
    match op_str {
        "-n" => Ok(!arg.is_empty()),
        "-z" => Ok(arg.is_empty()),
        "-e" | "-f" | "-d" | "-s" => {
            let path = state.cwd().join(Path::new(arg));
            let metadata = std::fs::metadata(&path).ok();
            Ok(match op_str {
                "-e" => metadata.is_some(),
                "-f" => metadata.is_some_and(|m| m.is_file()),
                "-d" => metadata.is_some_and(|m| m.is_dir()),
                "-s" => metadata.is_some_and(|m| m.len() > 0),
                _ => unreachable!(),
            })
        }
        _ => Err(format!("unary operator expected: {op_str}")),
    }
}

fn binary(lhs: &OsStr, op: &OsStr, rhs: &OsStr) -> Result<bool, String> {
    let op_str = op
        .to_str()
        .ok_or_else(|| format!("binary operator expected: {}", op.to_string_lossy()))?;
    match op_str {
        "=" => Ok(lhs == rhs),
        "!=" => Ok(lhs != rhs),
        "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" => {
            let lhs = parse_int(lhs)?;
            let rhs = parse_int(rhs)?;
            Ok(match op_str {
                "-eq" => lhs == rhs,
                "-ne" => lhs != rhs,
                "-lt" => lhs < rhs,
                "-le" => lhs <= rhs,
                "-gt" => lhs > rhs,
                "-ge" => lhs >= rhs,
                _ => unreachable!(),
            })
        }
        _ => Err(format!("binary operator expected: {op_str}")),
    }
}

fn parse_int(s: &OsStr) -> Result<i64, String> {
    s.to_str()
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| format!("integer expression expected: {}", s.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::types::KillSignal;
    use std::path::PathBuf;

    fn state() -> ShellState {
        state_with_cwd(std::env::current_dir().unwrap())
    }

    fn state_with_cwd(cwd: PathBuf) -> ShellState {
        ShellState::new(Default::default(), cwd, Default::default(), KillSignal::default())
    }

    fn args(parts: &[&str]) -> Vec<OsString> {
        parts
            .iter()
            .map(OsString::from)
            .collect()
    }

    #[test]
    fn zero_args_is_false() {
        assert!(!evaluate(&args(&[]), &state()).unwrap());
    }

    #[test]
    fn single_arg_string_truthiness() {
        assert!(!evaluate(&args(&[""]), &state()).unwrap());
        assert!(evaluate(&args(&["x"]), &state()).unwrap());
        assert!(evaluate(&args(&["0"]), &state()).unwrap());
    }

    #[test]
    fn string_predicates() {
        assert!(evaluate(&args(&["-n", "x"]), &state()).unwrap());
        assert!(!evaluate(&args(&["-n", ""]), &state()).unwrap());
        assert!(evaluate(&args(&["-z", ""]), &state()).unwrap());
        assert!(!evaluate(&args(&["-z", "x"]), &state()).unwrap());
    }

    #[test]
    fn string_comparison() {
        assert!(evaluate(&args(&["a", "=", "a"]), &state()).unwrap());
        assert!(!evaluate(&args(&["a", "=", "b"]), &state()).unwrap());
        assert!(evaluate(&args(&["a", "!=", "b"]), &state()).unwrap());
        assert!(!evaluate(&args(&["a", "!=", "a"]), &state()).unwrap());
    }

    #[test]
    fn integer_comparison() {
        let cases = [
            (&["1", "-eq", "1"][..], true),
            (&["1", "-eq", "2"][..], false),
            (&["1", "-ne", "2"][..], true),
            (&["1", "-lt", "2"][..], true),
            (&["2", "-lt", "1"][..], false),
            (&["2", "-le", "2"][..], true),
            (&["3", "-gt", "2"][..], true),
            (&["2", "-gt", "2"][..], false),
            (&["2", "-ge", "2"][..], true),
            (&["-1", "-lt", "0"][..], true),
        ];
        for (input, expected) in cases {
            assert_eq!(evaluate(&args(input), &state()).unwrap(), expected, "case: {input:?}");
        }
    }

    #[test]
    fn integer_parse_error() {
        let err = evaluate(&args(&["x", "-eq", "1"]), &state()).err().unwrap();
        assert_eq!(err, "integer expression expected: x");
    }

    #[test]
    fn negation() {
        assert!(!evaluate(&args(&["!", "x"]), &state()).unwrap());
        assert!(evaluate(&args(&["!", ""]), &state()).unwrap());
        assert!(!evaluate(&args(&["!", "-n", "x"]), &state()).unwrap());
        assert!(evaluate(&args(&["!", "-z", "x"]), &state()).unwrap());
        assert!(!evaluate(&args(&["!", "1", "-eq", "1"]), &state()).unwrap());
        assert!(evaluate(&args(&["!", "1", "-eq", "2"]), &state()).unwrap());
    }

    #[test]
    fn file_predicates() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("a.txt");
        std::fs::write(&file_path, b"hi").unwrap();
        let empty_path = tmp.path().join("empty.txt");
        std::fs::write(&empty_path, b"").unwrap();
        let dir_path = tmp.path().join("sub");
        std::fs::create_dir(&dir_path).unwrap();
        let missing_path = tmp.path().join("nope");
        let state = state_with_cwd(tmp.path().to_path_buf());

        let cases: &[(&[&str], bool)] = &[
            // -e: exists
            (&["-e", "a.txt"], true),
            (&["-e", "sub"], true),
            (&["-e", "nope"], false),
            // -f: regular file
            (&["-f", "a.txt"], true),
            (&["-f", "sub"], false),
            (&["-f", "nope"], false),
            // -d: directory
            (&["-d", "sub"], true),
            (&["-d", "a.txt"], false),
            (&["-d", "nope"], false),
            // -s: nonzero size
            (&["-s", "a.txt"], true),
            (&["-s", "empty.txt"], false),
            (&["-s", "nope"], false),
        ];
        for (input, expected) in cases {
            assert_eq!(evaluate(&args(input), &state).unwrap(), *expected, "case: {input:?}");
        }

        // absolute paths also work
        assert!(evaluate(&args(&["-f", &file_path.to_string_lossy()]), &state).unwrap());
        assert!(!evaluate(&args(&["-e", &missing_path.to_string_lossy()]), &state).unwrap());
    }

    #[test]
    fn syntax_errors() {
        assert_eq!(
            evaluate(&args(&["-x", "foo"]), &state()).err().unwrap(),
            "unary operator expected: -x"
        );
        assert_eq!(
            evaluate(&args(&["a", "+", "b"]), &state()).err().unwrap(),
            "binary operator expected: +"
        );
        assert_eq!(
            evaluate(&args(&["a", "b", "c", "d", "e"]), &state()).err().unwrap(),
            "too many arguments"
        );
    }
}
