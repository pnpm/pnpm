use crate::{
    extend_path::{ScriptsPrependNodePath, extend_path},
    lifecycle::push_script_arg,
    make_env::{EnvOptions, build_env, path_value},
    shell::{ScriptShellError, select_shell},
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

/// Error from running a user script through [`run_script`].
///
/// Ports the failure modes of pnpm's `runLifecycleHook` for the
/// `stdio: 'inherit'` path at
/// <https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/lifecycle/src/runLifecycleHook.ts>.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RunScriptError {
    #[display("Failed to spawn script `{script}`: {source}")]
    #[diagnostic(code(pacquet_executor::run_script_spawn))]
    Spawn {
        script: String,
        #[error(source)]
        source: io::Error,
    },

    #[diagnostic(transparent)]
    ScriptShell(#[error(source)] ScriptShellError),
}

/// Inputs for [`run_script`].
///
/// Mirrors the subset of pnpm's `RunLifecycleHookOptions`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/lifecycle/src/runLifecycleHook.ts#L15-L31>)
/// that a foreground `pnpm run` invocation needs.
pub struct RunScript<'a> {
    /// The package manifest, used to stamp `npm_package_*` env vars.
    pub manifest: &'a Value,
    /// The lifecycle stage, written into `npm_lifecycle_event` (the
    /// script name for `pnpm run <name>`, or `pre`/`post` variants).
    pub stage: &'a str,
    /// The script body to run.
    pub script: &'a str,
    /// Arguments appended to the script after shell-quoting. Only the
    /// main stage receives these; `pre`/`post` stages pass `&[]`.
    pub args: &'a [String],
    /// The project directory the script runs in.
    pub pkg_root: &'a Path,
    /// Value written into `INIT_CWD`.
    pub init_cwd: &'a Path,
    /// Extra directories prepended to `PATH` (the `extraBinPaths` config).
    pub extra_bin_paths: &'a [PathBuf],
    /// Custom shell from the `scriptShell` config, if any.
    pub script_shell: Option<&'a Path>,
    /// The `scriptsPrependNodePath` config.
    pub scripts_prepend_node_path: ScriptsPrependNodePath,
    /// Path to a `node` binary for `npm_node_execpath` / `NODE`.
    pub node_execpath: Option<&'a Path>,
    /// Path written into `npm_execpath`.
    pub npm_execpath: Option<&'a Path>,
    /// Value written into `npm_config_user_agent`.
    pub user_agent: Option<&'a str>,
    /// Extra environment variables (the `extraEnv` / `NODE_OPTIONS` set).
    pub extra_env: &'a HashMap<String, String>,
    /// When `true`, suppress the `$ <script>` echo to stderr.
    pub silent: bool,
}

/// Run a single user script in the foreground, inheriting the parent's
/// stdio so the script's output reaches the terminal directly.
///
/// Ports the `stdio: 'inherit'` branch of pnpm's `runLifecycleHook`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/lifecycle/src/runLifecycleHook.ts#L33-L145>):
/// it sets up `node_modules/.bin` on `PATH` and the `npm_*` environment
/// via [`build_env`] / [`extend_path`], echoes `$ <script>` to stderr
/// unless `silent`, then spawns the script under the selected shell.
///
/// Returns the script's [`ExitStatus`] so the caller can propagate its
/// exit code, matching pnpm's behavior where a failing script sets the
/// process exit code.
pub fn run_script(opts: &RunScript<'_>) -> Result<ExitStatus, RunScriptError> {
    let command = build_command(opts.script, opts.args);

    let shell =
        select_shell(opts.script_shell, cfg!(windows)).map_err(RunScriptError::ScriptShell)?;

    let parent_env: HashMap<String, String> = env::vars().collect();
    let env_opts = EnvOptions {
        stage: opts.stage,
        script: &command,
        pkg_root: opts.pkg_root,
        init_cwd: opts.init_cwd,
        script_src_dir: opts.pkg_root,
        node_execpath: opts.node_execpath,
        npm_execpath: opts.npm_execpath,
        node_gyp_path: None,
        user_agent: opts.user_agent,
        // Explicit `pnpm run` invocations are trusted, so the temp-dir /
        // privilege-drop path is skipped. Matches `unsafePerm: true` at
        // <https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/run.ts#L284>.
        unsafe_perm: true,
        extra_env: opts.extra_env,
    };
    let built = build_env(&env_opts, opts.manifest, parent_env);

    let original_path = path_value(&built.env).map(OsString::from);
    let path_env = extend_path(
        opts.pkg_root,
        original_path.as_ref(),
        None,
        opts.extra_bin_paths,
        opts.scripts_prepend_node_path,
        opts.node_execpath,
    );

    let mut child_env = built.env;
    child_env.retain(|key, _| !key.eq_ignore_ascii_case("PATH"));
    child_env.insert("PATH".to_string(), path_env.to_string_lossy().into_owned());

    if !opts.silent {
        // Mirrors the `$ <script>` echo pnpm writes to stderr for an
        // inherited-stdio run at runLifecycleHook.ts:110. The dim styling
        // upstream applies through chalk is omitted.
        let mut stderr = io::stderr();
        let _ = writeln!(stderr, "$ {command}");
    }

    // The script is appended through `push_script_arg` (not a chained
    // `.arg`) so the Windows `cmd /d /s /c` verbatim path can use
    // `raw_arg` and keep embedded quoting like `node -e "..."` intact —
    // matching the lifecycle runner.
    let mut cmd = Command::new(&shell.program);
    cmd.args(&shell.args);
    push_script_arg(&mut cmd, &command, shell.windows_verbatim_args);
    let status = cmd
        .current_dir(opts.pkg_root)
        .env_clear()
        .envs(&child_env)
        .status()
        .map_err(|source| RunScriptError::Spawn { script: command.clone(), source })?;

    Ok(status)
}

/// Append shell-quoted `args` to `script`, matching the arg escaping in
/// pnpm's `runLifecycleHook`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/lifecycle/src/runLifecycleHook.ts#L91-L97>):
/// `shlex.join` on POSIX and per-argument `JSON.stringify` on Windows.
fn build_command(script: &str, args: &[String]) -> String {
    if args.is_empty() {
        return script.to_string();
    }
    let quoted = if cfg!(windows) {
        args.iter().map(|arg| Value::String(arg.clone()).to_string()).collect::<Vec<_>>().join(" ")
    } else {
        args.iter().map(|arg| posix_quote(arg)).collect::<Vec<_>>().join(" ")
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
    let safe = arg.chars().all(|ch| ch.is_ascii_alphanumeric() || "_@%+=:,./-".contains(ch));
    if safe { arg.to_string() } else { format!("'{}'", arg.replace('\'', r#"'"'"'"#)) }
}

#[cfg(test)]
mod tests;
