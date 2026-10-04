#[cfg(target_family = "wasm")]
pub(crate) use pnpm_process as process;
#[cfg(target_family = "wasm")]
extern crate pnpm_which as which;

#[cfg(target_family = "wasm")]
pub(crate) use pnpm_process::asynchronous as async_process;
#[cfg(not(target_family = "wasm"))]
pub(crate) use std::process;
#[cfg(not(target_family = "wasm"))]
pub(crate) use tokio::process as async_process;

pub use bundled_node_gyp::{bundled_node_gyp_bin, bundled_node_gyp_entry};
pub use extend_path::{ScriptsPrependNodePath, extend_path};
pub use interrupt::{exit_like, interrupt_count};
pub use job_control::{JobGuard, arm_process_tree_cleanup};
pub use lifecycle::{
    DEV_PREINSTALL_ALREADY_RAN_ENV, DEV_PREINSTALL_STAGE, LifecycleScriptError,
    PROJECT_INSTALL_STAGES, PROJECT_LIFECYCLE_STAGES, PROJECT_POST_UNINSTALL_STAGES,
    PROJECT_PRE_UNINSTALL_STAGES, ROOT_PREINSTALL_ALREADY_RAN_ENV, RunPostinstallHooks,
    StreamedScript, push_script_arg, run_dev_preinstall_hook, run_lifecycle_hook,
    run_postinstall_hooks, run_project_lifecycle_scripts,
    run_project_lifecycle_scripts_after_preinstall, run_project_lifecycle_stages,
    run_root_preinstall_hook,
};
pub use make_env::{
    EnvBuild, EnvOptions, VERIFY_DEPS_BEFORE_RUN_ENV, build_env, package_manager_env,
};
pub use pnpm_executable::{current_executable, current_pnpm_exe, is_pnpx_alias};
pub use process_tracker::{ProcessTracker, SpawnedChild, spawn_child};
pub use run_script::{RunScript, RunScriptError, ScriptOutput, run_script};
pub use script_exit::ScriptExit;
pub use script_options::{ScriptEnvironment, ScriptExecutionOptions, ScriptInvocation};
pub use shell::{ScriptShellError, SelectedShell, select_shell};
pub use shell_emulator::{EmulatedOutput, ShellEmulatorError, execute_emulated};

mod bundled_node_gyp;
mod extend_path;
mod interrupt;
mod job_control;
mod lifecycle;
mod make_env;
mod pnpm_executable;
mod process_tracker;
mod run_script;
mod script_args;
mod script_exit;
mod script_options;
mod script_working_dir;
mod shell;
#[cfg_attr(target_family = "wasm", path = "shell_emulator_wasm.rs")]
mod shell_emulator;
mod shell_emulator_types;

use crate::process::{Command, ExitStatus};
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::path::Path;

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ExecutorError {
    #[display("Failed to spawn command: {_0}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_SPAWN_COMMAND))]
    SpawnCommand(#[error(source)] std::io::Error),

    #[display("Process exits with an error: {_0}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_WAIT_PROCESS))]
    WaitProcess(#[error(source)] std::io::Error),
}

pub fn execute_shell(command: &str) -> Result<(), ExecutorError> {
    spawn_shell(command, None).map(|_status| ())
}

/// Run `command` through `sh -c` in `current_dir` and return the child's
/// exit status.
///
/// A non-zero exit is *not* an [`ExecutorError`] —
/// only a failure to spawn the shell or to wait on it is.
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
