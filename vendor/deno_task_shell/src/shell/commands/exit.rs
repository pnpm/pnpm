// Copyright 2018-2025 the Deno authors. MIT license.

use std::ffi::OsString;

use futures::future::LocalBoxFuture;

use crate::shell::types::ExecuteResult;

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

pub struct ExitCommand;

impl ShellCommand for ExitCommand {
    fn execute(&self, mut context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        let result = match execute_exit(&context.args) {
            Ok(code) => ExecuteResult::Exit(code, Vec::new()),
            Err(err) => {
                context.stderr
                    .write_line(&format!("exit: {err}"))
                    .unwrap();
                ExecuteResult::Exit(2, Vec::new())
            }
        };
        Box::pin(futures::future::ready(result))
    }
}

fn execute_exit(args: &[OsString]) -> Result<i32, ShellCommandError> {
    let exit_code = parse_args(args)?;

    Ok(if exit_code < 0 {
        let code = -exit_code % 256;
        256 - code
    } else {
        exit_code % 256
    })
}

fn parse_args(args: &[OsString]) -> Result<i32, ShellCommandError> {
    let args = parse_arg_kinds(args);
    let mut paths = Vec::new();
    for arg in args {
        match arg {
            ArgKind::Arg(arg) => {
                paths.push(arg);
            }
            _ => arg.bail_unsupported()?,
        }
    }

    match paths.len() {
        0 => Ok(1),
        1 => {
            let arg = paths.remove(0).to_string_lossy();
            match arg.parse::<i32>() {
                Ok(value) => Ok(value),
                Err(_) => bail!("numeric argument required."),
            }
        }
        _ => {
            bail!("too many arguments")
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parses_args() {
        assert_eq!(parse_args(&[]).unwrap(), 1);
        assert_eq!(parse_args(&["5".into()]).unwrap(), 5);
        assert_eq!(
            parse_args(&["test".into()])
                .err()
                .unwrap()
                .to_string(),
            "numeric argument required."
        );
        assert_eq!(
            parse_args(&["1".into(), "2".into()])
                .err()
                .unwrap()
                .to_string(),
            "too many arguments"
        );
        assert_eq!(
            parse_args(&["-a".into()])
                .err()
                .unwrap()
                .to_string(),
            "unsupported flag: -a"
        );
        assert_eq!(
            parse_args(&["--a".into()])
                .err()
                .unwrap()
                .to_string(),
            "unsupported flag: --a"
        );
    }

    #[test]
    fn executes_exit() {
        assert_eq!(execute_exit(&[]).unwrap(), 1);
        assert_eq!(execute_exit(&["0".into()]).unwrap(), 0);
        assert_eq!(execute_exit(&["255".into()]).unwrap(), 255);
        assert_eq!(execute_exit(&["256".into()]).unwrap(), 0);
        assert_eq!(execute_exit(&["257".into()]).unwrap(), 1);
        assert_eq!(execute_exit(&["-1".into()]).unwrap(), 255);
    }
}
