mod recursive;

use crate::path_env::{BadPathDir, prepend_dirs_to_path, set_command_path};
use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_executor::{
    ProcessTracker, ScriptOutput, StreamedScript, push_script_arg, select_shell, spawn_child,
};
use pnpm_package_manager::{
    make_node_package_map_option, make_node_require_option, package_map_path_for_execution,
    pnp_path_for_execution,
};
use pnpm_workspace::safe_read_project_manifest_only;
use std::{
    path::Path,
    process::{Command, ExitStatus, Stdio},
};

use super::reporter::{ReporterType, reporter_emit};

/// Run a shell command in the context of a project.
///
/// With `-r` / `--recursive`, runs the command in every workspace project
/// (or the `--filter`-selected subset), in topological order.
#[derive(Debug, Args)]
pub struct ExecArgs {
    /// The command to run, followed by its arguments.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,

    /// Run the command inside of a shell. Uses `/bin/sh` on UNIX and
    /// `cmd.exe` on Windows.
    #[clap(long, short = 'c')]
    pub shell_mode: bool,

    /// Recursive only: resume execution from the given package, skipping
    /// every earlier project in the topological order.
    #[clap(skip)]
    pub resume_from: Option<String>,

    /// Recursive only: write a `pnpm-exec-summary.json` execution report
    /// to the workspace root.
    #[clap(skip)]
    pub report_summary: bool,

    /// Recursive only: keep going after a project fails instead of
    /// stopping at the first failure.
    #[clap(skip)]
    pub no_bail: bool,

    /// Sort recursive workspace projects topologically before running.
    #[clap(skip = true)]
    pub sort: bool,

    /// Reverse the project order of a recursive exec.
    #[clap(skip = true)]
    pub reverse: bool,

    /// Run every selected project concurrently, without a concurrency cap.
    #[clap(skip = true)]
    pub parallel: bool,
}

/// Errors from `pacquet exec`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ExecError {
    #[display("'pnpm exec' requires a command to run")]
    #[diagnostic(code(ERR_PNPM_EXEC_MISSING_COMMAND))]
    MissingCommand,

    #[display(
        "Cannot add {dir} to PATH because it contains the path delimiter character ({delimiter})"
    )]
    #[diagnostic(code(ERR_PNPM_BAD_PATH_DIR))]
    BadPathDir { dir: String, delimiter: char },

    #[display("Command \"{command}\" not found")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL))]
    CommandNotFound { command: String },

    #[display("Failed to spawn command \"{command}\": {source}")]
    #[diagnostic(code(ERR_PNPM_CLI_EXEC_SPAWN))]
    Spawn {
        command: String,
        #[error(source)]
        source: std::io::Error,
    },
}

impl From<BadPathDir> for ExecError {
    fn from(BadPathDir { dir, delimiter }: BadPathDir) -> Self {
        ExecError::BadPathDir { dir, delimiter }
    }
}

