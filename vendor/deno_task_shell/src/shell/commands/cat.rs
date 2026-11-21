// Copyright 2018-2025 the Deno authors. MIT license.

use futures::future::LocalBoxFuture;
use std::{
    ffi::OsString,
    fs::File,
    io::Read,
};

use crate::shell::types::ExecuteResult;

use super::{
    ShellCommand,
    ShellCommandContext,
    args::{
        ArgKind,
        parse_arg_kinds,
    },
    error::ShellCommandError,
};

pub struct CatCommand;

impl ShellCommand for CatCommand {
    fn execute(&self, context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        let mut stderr = context.stderr.clone();
        let result = match execute_cat(context) {
            Ok(result) => result,
            Err(err) => {
                let _ = stderr.write_line(&format!("cat: {err}"));
                ExecuteResult::from_exit_code(1)
            }
        };
        Box::pin(futures::future::ready(result))
    }
}

fn execute_cat(mut context: ShellCommandContext) -> Result<ExecuteResult, ShellCommandError> {
    let flags = parse_args(context.args)?;
    let mut exit_code = 0;
    let mut buf = vec![0; 4096];
    for path in flags.paths {
        if path == "-" {
            context.stdin.clone().pipe_to_sender(context.stdout.clone())?;
        } else {
            // buffered to prevent reading an entire file
            // in memory
            match File::open(context.state.cwd().join(&path)) {
                Ok(mut file) => loop {
                    if let Some(exit_code) = context.state.kill_signal().aborted_code() {
                        return Ok(ExecuteResult::from_exit_code(exit_code));
                    }

                    let size = file.read(&mut buf)?;
                    if size == 0 {
                        break;
                    } else {
                        context.stdout.write_all(&buf[..size])?;
                    }
                },
                Err(err) => {
                    context.stderr.write_line(&format!(
                        "cat: {}: {}",
                        path.to_string_lossy(),
                        err
                    ))?;
                    exit_code = 1;
                }
            }
        }
    }

    Ok(ExecuteResult::from_exit_code(exit_code))
}

#[derive(Debug, PartialEq)]
struct CatFlags {
    paths: Vec<OsString>,
}

fn parse_args(args: Vec<OsString>) -> Result<CatFlags, ShellCommandError> {
    let mut paths = Vec::new();
    for arg in parse_arg_kinds(&args) {
        match arg {
            ArgKind::Arg(file_name) => {
                paths.push(file_name.to_os_string());
            }
            // for now, we don't support any arguments
            _ => arg.bail_unsupported()?,
        }
    }

    if paths.is_empty() {
        paths.push("-".into());
    }

    Ok(CatFlags { paths })
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_args() {
        assert_eq!(parse_args(vec![]).unwrap(), CatFlags { paths: vec!["-".into()] });
        assert_eq!(
            parse_args(vec!["path".into()]).unwrap(),
            CatFlags { paths: vec!["path".into()] }
        );
        assert_eq!(
            parse_args(vec!["path".into(), "-".into()]).unwrap(),
            CatFlags { paths: vec!["path".into(), "-".into()] }
        );
        assert_eq!(
            parse_args(vec!["path".into(), "other-path".into()]).unwrap(),
            CatFlags { paths: vec!["path".into(), "other-path".into()] }
        );
        assert_eq!(
            parse_args(vec!["--flag".into()])
                .err()
                .unwrap()
                .to_string(),
            "unsupported flag: --flag"
        );
        assert_eq!(
            parse_args(vec!["-t".into()])
                .err()
                .unwrap()
                .to_string(),
            "unsupported flag: -t"
        );
    }
}
