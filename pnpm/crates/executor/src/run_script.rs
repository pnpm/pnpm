use crate::{
    extend_path::extend_path,
    lifecycle::{StreamedScript, push_script_arg},
    make_env::{EnvOptions, build_env, path_value},
    process_tracker::{ProcessTracker, spawn_child},
    script_exit::ScriptExit,
    shell::{ScriptShellError, SelectedShell, select_shell},
    shell_emulator::{EmulatedOutput, ShellEmulatorError, execute_emulated},
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_reporter::LogEvent;
use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
};

/// Error from running a user script through [`run_script`] — the
/// failure modes of the `stdio: 'inherit'` foreground path.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RunScriptError {
    #[display("Failed to spawn script `{script}`: {source}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_RUN_SCRIPT_SPAWN))]
    Spawn {
        script: String,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed waiting for script `{script}`: {source}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_RUN_SCRIPT_WAIT))]
    Wait {
        script: String,
        #[error(source)]
        source: io::Error,
    },

    #[diagnostic(transparent)]
    ScriptShell(#[error(source)] ScriptShellError),

    #[diagnostic(transparent)]
    ShellEmulator(#[error(source)] ShellEmulatorError),
}

/// Where a script's output goes.
#[derive(Clone, Copy)]
pub enum ScriptOutput<'a> {
    /// Inherit the parent's stdio, so the script writes straight to the
    /// terminal.
    Inherit,
    /// Pipe the child and republish its output as `pnpm:lifecycle`
    /// events, one per line — pnpm's `--stream`. The reporter renders
    /// each line under the project it came from, which is what makes
    /// concurrent projects' output readable.
    Streamed {
        /// Identifies the script in the emitted events. `pnpm run` names
        /// the project directory; `pnpm exec` names the package.
        dep_path: &'a str,
        emit: fn(&LogEvent),
    },
}

/// Inputs for [`run_script`] — the subset of lifecycle-hook options a
/// foreground `pnpm run` invocation needs.
pub struct RunScript<'a> {
    /// The package manifest, used to stamp `npm_package_*` env vars.
    pub manifest: &'a Value,
    /// The project directory the script runs in.
    pub pkg_root: &'a Path,
    /// When `true`, suppress the `$ <script>` echo to stderr.
    pub silent: bool,
    /// Where the script's output goes.
    pub output: ScriptOutput<'a>,
    /// Tracks this script for cancellation when a recursive command bails.
    pub process_tracker: Option<&'a ProcessTracker>,
    pub environment: crate::ScriptEnvironment<'a>,
    pub execution: crate::ScriptExecutionOptions<'a>,
    pub invocation: crate::ScriptInvocation<'a>,
}

/// Run a single user script in the foreground, sending its output where
/// [`RunScript::output`] says.
pub fn run_script(opts: &RunScript<'_>) -> Result<ScriptExit, RunScriptError> {
    let command = build_command(
        opts.invocation.script,
        opts.invocation.args,
        parsed_by_windows_shell(cfg!(windows), opts.execution.shell_emulator),
    );

    // The `scriptShell` value is validated even when the emulator will
    // run the script, matching pnpm's `runLifecycleHook`, which rejects a
    // `.bat` / `.cmd` shell before it looks at `shellEmulator`.
    let shell =
        select_shell(opts.execution.shell, cfg!(windows)).map_err(RunScriptError::ScriptShell)?;

    let child_env = child_env(opts, &command);

    if let ScriptOutput::Streamed { dep_path, emit } = opts.output {
        let wd = opts.pkg_root.to_string_lossy().into_owned();
        let streamed = StreamedScript { dep_path, stage: opts.invocation.stage, wd: &wd, emit };
        return run_streamed(opts, &shell, &command, &child_env, streamed);
    }

    if !opts.silent {
        // Echo `$ <script>` to stderr for an inherited-stdio run, the
        // same as `pnpm run`. The dim styling is omitted.
        let mut stderr = io::stderr();
        let _ = writeln!(stderr, "$ {command}");
    }

    if opts.execution.shell_emulator {
        return execute_emulated(
            &command,
            opts.pkg_root,
            &child_env,
            EmulatedOutput::Inherit,
            opts.process_tracker,
        )
        .map(ScriptExit::Emulated)
        .map_err(RunScriptError::ShellEmulator);
    }

    run_in_shell(opts, &shell, &command, &child_env)
}