impl ExecArgs {
    /// Execute the subcommand in `dir` (the project / working directory).
    ///
    /// On a non-zero child exit code this terminates the process with the
    /// same code via [`std::process::exit`], matching pnpm's exec, which
    /// returns `{ exitCode }` and lets the CLI exit with it.
    pub fn run(self, dir: &Path, config: &Config, reporter: ReporterType) -> miette::Result<()> {
        let command = prepare_command(self.command)?;
        super::verify_deps::verify_deps_before_run(dir, config, reporter)?;
        let status =
            spawn_in_dir(&command, dir, config, self.shell_mode, ScriptOutput::Inherit, None)?;
        if !status.success() {
            // Propagate the child's exit code. A signal-terminated child
            // has no code; fall back to 1, matching pnpm's `exitCode ?? 1`.
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(())
    }

    /// Execute the command across the `--filter`-selected workspace
    /// projects, in topological order. The recursive counterpart of
    /// [`Self::run`], selected when the global `-r` / `--recursive` flag is set.
    pub async fn run_recursive(
        &self,
        config: &Config,
        dir: &Path,
        reporter: ReporterType,
    ) -> miette::Result<()> {
        super::verify_deps::verify_deps_before_run(dir, config, reporter)?;
        recursive::exec_recursive(self, config, dir, reporter_emit(reporter)).await
    }
}

/// Strip a surviving leading `--` and reject an empty command.
///
/// clap normally consumes a bare `--` itself, so this only fires when one
/// survives as a literal token.
fn prepare_command(mut command: Vec<String>) -> Result<Vec<String>, ExecError> {
    if command.first().map(String::as_str) == Some("--") {
        command.remove(0);
    }
    if command.is_empty() {
        return Err(ExecError::MissingCommand);
    }
    Ok(command)
}

/// Resolve and spawn `command` in `dir` with `node_modules/.bin` +
/// `extraBinPaths` on `PATH` and the exec environment stamped
/// (`npm_config_user_agent`, `PNPM_PACKAGE_NAME`, `NODE_OPTIONS`).
///
/// Returns the child's [`ExitStatus`] without terminating the process, so
/// the single-project path can `process::exit` while the recursive path
/// records the per-project status. `command` is assumed non-empty (see
/// [`prepare_command`]).
///
/// `output` decides where the child writes: a recursive `exec` under
/// `--no-reporter-hide-prefix` streams, so the reporter can label each
/// line with the project it came from; every other invocation inherits
/// the terminal.
pub(super) fn spawn_in_dir(
    command: &[String],
    dir: &Path,
    config: &Config,
    shell_mode: bool,
    output: ScriptOutput<'_>,
    process_tracker: Option<&ProcessTracker>,
) -> Result<ExitStatus, ExecError> {
    let mut cmd = command_in_dir(command, dir, config, shell_mode)?;
    let ScriptOutput::Streamed { dep_path, emit } = output else {
        let mut child = spawn_child(&mut cmd, process_tracker)
            .map_err(|source| ExecError::Spawn { command: command[0].clone(), source })?;
        return child
            .wait()
            .map_err(|source| ExecError::Spawn { command: command[0].clone(), source });
    };
    let wd = dir.to_string_lossy();
    let streamed = StreamedScript { dep_path, stage: EXEC_STAGE, wd: &wd, emit };
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = spawn_child(&mut cmd, process_tracker)
        .map_err(|source| ExecError::Spawn { command: command[0].clone(), source })?;
    let status = streamed
        .pump(child.child_mut())
        .map_err(|source| ExecError::Spawn { command: command[0].clone(), source })?;
    streamed.finished(status.code().unwrap_or(-1));
    Ok(status)
}

fn command_in_dir(
    command: &[String],
    dir: &Path,
    config: &Config,
    shell_mode: bool,
) -> Result<Command, ExecError> {
    // Prepend `./node_modules/.bin` (resolved against the project
    // directory) and then the `extraBinPaths`.
    let mut prepend = Vec::with_capacity(1 + config.extra_bin_paths.len());
    prepend.push(dir.join("node_modules").join(".bin"));
    prepend.extend(config.extra_bin_paths.iter().cloned());
    let path = prepend_dirs_to_path(&prepend)?;

    let mut cmd = if shell_mode {
        // execa's `shell: true` joins the command and its arguments
        // into a single string and hands it to the shell verbatim (no
        // per-token escaping). Mirror that with the platform shell,
        // appending the joined string through `push_script_arg` so the
        // Windows `cmd /d /s /c` verbatim path uses `raw_arg` — matching
        // execa's `windowsVerbatimArguments` and keeping embedded quoting
        // (e.g. `node -e "..."`) intact.
        let shell = select_shell(None, cfg!(windows)).expect("default shell selection never fails");
        let mut cmd = Command::new(&shell.program);
        cmd.args(&shell.args);
        push_script_arg(&mut cmd, &command.join(" "), shell.windows_verbatim_args);
        cmd
    } else {
        // execa resolves the program against the (extended) PATH up
        // front (via cross-spawn / which). Do the same explicitly:
        // Rust's `Command` does not reliably search the child's PATH
        // for the program on every platform.
        let program = which::which_in(&command[0], Some(&path), dir)
            .map_err(|_| ExecError::CommandNotFound { command: command[0].clone() })?;
        let mut cmd = Command::new(program);
        cmd.args(&command[1..]);
        cmd
    };

    cmd.current_dir(dir);
    // `updateConfig`-provided env, applied first so pnpm's own keys
    // below (PATH, user-agent, NODE_OPTIONS) win on conflict — matching
    // TS `makeEnv`, which spreads `...extraEnv` into the base. Empty
    // unless an install-family command populated it.
    cmd.envs(&config.extra_env);
    set_command_path(&mut cmd, &path);
    cmd.env("npm_config_user_agent", &config.user_agent);
    // Same recursion-guard stamp as the lifecycle env builder.
    cmd.env(pnpm_executor::VERIFY_DEPS_BEFORE_RUN_ENV, "false");
    if let Some(name) = read_package_name(dir) {
        cmd.env("PNPM_PACKAGE_NAME", name);
    }
    let mut node_options = configured_node_options(config);
    if let Some(pnp_path) = pnp_path_for_execution(config, dir) {
        node_options = Some(make_node_require_option(&pnp_path, node_options.as_deref()));
    }
    if let Some(package_map_path) = package_map_path_for_execution(config, dir) {
        node_options =
            Some(make_node_package_map_option(&package_map_path, node_options.as_deref()));
    }
    // pnpm forwards `nodeOptions` as `NODE_OPTIONS` to the child.
    // See exec.ts:246.
    if let Some(node_options) = node_options {
        cmd.env("NODE_OPTIONS", node_options);
    }

    Ok(cmd)
}

/// The `stage` pnpm stamps on the lifecycle events of an exec'd command.
pub(super) const EXEC_STAGE: &str = "(exec)";

fn configured_node_options(config: &Config) -> Option<String> {
    match config.node_options.as_deref() {
        Some(node_options) => {
            Some(pnpm_config::esm_node_path_loader::keep_esm_node_path_loader_option(
                node_options,
                config.extra_env.get("NODE_OPTIONS").map(String::as_str),
            ))
        }
        None => config.extra_env.get("NODE_OPTIONS").cloned(),
    }
}

/// Read the `name` field of the project's package manifest, if any.
///
/// Used only to stamp `PNPM_PACKAGE_NAME`; a missing or nameless manifest
/// is not an error for `exec` (it can run a command in any directory).
pub(super) fn read_package_name(dir: &Path) -> Option<String> {
    safe_read_project_manifest_only(dir).ok()??.value().get("name")?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests;
