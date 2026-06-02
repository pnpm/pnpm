mod extend_path;
mod lifecycle;
mod make_env;
mod run_script;
mod shell;

pub use extend_path::{ScriptsPrependNodePath, extend_path};
pub use lifecycle::{
    LifecycleScriptError, PROJECT_LIFECYCLE_STAGES, RunPostinstallHooks, push_script_arg,
    run_lifecycle_hook, run_postinstall_hooks, run_project_lifecycle_scripts,
};
pub use make_env::{EnvBuild, EnvOptions, build_env};
pub use run_script::{RunScript, RunScriptError, run_script};
pub use shell::{ScriptShellError, SelectedShell, select_shell};

use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    path::Path,
    process::{Command, ExitStatus},
};

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ExecutorError {
    #[display("Failed to spawn command: {_0}")]
    #[diagnostic(code(pacquet_executor::spawn_command))]
    SpawnCommand(#[error(source)] std::io::Error),

    #[display("Process exits with an error: {_0}")]
    #[diagnostic(code(pacquet_executor::wait_process))]
    WaitProcess(#[error(source)] std::io::Error),
}

pub fn execute_shell(command: &str) -> Result<(), ExecutorError> {
    spawn_shell(command, None).map(|_status| ())
}

/// Run `command` through `sh -c` in `current_dir` and return the child's
/// exit status.
///
/// The variant [`execute_shell`] builds on: callers that need to react
/// to a non-zero exit (e.g. recursive run recording a per-package
/// `passed` / `failure` status) inspect the returned [`ExitStatus`],
/// while callers that only care about spawn / wait failures use
/// [`execute_shell`]. A non-zero exit is *not* an [`ExecutorError`] —
/// only a failure to spawn the shell or to wait on it is.
///
/// `current_dir` is the directory the script runs in. A recursive run
/// passes each package's root so scripts resolve relative paths against
/// their own project, matching pnpm's `runLifecycleHook` (which runs
/// with `pkgRoot` as the working directory).
pub fn execute_shell_with_status(
    command: &str,
    current_dir: &Path,
) -> Result<ExitStatus, ExecutorError> {
    spawn_shell(command, Some(current_dir))
}

fn spawn_shell(command: &str, current_dir: Option<&Path>) -> Result<ExitStatus, ExecutorError> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    if let Some(current_dir) = current_dir {
        cmd.current_dir(current_dir);
    }
    let mut child = cmd.spawn().map_err(ExecutorError::SpawnCommand)?;
    child.wait().map_err(ExecutorError::WaitProcess)
}

#[cfg(test)]
mod tests;
