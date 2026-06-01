use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_executor::select_shell;
use pacquet_package_manifest::PackageManifest;
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
};

/// Run a shell command in the context of a project.
///
/// Ports the single-project (non-recursive) path of pnpm's `exec`
/// command from
/// <https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/exec.ts>.
/// The recursive / workspace-filtered variant (topological scheduler,
/// `--workspace-concurrency`, `--resume-from`, `--report-summary`) is
/// not ported yet. pacquet now has the selection layer
/// (`workspace-projects-filter`, `workspace-projects-graph`, and the
/// global `--filter`/`--recursive` flags landed in #11959 and #12000),
/// but no recursive runner to consume it — the global flags are
/// accepted on `exec` via clap but not yet acted on.
#[derive(Debug, Args)]
pub struct ExecArgs {
    /// The command to run, followed by its arguments.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,

    /// Run the command inside of a shell. Uses `/bin/sh` on UNIX and
    /// `cmd.exe` on Windows.
    #[clap(long, short = 'c')]
    pub shell_mode: bool,
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
        let ExecArgs { mut command, shell_mode } = self;

        // For backward compatibility, mirroring `if (params[0] === '--')
        // params.shift()` at exec.ts:171-173. Clap normally consumes a
        // bare `--` itself, so this only fires when one survives as a
        // literal token.
        if command.first().map(String::as_str) == Some("--") {
            command.remove(0);
        }

        if command.is_empty() {
            return Err(ExecError::MissingCommand.into());
        }

        // pnpm prepends `./node_modules/.bin` (resolved against the
        // project directory) and then the `extraBinPaths`. See
        // exec.ts:225-228.
        let mut prepend = Vec::with_capacity(1 + config.extra_bin_paths.len());
        prepend.push(dir.join("node_modules").join(".bin"));
        prepend.extend(config.extra_bin_paths.iter().cloned());
        let path = prepend_dirs_to_path(&prepend)?;

        let mut cmd = if shell_mode {
            // execa's `shell: true` joins the command and its arguments
            // into a single string and hands it to the shell verbatim
            // (no escaping). Mirror that with the platform shell.
            let shell =
                select_shell(None, cfg!(windows)).expect("default shell selection never fails");
            let mut cmd = Command::new(&shell.program);
            cmd.args(&shell.args).arg(command.join(" "));
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
        // pnpm forwards `nodeOptions` as `NODE_OPTIONS` to the child.
        // See exec.ts:246.
        if let Some(node_options) = &config.node_options {
            cmd.env("NODE_OPTIONS", node_options);
        }

        let status = cmd
            .status()
            .map_err(|source| ExecError::Spawn { command: command[0].clone(), source })?;

        if !status.success() {
            // Propagate the child's exit code. A signal-terminated child
            // has no code; fall back to 1, matching pnpm's `exitCode ?? 1`.
            std::process::exit(status.code().unwrap_or(1));
        }

        Ok(())
    }
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
