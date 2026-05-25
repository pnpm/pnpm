use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_executor::select_shell;
use pacquet_package_manifest::PackageManifest;
use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Args)]
pub struct ExecArgs {
    /// If set, runs the command inside a shell (`/bin/sh -c` on UNIX,
    /// `cmd /d /s /c` on Windows).
    #[clap(short = 'c', long = "shell-mode")]
    pub shell_mode: bool,

    /// The command to run, followed by its arguments. Hyphen-prefixed
    /// tokens flow through to the command rather than being parsed as
    /// pacquet flags.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    pub command: Vec<String>,
}

/// Error type of [`ExecArgs::run`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ExecError {
    #[display("'pacquet exec' requires a command to run")]
    #[diagnostic(code(ERR_PNPM_EXEC_MISSING_COMMAND))]
    MissingCommand,

    #[display(r#"Command "{command}" not found"#)]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL))]
    CommandNotFound { command: String },

    #[display("Failed to spawn `{command}`: {source}")]
    #[diagnostic(code(pacquet_cli::exec_spawn))]
    Spawn {
        command: String,
        #[error(source)]
        source: std::io::Error,
    },
}

impl ExecArgs {
    /// Run a command in the context of the project at `dir`, with
    /// `node_modules/.bin` prepended to `PATH`. Ports the
    /// single-project path of upstream's `exec` handler at
    /// <https://github.com/pnpm/pnpm/blob/80037699fb/exec/commands/src/exec.ts#L166-L349>.
    pub fn run(self, dir: &Path, config: &Config) -> miette::Result<()> {
        let ExecArgs { shell_mode, mut command } = self;

        // Backward-compat: a leading `--` separator is dropped.
        if command.first().map(String::as_str) == Some("--") {
            command.remove(0);
        }
        if command.is_empty() {
            return Err(ExecError::MissingCommand.into());
        }

        let manifest = PackageManifest::from_path(dir.join("package.json")).ok();
        let package_name = manifest
            .as_ref()
            .and_then(|manifest| manifest.value().get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let env = make_env(MakeEnv {
            dir,
            extra_bin_paths: &config.extra_bin_paths,
            node_options: config.node_options.as_deref(),
            package_name: package_name.as_deref(),
            user_agent: "pnpm",
        });

        let (program, args) = command.split_first().expect("command is non-empty");
        let status = if shell_mode {
            // `shell: true` joins the command line and runs it through
            // the platform shell.
            let line = command.join(" ");
            let shell = select_shell(None, cfg!(windows)).map_err(miette::Report::new)?;
            Command::new(&shell.program)
                .args(&shell.args)
                .arg(&line)
                .current_dir(dir)
                .env_clear()
                .envs(&env)
                .status()
                .map_err(|source| ExecError::Spawn { command: line.clone(), source })?
        } else {
            match Command::new(program).args(args).current_dir(dir).env_clear().envs(&env).status()
            {
                Ok(status) => status,
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                    return Err(ExecError::CommandNotFound { command: program.clone() }.into());
                }
                Err(source) => {
                    return Err(ExecError::Spawn { command: program.clone(), source }.into());
                }
            }
        };

        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(())
    }
}

/// Inputs to [`make_env`].
pub struct MakeEnv<'a> {
    pub dir: &'a Path,
    pub extra_bin_paths: &'a [PathBuf],
    pub node_options: Option<&'a str>,
    pub package_name: Option<&'a str>,
    pub user_agent: &'a str,
}

/// Build the environment for an `exec` / `dlx` child process: the
/// inherited env with `<dir>/node_modules/.bin` (and any extra bin
/// paths) prepended to `PATH`, plus `npm_config_user_agent` and
/// `PNPM_PACKAGE_NAME`. Ports upstream's `makeEnv` from
/// <https://github.com/pnpm/pnpm/blob/80037699fb/exec/commands/src/makeEnv.ts>.
pub fn make_env(opts: MakeEnv<'_>) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = env::vars().collect();

    let mut prepend: Vec<PathBuf> = Vec::with_capacity(1 + opts.extra_bin_paths.len());
    prepend.push(opts.dir.join("node_modules").join(".bin"));
    prepend.extend_from_slice(opts.extra_bin_paths);

    let (path_key, path_value) = prepend_dirs_to_path(&prepend, &env);
    // Drop any existing PATH-cased key so the explicit insert wins on
    // Windows (where `Path` and `PATH` collapse at spawn time).
    env.retain(|key, _| !key.eq_ignore_ascii_case("PATH"));
    env.insert(path_key, path_value);

    env.insert("npm_config_user_agent".to_string(), opts.user_agent.to_string());
    if let Some(name) = opts.package_name {
        env.insert("PNPM_PACKAGE_NAME".to_string(), name.to_string());
    }
    if let Some(node_options) = opts.node_options {
        env.insert("NODE_OPTIONS".to_string(), node_options.to_string());
    }

    env
}

/// Prepend `dirs` to the inherited `PATH`, returning the
/// platform-correct key name and the joined value. Mirrors
/// `@pnpm/shell.path`'s `prependDirsToPath`.
fn prepend_dirs_to_path(dirs: &[PathBuf], env: &HashMap<String, String>) -> (String, String) {
    let existing = env.iter().find(|(k, _)| k.eq_ignore_ascii_case("PATH"));
    let key = existing.map_or_else(
        || if cfg!(windows) { "Path".to_string() } else { "PATH".to_string() },
        |(k, _)| k.clone(),
    );

    let mut entries: Vec<OsString> =
        dirs.iter().map(|dir| dir.as_os_str().to_os_string()).collect();
    if let Some((_, value)) = existing
        && !value.is_empty()
    {
        for part in env::split_paths(value) {
            entries.push(part.into_os_string());
        }
    }
    let joined = env::join_paths(&entries)
        .map(|joined| joined.to_string_lossy().into_owned())
        .unwrap_or_else(|_| {
            let sep = if cfg!(windows) { ";" } else { ":" };
            entries
                .iter()
                .map(|entry| entry.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(sep)
        });
    (key, joined)
}

#[cfg(test)]
mod tests;
