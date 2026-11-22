// Copyright 2018-2025 the Deno authors. MIT license.

use std::ffi::OsString;

use futures::future::LocalBoxFuture;

use crate::{
    EnvChange,
    shell::types::ExecuteResult,
};

use super::{
    ShellCommand,
    ShellCommandContext,
    error::{
        ShellCommandError,
        bail,
    },
};

pub struct UnsetCommand;

impl ShellCommand for UnsetCommand {
    fn execute(&self, mut context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        let result = match parse_names(context.args) {
            Ok(names) => ExecuteResult::Continue(
                0,
                names
                    .into_iter()
                    .map(EnvChange::UnsetVar)
                    .collect(),
                Vec::new(),
            ),
            Err(err) => {
                let _ = context.stderr.write_line(&format!("unset: {err}"));
                ExecuteResult::Continue(1, Vec::new(), Vec::new())
            }
        };
        Box::pin(futures::future::ready(result))
    }
}

fn parse_names(mut args: Vec<OsString>) -> Result<Vec<OsString>, ShellCommandError> {
    match args.first() {
        None => {
            // Running the actual `unset` with no argument completes with success.
            Ok(args)
        }
        Some(flag) if flag == "-f" => bail!("unsupported flag: -f"),
        Some(flag) if flag == "-v" => {
            // It's fine to use `swap_remove` (instead of `remove`) because the order
            // of args doesn't matter for `unset` command.
            args.swap_remove(0);
            Ok(args)
        }
        Some(_) => Ok(args),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_args() {
        assert_eq!(parse_names(vec!["VAR1".into()]).unwrap(), vec![OsString::from("VAR1")]);
        assert_eq!(
            parse_names(vec!["VAR1".into(), "VAR2".into()]).unwrap(),
            vec![OsString::from("VAR1"), "VAR2".into()]
        );
        assert!(parse_names(vec![]).unwrap().is_empty());
        assert_eq!(
            parse_names(vec!["-f".into(), "VAR1".into(), "VAR2".into()])
                .err()
                .unwrap()
                .to_string(),
            "unsupported flag: -f".to_string()
        );
        assert_eq!(
            parse_names(vec!["-v".into(), "VAR1".into(), "VAR2".into()]).unwrap(),
            vec![OsString::from("VAR2"), "VAR1".into()]
        );
    }
}
