mod recursive;

use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_executor::{push_script_arg, select_shell};
use pacquet_package_manager::{make_node_package_map_option, package_map_path_for_execution};
use pacquet_package_manifest::PackageManifest;
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

/// Run a shell command in the context of a project.
///
/// Ports pnpm's `exec` command from
/// <https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/exec.ts>.
/// The recursive variant (selected by the global `-r` / `--recursive`
/// flag) runs the command in every workspace project, topologically
/// sorted and sequential, with `--resume-from` / `--report-summary` /
/// `--no-bail` (see [`recursive`]). The `--filter` package-selector
/// narrowing and `--workspace-concurrency` parallelism are not ported yet
/// — the selected set is every workspace project, matching the recursive
/// `run` runner and pacquet's currently-unfiltered `install`.
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
    #[clap(long = "resume-from")]
    pub resume_from: Option<String>,

    /// Recursive only: write a `pnpm-exec-summary.json` execution report
    /// to the workspace root.
    #[clap(long = "report-summary")]
    pub report_summary: bool,

    /// Recursive only: keep going after a project fails instead of
    /// stopping at the first failure.
    #[clap(long = "no-bail")]
    pub no_bail: bool,
}

/// Errors from `pacquet exec`.
///
/// Mirrors the error codes pnpm raises in `exec.ts`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/exec.ts>)
/// and the `BAD_PATH_DIR` guard from its `makeEnv`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/makeEnv.ts#L19-L25>).
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
    #[diagnostic(code(pacquet_cli::exec_spawn))]
    Spawn {
        command: String,
        #[error(source)]
        source: std::io::Error,
    },
}

impl ExecArgs {
    /// Execute the subcommand in `dir` (the project / working directory).
    ///
    /// On a non-zero child exit code this terminates the process with the
    /// same code via [`std::process::exit`], matching pnpm's exec, which
    /// returns `{ exitCode }` and lets the CLI exit with it.
    pub fn run(self, dir: &Path, config: &Config) -> miette::Result<()> {
        let command = prepare_command(self.command)?;
        let status = spawn_in_dir(&command, dir, config, self.shell_mode)?;
        if !status.success() {
            // Propagate the child's exit code. A signal-terminated child
            // has no code; fall back to 1, matching pnpm's `exitCode ?? 1`.
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(())
    }

    /// Execute the command for every project in the workspace, in
    /// topological order. The recursive counterpart of [`Self::run`],
    /// selected when the global `-r` / `--recursive` flag is set.
    pub fn run_recursive(&self, config: &Config, dir: &Path) -> miette::Result<()> {
        recursive::exec_recursive(self, config, dir)
    }
}

/// Strip a surviving leading `--` and reject an empty command.
///
/// Mirrors `if (params[0] === '--') params.shift()` at exec.ts:171-173;
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
pub(super) fn spawn_in_dir(
    command: &[String],
    dir: &Path,
    config: &Config,
    shell_mode: bool,
) -> Result<ExitStatus, ExecError> {
    // pnpm prepends `./node_modules/.bin` (resolved against the project
    // directory) and then the `extraBinPaths`. See exec.ts:225-228.
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
    // Drop any inherited PATH-like key before re-inserting our own, so
    // a Windows `Path`/`PATH` pair can't collapse to an unspecified
    // winner at spawn time (matching the lifecycle spawn in
    // `pacquet-executor`).
    cmd.env_remove("PATH");
    cmd.env_remove("Path");
    cmd.env("PATH", &path);
    // pnpm's `makeEnv` defaults `npm_config_user_agent` to `'pnpm'`
    // when no `userAgent` is configured (makeEnv.ts:30). pacquet has
    // no `userAgent` setting yet, so it always takes that default.
    cmd.env("npm_config_user_agent", "pnpm");
    if let Some(name) = read_package_name(dir) {
        cmd.env("PNPM_PACKAGE_NAME", name);
    }
    let mut node_options = config.node_options.clone();
    if let Some(package_map_path) = package_map_path_for_execution(config, dir) {
        node_options =
            Some(make_node_package_map_option(&package_map_path, node_options.as_deref()));
    }
    // pnpm forwards `nodeOptions` as `NODE_OPTIONS` to the child.
    // See exec.ts:246.
    if let Some(node_options) = node_options {
        cmd.env("NODE_OPTIONS", node_options);
    }

    cmd.status().map_err(|source| ExecError::Spawn { command: command[0].clone(), source })
}

/// Read the `name` field of the project's `package.json`, if any.
///
/// Used only to stamp `PNPM_PACKAGE_NAME`; a missing or nameless manifest
/// is not an error for `exec` (it can run a command in any directory).
fn read_package_name(dir: &Path) -> Option<String> {
    PackageManifest::from_path(dir.join("package.json"))
        .ok()?
        .value()
        .get("name")?
        .as_str()
        .map(str::to_string)
}

/// Prepend `dirs` to the current process `PATH`.
///
/// Ports `prependDirsToPath`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/shell/path/src/index.ts>)
/// together with `makeEnv`'s up-front delimiter guard
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/makeEnv.ts#L19-L25>):
/// a directory containing the platform path delimiter cannot be expressed
/// in `PATH`, so it is rejected with [`ExecError::BadPathDir`] rather than
/// silently splitting into two entries.
fn prepend_dirs_to_path(dirs: &[PathBuf]) -> Result<OsString, ExecError> {
    let delimiter = if cfg!(windows) { ';' } else { ':' };
    for dir in dirs {
        if dir.to_string_lossy().contains(delimiter) {
            return Err(ExecError::BadPathDir {
                dir: dir.to_string_lossy().into_owned(),
                delimiter,
            });
        }
    }

    let sep: &OsStr = if cfg!(windows) { OsStr::new(";") } else { OsStr::new(":") };
    let mut out = OsString::new();
    for (i, dir) in dirs.iter().enumerate() {
        if i > 0 {
            out.push(sep);
        }
        out.push(dir);
    }
    if let Some(current) = std::env::var_os("PATH")
        && !current.is_empty()
    {
        if !out.is_empty() {
            out.push(sep);
        }
        out.push(current);
    }
    Ok(out)
}
