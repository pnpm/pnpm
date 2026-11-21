// Copyright 2018-2025 the Deno authors. MIT license.

use std::ffi::OsString;

use futures::{
    FutureExt,
    future::LocalBoxFuture,
};

use crate::{
    ExecuteCommandArgsContext,
    shell::types::{
        ExecuteResult,
        ShellPipeReader,
    },
};

use super::{
    ShellCommand,
    ShellCommandContext,
    args::{
        ArgKind,
        parse_arg_kinds,
    },
    error::{
        ShellCommandError,
        bail,
    },
};

pub struct XargsCommand;

impl ShellCommand for XargsCommand {
    fn execute(&self, mut context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        async move {
            match xargs_collect_args(&context.args, context.stdin.clone()) {
                Ok(args) => {
                    // don't select on cancellation here as that will occur at a lower level
                    (context.execute_command_args)(ExecuteCommandArgsContext {
                        args,
                        state: context.state,
                        stdin: context.stdin,
                        stdout: context.stdout,
                        stderr: context.stderr,
                    })
                    .await
                }
                Err(err) => {
                    let _ = context.stderr.write_line(&format!("xargs: {err}"));
                    ExecuteResult::from_exit_code(1)
                }
            }
        }
        .boxed_local()
    }
}

fn xargs_collect_args(
    cli_args: &[OsString],
    stdin: ShellPipeReader,
) -> Result<Vec<OsString>, ShellCommandError> {
    let flags = parse_args(cli_args)?;
    let mut buf = Vec::new();
    stdin.pipe_to(&mut buf)?;
    let text = String::from_utf8(buf)?;
    let mut args = flags.initial_args;

    if args.is_empty() {
        // defaults to echo
        args.push("echo".into());
    }

    if flags.delimiter.is_some() || flags.is_null_delimited {
        let delimiter = flags.delimiter.unwrap_or('\0');
        args.extend(text.split(delimiter).map(|t| t.into()));

        // remove last arg if it is empty
        if let Some(last) = args.last()
            && last.is_empty()
        {
            args.pop();
        }
    } else {
        args.extend(delimit_blanks(&text)?);
    }

    Ok(args)
}

fn delimit_blanks(text: &str) -> Result<Vec<OsString>, ShellCommandError> {
    let mut chars = text.chars().peekable();
    let mut result = Vec::new();
    while chars.peek().is_some() {
        let mut current = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\n' | '\t' | ' ' => break,
                '"' | '\'' => {
                    const UNMATCHED_MESSAGE: &str = "unmatched quote; by default quotes are special to xargs unless you use the -0 option";
                    let original_quote_char = c;
                    while let Some(c) = chars.next() {
                        if c == original_quote_char {
                            break;
                        }
                        match c {
                            '\n' => bail!("{}", UNMATCHED_MESSAGE),
                            _ => current.push(c),
                        }
                        if chars.peek().is_none() {
                            bail!("{}", UNMATCHED_MESSAGE)
                        }
                    }
                }
                '\\' => {
                    if matches!(chars.peek(), Some('\n' | '\t' | ' ' | '"' | '\'')) {
                        current.push(chars.next().unwrap());
                    } else {
                        current.push(c);
                    }
                }
                _ => current.push(c),
            }
        }

        if !current.is_empty() {
            result.push(current.into());
        }
    }
    Ok(result)
}

#[derive(Debug, PartialEq)]
struct XargsFlags {
    initial_args: Vec<OsString>,
    delimiter: Option<char>,
    is_null_delimited: bool,
}

