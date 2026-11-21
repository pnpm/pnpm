// Copyright 2018-2025 the Deno authors. MIT license.

use std::path::PathBuf;

use crate::{
    ExecuteResult,
    FutureExecuteResult,
    ShellCommand,
    ShellCommandContext,
};
use futures::FutureExt;

/// Command that resolves the command name and
/// executes it in a separate process.
pub struct ExecutableCommand {
    display_name: String,
    command_path: PathBuf,
}

impl ExecutableCommand {
    pub fn new(display_name: String, command_path: PathBuf) -> Self {
        Self { display_name, command_path }
    }
}

impl ShellCommand for ExecutableCommand {
    fn execute(&self, context: ShellCommandContext) -> FutureExecuteResult {
        let display_name = self.display_name.clone();
        let command_name = self.command_path.clone();
        async move {
            // don't spawn if already aborted
            if let Some(exit_code) = context.state.kill_signal().aborted_code() {
                return ExecuteResult::from_exit_code(exit_code);
            }

            let mut stderr = context.stderr;
            let mut sub_command = tokio::process::Command::new(&command_name);
            let child = sub_command
                .current_dir(context.state.cwd())
                .args(context.args)
                .env_clear()
                .envs(context.state.env_vars())
                .stdout(context.stdout.into_stdio())
                .stdin(context.stdin.into_stdio())
                .stderr(stderr.clone().into_stdio())
                .spawn();

            let mut child = match child {
                Ok(child) => child,
                Err(err) => {
                    let _ =
                        stderr.write_line(&format!("Error launching '{}': {}", display_name, err));
                    return ExecuteResult::from_exit_code(1);
                }
            };

            context.state.track_child_process(&child);

            // avoid deadlock since this is holding onto the pipes
            drop(sub_command);

            loop {
                tokio::select! {
                  result = child.wait() => match result {
                    Ok(status) => {
                      #[cfg(unix)]
                      let exit_code = {
                        use std::os::unix::process::ExitStatusExt;
                        status.code().unwrap_or_else(|| {
                          // Process was terminated by a signal, return 128 + signal_number per POSIX
                          status.signal().map(|sig| 128 + sig).unwrap_or(1)
                        })
                      };
                      #[cfg(not(unix))]
                      let exit_code = status.code().unwrap_or(1);

                      return ExecuteResult::Continue(
                        exit_code,
                        Vec::new(),
                        Vec::new(),
                      );
                    }
                    Err(err) => {
                      let _ = stderr.write_line(&format!("{}", err));
                      return ExecuteResult::from_exit_code(1);
                    }
                  },
                  signal = context.state.kill_signal().wait_any() => {
                    if let Some(_id) = child.id() {
                      #[cfg(unix)]
                      kill(_id as i32, signal);

                      if cfg!(not(unix)) && signal.causes_abort() {
                        let _ = child.start_kill();
                        let status = child.wait().await.ok();
                        return ExecuteResult::from_exit_code(
                          status.and_then(|s| s.code()).unwrap_or(signal.aborted_code()),
                        );
                      }
                    }
                  }
                }
            }
        }
        .boxed_local()
    }
}

#[cfg(unix)]
pub fn kill(pid: i32, signal: crate::SignalKind) -> Option<()> {
    use nix::{
        sys::signal::{
            Signal,
            kill as unix_kill,
        },
        unistd::Pid,
    };
    let signo: i32 = signal.into();
    let sig = Signal::try_from(signo).ok()?;
    unix_kill(Pid::from_raw(pid), Some(sig)).ok()?;
    Some(())
}