/// The script's environment: the parent's, the `npm_*` lifecycle variables,
/// and `PATH` extended with the bin directories.
fn child_env(opts: &RunScript<'_>, command: &str) -> HashMap<String, String> {
    let parent_env: HashMap<String, String> = env::vars().collect();
    let env_opts = EnvOptions {
        environment: crate::ScriptEnvironment {
            init_cwd: opts.environment.init_cwd,
            node_execpath: opts.environment.node_execpath,
            npm_execpath: opts.environment.npm_execpath,
            node_gyp_path: None,
            user_agent: opts.environment.user_agent,
            extra_env: opts.environment.extra_env,
        },
        stage: opts.invocation.stage,
        script: command,
        pkg_root: opts.pkg_root,

        script_src_dir: opts.pkg_root,

        // Explicit `pnpm run` invocations are trusted, so the temp-dir /
        // privilege-drop path is skipped (`unsafe_perm: true`).
        unsafe_perm: true,
    };
    let built = build_env(&env_opts, opts.manifest, parent_env);

    let original_path = path_value(&built.env).map(OsString::from);
    let path_env = extend_path(
        opts.pkg_root,
        original_path.as_ref(),
        crate::bundled_node_gyp_bin(),
        opts.execution.extra_bin_paths,
        opts.execution.prepend_node_path,
        opts.environment.node_execpath,
    );

    let mut child_env = built.env;
    child_env.retain(|key, _| !key.eq_ignore_ascii_case("PATH"));
    child_env.insert("PATH".to_string(), path_env.to_string_lossy().into_owned());
    child_env
}

fn run_streamed(
    opts: &RunScript<'_>,
    shell: &SelectedShell,
    command: &str,
    child_env: &HashMap<String, String>,
    streamed: StreamedScript<'_>,
) -> Result<ScriptExit, RunScriptError> {
    streamed.started(command);
    let status = if opts.execution.shell_emulator {
        let emit_line = |stdio, line| streamed.emit_line(stdio, line);
        execute_emulated(
            command,
            opts.pkg_root,
            child_env,
            EmulatedOutput::Lines(&emit_line),
            opts.process_tracker,
        )
        .map(ScriptExit::Emulated)
        .map_err(RunScriptError::ShellEmulator)?
    } else {
        run_piped(shell, command, opts.pkg_root, child_env, streamed, opts.process_tracker)?
    };
    streamed.finished(status.code().unwrap_or(-1));
    Ok(status)
}

/// The script is appended through `push_script_arg` (not a chained
/// `.arg`) so the Windows `cmd /d /s /c` verbatim path can use
/// `raw_arg` and keep embedded quoting like `node -e "..."` intact —
/// matching the lifecycle runner.
fn run_in_shell(
    opts: &RunScript<'_>,
    shell: &SelectedShell,
    command: &str,
    child_env: &HashMap<String, String>,
) -> Result<ScriptExit, RunScriptError> {
    let mut cmd = Command::new(&shell.program);
    cmd.args(&shell.args);
    push_script_arg(&mut cmd, command, shell.windows_verbatim_args);
    cmd.current_dir(opts.pkg_root)
        .env_clear()
        .envs(child_env);
    let mut child = spawn_child(&mut cmd, opts.process_tracker)
        .map_err(|source| RunScriptError::Spawn { script: command.to_string(), source })?;
    let status = child
        .wait()
        .map_err(|source| RunScriptError::Wait { script: command.to_string(), source })?;
    Ok(ScriptExit::Process(status))
}

/// Spawn `command` under `shell` with both output streams piped, and
/// republish each line through `streamed`.
fn run_piped(
    shell: &SelectedShell,
    command: &str,
    pkg_root: &Path,
    child_env: &HashMap<String, String>,
    streamed: StreamedScript<'_>,
    process_tracker: Option<&ProcessTracker>,
) -> Result<ScriptExit, RunScriptError> {
    let mut cmd = Command::new(&shell.program);
    cmd.args(&shell.args);
    push_script_arg(&mut cmd, command, shell.windows_verbatim_args);
    cmd.current_dir(pkg_root)
        .env_clear()
        .envs(child_env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn_child(&mut cmd, process_tracker)
        .map_err(|source| RunScriptError::Spawn { script: command.to_string(), source })?;

    streamed
        .pump(&mut child)
        .map(ScriptExit::Process)
        .map_err(|source| RunScriptError::Wait { script: command.to_string(), source })
}

/// Whether `cmd` will parse the script. The shell emulator is a POSIX
/// shell on every platform, so only a native Windows run reaches `cmd`.
fn parsed_by_windows_shell(windows: bool, shell_emulator: bool) -> bool {
    windows && !shell_emulator
}

/// Append shell-quoted `args` to `script`: per-argument JSON quoting when
/// `cmd` will parse them, and `shlex`-style POSIX quoting otherwise.
fn build_command(script: &str, args: &[String], windows_shell: bool) -> String {
    if args.is_empty() {
        return script.to_string();
    }
    let quoted = if windows_shell {
        args.iter()
            .map(|arg| Value::String(arg.clone()).to_string())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        args.iter()
            .map(|arg| posix_quote(arg))
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!("{script} {quoted}")
}

/// Quote a single argument the way the `shlex` npm package's `quote`
/// does: a string of only shell-safe characters is left as-is, anything
/// else is wrapped in single quotes with embedded quotes escaped as
/// `'"'"'`.
fn posix_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    let safe = arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "_@%+=:,./-".contains(ch));
    if safe { arg.to_string() } else { format!("'{}'", arg.replace('\'', r#"'"'"'"#)) }
}

#[cfg(test)]
mod tests;
