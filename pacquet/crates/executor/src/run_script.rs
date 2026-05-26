use crate::{
    extend_path::{ScriptsPrependNodePath, extend_path},
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
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
};

/// Inputs for [`run_script`], the foreground script runner behind
/// `pacquet run` (and the `test` / `start` aliases).
///
/// Unlike [`crate::run_lifecycle_hook`], which pipes a dependency's
/// install hooks into `pnpm:lifecycle` reporter events, this runner
/// inherits the parent's stdio so a user-invoked script writes straight
/// to the terminal. It mirrors pnpm's `runLifecycleHook` with
/// `stdio: 'inherit'` and `unsafePerm: true` (explicitly-run scripts are
/// trusted) from
/// <https://github.com/pnpm/pnpm/blob/80037699fb/exec/lifecycle/src/runLifecycleHook.ts>.
pub struct RunScript<'a> {
    /// Project directory the script runs in (`cwd`).
    pub pkg_root: &'a Path,
    /// Lifecycle event name (the script key, e.g. `build`). Stamped
    /// into `npm_lifecycle_event`.
    pub stage: &'a str,
    /// Script body from `package.json#scripts[stage]`, before args are
    /// appended.
    pub script: &'a str,
    /// Extra arguments passed after the script name on the CLI. Appended
    /// to the script body, shell-quoted, mirroring upstream's
    /// `${script} ${escapedArgs}` at
    /// <https://github.com/pnpm/pnpm/blob/80037699fb/exec/lifecycle/src/runLifecycleHook.ts#L91-L97>.
    pub args: &'a [String],
    /// The project's `package.json` body, for the `npm_package_*` stamp.
    pub manifest: &'a Value,
    /// Value for `INIT_CWD` (the directory pnpm was invoked from).
    pub init_cwd: &'a Path,
    pub extra_bin_paths: &'a [PathBuf],
    pub extra_env: &'a HashMap<String, String>,
    pub node_execpath: Option<&'a Path>,
    pub npm_execpath: Option<&'a Path>,
    pub user_agent: Option<&'a str>,
    pub scripts_prepend_node_path: ScriptsPrependNodePath,
    pub script_shell: Option<&'a Path>,
    /// When `true`, echo `$ <script>` to stderr before spawning,
    /// matching pnpm's non-silent foreground behavior.
    pub print_command: bool,
}

/// Error from [`run_script`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RunScriptError {
    #[display("Invalid script shell for `{stage}`: {source}")]
    #[diagnostic(code(pacquet_executor::run_script_shell))]
    ScriptShell {
        stage: String,
        #[error(source)]
        source: ScriptShellError,
    },

    #[display("Failed to spawn `{stage}` script: {source}")]
    #[diagnostic(code(pacquet_executor::run_script_spawn))]
    Spawn {
        stage: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed waiting for `{stage}` script: {source}")]
    #[diagnostic(code(pacquet_executor::run_script_wait))]
    Wait {
        stage: String,
        #[error(source)]
        source: std::io::Error,
    },
}

/// Run a single package script in the foreground, inheriting stdio.
///
/// Returns the child's [`ExitStatus`] so the caller can propagate the
/// script's exit code (pnpm exits the process with the failing script's
/// code). A non-zero exit is **not** an error here. Only a failure to
/// spawn the process, wait on it, or pick a shell is.
pub fn run_script(opts: RunScript<'_>) -> Result<ExitStatus, RunScriptError> {
    // Append CLI args to the script body, shell-quoted, so
    // `npm_lifecycle_script` and the spawned command both see them.
    let combined_script = if opts.args.is_empty() {
        opts.script.to_string()
    } else {
        format!("{} {}", opts.script, quote_args(opts.args))
    };

    if opts.print_command {
        eprintln!("$ {combined_script}");
    }

    let env_opts = EnvOptions {
        stage: opts.stage,
        script: &combined_script,
        pkg_root: opts.pkg_root,
        init_cwd: opts.init_cwd,
        script_src_dir: opts.pkg_root,
        node_execpath: opts.node_execpath,
        npm_execpath: opts.npm_execpath,
        // Explicitly-run scripts are trusted; no node-gyp wrapper.
        node_gyp_path: None,
        user_agent: opts.user_agent,
        // unsafePerm is true for explicitly-run scripts. No per-package
        // TMPDIR or privilege drop applies.
        unsafe_perm: true,
        extra_env: opts.extra_env,
    };
    let built = build_env(&env_opts, opts.manifest, env::vars().collect());

    let original_path = path_value(&built.env).map(OsString::from);
    let path_env = extend_path(
        opts.pkg_root,
        original_path.as_ref(),
        None,
        opts.extra_bin_paths,
        opts.scripts_prepend_node_path,
        opts.node_execpath,
    );

    let shell = select_shell(opts.script_shell, cfg!(windows))
        .map_err(|source| RunScriptError::ScriptShell { stage: opts.stage.to_string(), source })?;

    let mut child_env = built.env;
    child_env.retain(|key, _| !key.eq_ignore_ascii_case("PATH"));
    child_env.insert("PATH".to_string(), path_env.to_string_lossy().into_owned());

    let mut cmd = Command::new(&shell.program);
    cmd.args(&shell.args)
        .arg(&combined_script)
        .current_dir(opts.pkg_root)
        .env_clear()
        .envs(&child_env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let _ = shell.windows_verbatim_args;

    let mut child = cmd
        .spawn()
        .map_err(|source| RunScriptError::Spawn { stage: opts.stage.to_string(), source })?;

    child.wait().map_err(|source| RunScriptError::Wait { stage: opts.stage.to_string(), source })
}

/// Join `args` into a single string, quoting each element for the
/// platform shell.
///
/// On POSIX this mirrors `shlex.join` (used by upstream at
/// <https://github.com/pnpm/pnpm/blob/80037699fb/exec/lifecycle/src/runLifecycleHook.ts#L95>);
/// on Windows it mirrors the `args.map(JSON.stringify).join(' ')` branch
/// at line 93, since cmd.exe can't quote arguments containing newlines.
fn quote_args(args: &[String]) -> String {
    args.iter().map(|arg| quote_arg(arg)).collect::<Vec<_>>().join(" ")
}

#[cfg(not(windows))]
fn quote_arg(arg: &str) -> String {
    // Empty string must be quoted so it survives as a distinct argument.
    if arg.is_empty() {
        return "''".to_string();
    }
    // Leave shell-safe strings untouched (matches shlex's allowlist).
    if arg.bytes().all(is_shlex_safe) {
        return arg.to_string();
    }
    // Wrap in single quotes, ending the quote / escaping / reopening for
    // each embedded `'`.
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('\'');
    for ch in arg.chars() {
        if ch == '\'' {
            out.push_str(r"'\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[cfg(not(windows))]
fn is_shlex_safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(byte, b'@' | b'%' | b'+' | b'=' | b':' | b',' | b'.' | b'/' | b'-' | b'_')
}

#[cfg(windows)]
fn quote_arg(arg: &str) -> String {
    Value::String(arg.to_string()).to_string()
}

#[cfg(test)]
mod tests;