fn parse_args(args: &[OsString]) -> Result<XargsFlags, ShellCommandError> {
    fn parse_delimiter(arg: &str) -> Result<char, ShellCommandError> {
        let mut chars = arg.chars();
        if let Some(first_char) = chars.next() {
            let mut delimiter = first_char;
            if first_char == '\\' {
                delimiter = match chars.next() {
                    // todo(dsherret): support more
                    Some('n') => '\n',
                    Some('r') => '\r',
                    Some('t') => '\t',
                    Some('\\') => '\\',
                    Some('0') => '\0',
                    None => bail!("expected character following escape"),
                    _ => bail!("unsupported/not implemented escape character"),
                };
            }

            if chars.next().is_some() {
                bail!("expected a single byte char delimiter. Found: {}", arg);
            }

            Ok(delimiter)
        } else {
            bail!("expected non-empty delimiter");
        }
    }

    let mut initial_args = Vec::new();
    let mut delimiter = None;
    let mut iterator = parse_arg_kinds(args).into_iter();
    let mut is_null_delimited = false;
    while let Some(arg) = iterator.next() {
        match arg {
            ArgKind::Arg(arg) => {
                if arg == "-0" {
                    is_null_delimited = true;
                } else {
                    initial_args.push(arg.to_os_string());
                    // parse the remainder as arguments
                    for arg in iterator.by_ref() {
                        match arg {
                            ArgKind::Arg(arg) => {
                                initial_args.push(arg.to_os_string());
                            }
                            ArgKind::ShortFlag(f) => initial_args.push(format!("-{f}").into()),
                            ArgKind::LongFlag(f) => initial_args.push(format!("--{f}").into()),
                        }
                    }
                }
            }
            ArgKind::LongFlag("null") => {
                is_null_delimited = true;
            }
            ArgKind::ShortFlag('d') => match iterator.next() {
                Some(ArgKind::Arg(arg)) => {
                    if let Some(arg) = arg.to_str() {
                        delimiter = Some(parse_delimiter(arg)?);
                    } else {
                        bail!("expected delimiter argument following -d to be UTF-8")
                    }
                }
                _ => bail!("expected delimiter argument following -d"),
            },
            ArgKind::LongFlag(flag) => {
                if let Some(arg) = flag.strip_prefix("delimiter=") {
                    delimiter = Some(parse_delimiter(arg)?);
                } else {
                    arg.bail_unsupported()?
                }
            }
            _ => arg.bail_unsupported()?,
        }
    }

    if is_null_delimited && delimiter.is_some() {
        bail!("cannot specify both null and delimiter flag")
    }

    Ok(XargsFlags { initial_args, delimiter, is_null_delimited })
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_args() {
        assert_eq!(
            parse_args(&[]).unwrap(),
            XargsFlags { initial_args: Vec::new(), delimiter: None, is_null_delimited: false }
        );
        assert_eq!(
            parse_args(&["-0".into(), "echo".into(), "2".into(), "-d".into(), "--test=3".into()])
                .unwrap(),
            XargsFlags {
                initial_args: vec!["echo".into(), "2".into(), "-d".into(), "--test=3".into()],
                delimiter: None,
                is_null_delimited: true,
            }
        );
        assert_eq!(
            parse_args(&["-d".into(), "\\n".into(), "echo".into()]).unwrap(),
            XargsFlags {
                initial_args: vec!["echo".into()],
                delimiter: Some('\n'),
                is_null_delimited: false,
            }
        );
        assert_eq!(
            parse_args(&["--delimiter=5".into(), "echo".into(), "-d".into()]).unwrap(),
            XargsFlags {
                initial_args: vec!["echo".into(), "-d".into()],
                delimiter: Some('5'),
                is_null_delimited: false,
            }
        );
        assert_eq!(
            parse_args(&["-d".into(), "5".into(), "-t".into()])
                .err()
                .unwrap()
                .to_string(),
            "unsupported flag: -t",
        );
        assert_eq!(
            parse_args(&["-d".into(), "-t".into()])
                .err()
                .unwrap()
                .to_string(),
            "expected delimiter argument following -d",
        );
        assert_eq!(
            parse_args(&["--delimiter=5".into(), "--null".into()])
                .err()
                .unwrap()
                .to_string(),
            "cannot specify both null and delimiter flag",
        );
    }

    #[test]
    fn should_delimit_blanks() {
        assert_eq!(
            delimit_blanks("testing this\tout\nhere\n  \n\t\t test").unwrap(),
            vec!["testing", "this", "out", "here", "test",]
        );
        assert_eq!(
            delimit_blanks("testing 'this\tout  here  ' \"now double\"").unwrap(),
            vec!["testing", "this\tout  here  ", "now double"]
        );
        assert_eq!(
            delimit_blanks("testing 'this\nout  here  '")
                .err()
                .unwrap()
                .to_string(),
            "unmatched quote; by default quotes are special to xargs unless you use the -0 option",
        );
    }

    #[test]
    fn test_xargs_collect_args() {
        fn collect(cli_args: &[&str], input: &str) -> Vec<String> {
            let stdin = ShellPipeReader::from_str(input);
            let cli_args = cli_args
                .iter()
                .map(|s| s.into())
                .collect::<Vec<_>>();
            let result = xargs_collect_args(&cli_args, stdin).unwrap();
            result
                .into_iter()
                .map(|s| s.to_str().unwrap().to_string())
                .collect()
        }

        // Test default behavior
        let result = collect(&[], "arg1 arg2\narg3");
        assert_eq!(result, ["echo", "arg1", "arg2", "arg3"]);

        let result = collect(&[], "arg1 arg2\narg3\n");
        assert_eq!(result, ["echo", "arg1", "arg2", "arg3"]);

        // printf "arg1 arg2\narg3\n\n" | xargs
        // > arg1 arg2 arg3
        let result = collect(&[], "arg1 arg2\n\narg3\n\n\n");
        assert_eq!(result, ["echo", "arg1", "arg2", "arg3"]);

        // Test null-delimited with trailing null
        let result = collect(&["-0"], "arg1\0arg2\0arg3\0");
        assert_eq!(result, ["echo", "arg1", "arg2", "arg3"]);

        // Test null-delimited with multiple nulls (all ignored)
        let result = collect(&["-0"], "arg1\0\0arg2\0arg3\0\0");
        assert_eq!(result, ["echo", "arg1", "", "arg2", "arg3", ""]);

        // Test custom delimiter with trailing delimiter
        let result = collect(&["-d", ":"], "arg1:arg2:arg3:");
        assert_eq!(result, ["echo", "arg1", "arg2", "arg3"]);

        let result = collect(&["-d", ":"], "arg1::arg2:arg3::");
        assert_eq!(result, ["echo", "arg1", "", "arg2", "arg3", ""]);
    }
}
