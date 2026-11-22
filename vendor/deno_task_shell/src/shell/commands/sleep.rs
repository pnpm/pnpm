// Copyright 2018-2025 the Deno authors. MIT license.

use std::{
    ffi::OsString,
    time::Duration,
};

use futures::{
    FutureExt,
    future::LocalBoxFuture,
};

use crate::shell::types::{
    ExecuteResult,
    ShellPipeWriter,
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
    execute_with_cancellation,
};

pub struct SleepCommand;

impl ShellCommand for SleepCommand {
    fn execute(&self, context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        async move {
            execute_with_cancellation!(
                sleep_command(&context.args, context.stderr),
                context.state.kill_signal()
            )
        }
        .boxed_local()
    }
}

async fn sleep_command(args: &[OsString], mut stderr: ShellPipeWriter) -> ExecuteResult {
    match execute_sleep(args).await {
        Ok(()) => ExecuteResult::from_exit_code(0),
        Err(err) => {
            let _ = stderr.write_line(&format!("sleep: {err}"));
            ExecuteResult::from_exit_code(1)
        }
    }
}

async fn execute_sleep(args: &[OsString]) -> Result<(), ShellCommandError> {
    let ms = parse_args(args)?;
    tokio::time::sleep(Duration::from_millis(ms)).await;
    Ok(())
}

fn parse_arg(arg: &str) -> Result<f64, ShellCommandError> {
    if let Some(t) = arg.strip_suffix('s') {
        return Ok(t.parse()?);
    }
    if let Some(t) = arg.strip_suffix('m') {
        return Ok(t.parse::<f64>()? * 60.);
    }
    if let Some(t) = arg.strip_suffix('h') {
        return Ok(t.parse::<f64>()? * 60. * 60.);
    }
    if let Some(t) = arg.strip_suffix('d') {
        return Ok(t.parse::<f64>()? * 60. * 60. * 24.);
    }

    Ok(arg.parse()?)
}

fn parse_args(args: &[OsString]) -> Result<u64, ShellCommandError> {
    // the time to sleep is the sum of all the arguments
    let mut total_time_ms = 0;
    let mut had_value = false;
    for arg in parse_arg_kinds(args) {
        match arg {
            ArgKind::Arg(arg) => {
                match arg
                    .to_str()
                    .ok_or_else(|| ShellCommandError::new("invalid utf-8"))
                    .and_then(parse_arg)
                {
                    Ok(value_s) => {
                        let ms = (value_s * 1000f64) as u64;
                        total_time_ms += ms;
                        had_value = true;
                    }
                    Err(err) => {
                        bail!(
                            "error parsing argument '{}' to number: {}",
                            arg.to_string_lossy(),
                            err
                        );
                    }
                }
            }
            ArgKind::LongFlag(_) | ArgKind::ShortFlag(_) => arg.bail_unsupported()?,
        }
    }
    if !had_value {
        bail!("missing operand");
    }
    Ok(total_time_ms)
}

#[cfg(test)]
mod test {
    use std::time::Instant;

    use super::*;

    #[test]
    fn should_parse_arg() {
        assert_eq!(parse_arg("1").unwrap(), 1.);
        assert_eq!(parse_arg("1s").unwrap(), 1.);
        assert_eq!(parse_arg("1m").unwrap(), 1. * 60.);
        assert_eq!(parse_arg("1h").unwrap(), 1. * 60. * 60.);
        assert_eq!(parse_arg("1d").unwrap(), 1. * 60. * 60. * 24.);
        assert!(parse_arg("d").err().is_some());
    }

    #[test]
    fn should_parse_args() {
        let value = parse_args(&["0.5".into(), "1m".into(), "1.25".into()]).unwrap();
        assert_eq!(value, 500 + 1000 * 60 + 1250);

        let result = parse_args(&[]).err().unwrap();
        assert_eq!(result.to_string(), "missing operand");

        let result = parse_args(&["test".into()]).err().unwrap();
        assert_eq!(
            result.to_string(),
            "error parsing argument 'test' to number: invalid float literal"
        );
    }

    #[tokio::test]
    async fn should_execute() {
        let time = Instant::now();
        execute_sleep(&["0.1".into()]).await.unwrap();
        assert!(time.elapsed().as_millis() >= 100);
    }
}
