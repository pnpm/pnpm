use crate::{
    extend_path::{ScriptsPrependNodePath, extend_path},
    make_env::{EnvOptions, build_env, path_value},
    shell::{ScriptShellError, select_shell},
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_package_manifest::{PackageManifestError, safe_read_package_json_from_dir};
use pacquet_reporter::{
    LifecycleLog, LifecycleMessage, LifecycleStdio, LogEvent, LogLevel, Reporter,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    thread,
};

/// Error from running lifecycle scripts.
///
/// Ports pnpm's error shape from `exec/lifecycle/src/runLifecycleHook.ts`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LifecycleScriptError {
    #[display("Failed to read package.json at {path}: {source}")]
    #[diagnostic(code(pacquet_executor::read_manifest))]
    ReadManifest {
        path: String,
        #[error(source)]
        source: PackageManifestError,
    },

    #[display("{dep_path} {stage}: `{script}` exited with {status}")]
    #[diagnostic(code(pacquet_executor::lifecycle_script_failed))]
    ScriptFailed { dep_path: String, stage: String, script: String, status: ExitStatus },

    #[display("Failed to spawn lifecycle script for {dep_path} {stage}: {source}")]
    #[diagnostic(code(pacquet_executor::spawn_lifecycle))]
    Spawn {
        dep_path: String,
        stage: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed waiting for lifecycle script for {dep_path} {stage}: {source}")]
    #[diagnostic(code(pacquet_executor::wait_lifecycle))]
    Wait {
        dep_path: String,
        stage: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Invalid script shell for {dep_path} {stage}: {source}")]
    #[diagnostic(code(pacquet_executor::invalid_script_shell))]
    ScriptShell {
        dep_path: String,
        stage: String,
        #[error(source)]
        source: ScriptShellError,
    },
}

/// Options for [`run_postinstall_hooks`].
///
/// Ports the subset of `RunLifecycleHookOptions` from
/// `exec/lifecycle/src/runLifecycleHook.ts` that the headless
/// installer needs.
pub struct RunPostinstallHooks<'a> {
    pub dep_path: &'a str,
    pub pkg_root: &'a Path,
    pub root_modules_dir: &'a Path,
    pub init_cwd: &'a Path,
    pub extra_bin_paths: &'a [PathBuf],
    pub extra_env: &'a HashMap<String, String>,
    /// Path to a `node` binary for `npm_node_execpath` / `NODE`. When
    /// `None`, [`crate::build_env`] falls back to looking `node` up
    /// on `PATH`. Required for native postinstalls that shell out
    /// via `$NODE`.
    pub node_execpath: Option<&'a Path>,
    /// Path written into `npm_execpath` so postinstalls can re-invoke
    /// the package manager. When `None`, `std::env::current_exe()`
    /// is used.
    pub npm_execpath: Option<&'a Path>,
    /// Bundled `node-gyp` wrapper path written into
    /// `npm_config_node_gyp`. Pacquet does not ship one yet, so
    /// callers pass `None`.
    pub node_gyp_path: Option<&'a Path>,
    /// Value written into `npm_config_user_agent`. Caller-supplied
    /// (typically `"pacquet/<version>"`); `None` skips the stamp.
    pub user_agent: Option<&'a str>,
    /// When `false`, a per-package `node_modules/.tmp` directory is
    /// created and exposed as `TMPDIR`, and (on POSIX) lifecycle
    /// scripts run with a dropped uid/gid. Pacquet does not yet
    /// surface the privilege drop, so callers currently pass
    /// `true` everywhere.
    pub unsafe_perm: bool,
    /// Bundled `node-gyp` shim directory prepended to `PATH`. Pacquet
    /// does not ship one yet; callers pass `None`.
    pub node_gyp_bin: Option<&'a Path>,
    /// Tri-state from `scriptsPrependNodePath` config. `Never` is the
    /// safe default; `Always` appends `dirname(node)` to `PATH`.
    pub scripts_prepend_node_path: ScriptsPrependNodePath,
    /// Custom shell from `scriptShell` config (e.g. `bash`,
    /// `/usr/local/bin/bash`). `None` means use the platform default
    /// (`sh -c` on POSIX, `cmd /d /s /c` on Windows).
    pub script_shell: Option<&'a Path>,
    /// Whether the dep is reachable only through optional edges
    /// (`snapshots[<key>].optional` in the v9 lockfile). Stamped
    /// into the `pnpm:lifecycle` `Script` and `Exit` events so
    /// downstream reporters can dispatch correctly, mirroring
    /// upstream's `lifecycleLogger.debug({ optional, … })` at
    /// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/exec/lifecycle/src/runLifecycleHook.ts#L102>.
    /// Does NOT affect failure handling — `BuildModules` consults the
    /// same flag independently to decide whether to swallow a build
    /// failure (see [#397](https://github.com/pnpm/pacquet/issues/397) item 6).
    pub optional: bool,
}

/// The lifecycle stages pnpm runs for a *dependency* during the build
/// phase, in execution order.
const DEPENDENCY_LIFECYCLE_STAGES: [&str; 3] = ["preinstall", "install", "postinstall"];

/// The lifecycle stages pnpm runs for each workspace *project* during
/// `pnpm install`, in execution order. Mirrors the hardcoded list at
/// the `runLifecycleHooksConcurrently` call sites in
/// [`pkg-manager/core`](https://github.com/pnpm/pnpm/blob/80037699fb/pkg-manager/core/src/install/index.ts#L1525)
/// and
/// [`pkg-manager/headless`](https://github.com/pnpm/pnpm/blob/80037699fb/pkg-manager/headless/src/index.ts#L671).
pub const PROJECT_LIFECYCLE_STAGES: [&str; 6] =
    ["preinstall", "install", "postinstall", "preprepare", "prepare", "postprepare"];

/// Run the preinstall, install, and postinstall lifecycle scripts for
/// a single dependency.
///
/// Ports `runPostinstallHooks` from
/// `https://github.com/pnpm/pnpm/blob/80037699fb/exec/lifecycle/src/index.ts`.
///
/// Returns `true` if any script was present and executed.
pub fn run_postinstall_hooks<Reporter: self::Reporter>(
    opts: &RunPostinstallHooks<'_>,
) -> Result<bool, LifecycleScriptError> {
    run_lifecycle_stages::<Reporter>(opts, &DEPENDENCY_LIFECYCLE_STAGES)
}

/// Run a workspace project's own lifecycle scripts during
/// `pnpm install` — preinstall, install, postinstall, preprepare,
/// prepare, postprepare, in that order.
///
/// Ports the per-importer body of `runLifecycleHooksConcurrently` from
/// `https://github.com/pnpm/pnpm/blob/80037699fb/exec/lifecycle/src/runLifecycleHooksConcurrently.ts`.
/// The caller fans this out across projects (and is responsible for
/// linking each project's bins beforehand so a later project's scripts
/// can resolve binaries built by an earlier one).
///
/// Returns `true` if any script was present and executed.
pub fn run_project_lifecycle_scripts<Reporter: self::Reporter>(
    opts: &RunPostinstallHooks<'_>,
) -> Result<bool, LifecycleScriptError> {
    run_lifecycle_stages::<Reporter>(opts, &PROJECT_LIFECYCLE_STAGES)
}

/// Read the manifest at `opts.pkg_root` and run each of `stages` whose
/// script is present, in order. Shared by [`run_postinstall_hooks`]
/// and [`run_project_lifecycle_scripts`].
///
/// The `install` stage falls back to `node-gyp rebuild` when neither
/// `install` nor `preinstall` is defined and a `binding.gyp` exists,
/// matching `checkBindingGyp` at
/// <https://github.com/pnpm/pnpm/blob/80037699fb/exec/lifecycle/src/runLifecycleHook.ts#L181-L188>.
/// The `npx only-allow pnpm` guard script is skipped — it does nothing
/// under pnpm/pacquet.
fn run_lifecycle_stages<Reporter: self::Reporter>(
    opts: &RunPostinstallHooks<'_>,
    stages: &[&str],
) -> Result<bool, LifecycleScriptError> {
    let manifest = match safe_read_package_json_from_dir(opts.pkg_root) {
        Ok(Some(value)) => value,
        Ok(None) => return Ok(false),
        Err(source) => {
            return Err(LifecycleScriptError::ReadManifest {
                path: opts.pkg_root.join("package.json").display().to_string(),
                source,
            });
        }
    };

    let scripts = manifest.get("scripts").and_then(|v| v.as_object());
    let get_script =
        |name: &str| -> Option<&str> { scripts.and_then(|s| s.get(name)).and_then(|v| v.as_str()) };

    // Snapshot the process env once for this package. Every stage reads
    // from this snapshot, which keeps the runs observably consistent
    // and avoids one call to `env::vars()` per stage over a
    // thread-shared global.
    let parent_env: HashMap<String, String> = env::vars().collect();

    let mut ran_any = false;

    for &stage in stages {
        let script = if stage == "install" {
            get_script("install").map(String::from).or_else(|| {
                (get_script("preinstall").is_none() && opts.pkg_root.join("binding.gyp").exists())
                    .then(|| "node-gyp rebuild".to_string())
            })
        } else {
            get_script(stage).map(String::from)
        };

        let Some(script) = script else { continue };
        if script == "npx only-allow pnpm" {
            continue;
        }

        run_lifecycle_hook::<Reporter>(stage, &script, opts, &manifest, &parent_env)?;
        ran_any = true;
    }

    Ok(ran_any)
}

/// Run a single lifecycle hook and emit `pnpm:lifecycle` events.
///
/// Ports the core of `runLifecycleHook` from
/// `https://github.com/pnpm/pnpm/blob/80037699fb/exec/lifecycle/src/runLifecycleHook.ts`.
///
/// Mirrors the upstream emit ordering: a `Script` event before the spawn,
/// `Stdio` events for each stdout/stderr line, then an `Exit` event with
/// the resolved exit code.
///
/// `parent_env` is captured by the caller so multi-stage callers (the
/// `run_postinstall_hooks` wrapper and `pacquet-git-fetcher`'s
/// `preparePackage` port) can snapshot once and reuse across stages,
/// matching upstream's behavior where each stage sees the same parent
/// env regardless of what siblings wrote into the process's own env.
pub fn run_lifecycle_hook<Reporter: self::Reporter>(
    stage: &str,
    script: &str,
    opts: &RunPostinstallHooks<'_>,
    manifest: &Value,
    parent_env: &HashMap<String, String>,
) -> Result<(), LifecycleScriptError> {
    tracing::debug!(
        target: "pacquet::lifecycle",
        dep_path = opts.dep_path,
        stage,
        script,
        pkg_root = %opts.pkg_root.display(),
    );

    let pkg_root_str = opts.pkg_root.to_string_lossy().into_owned();

    // Mirrors `lifecycleLogger.debug({ depPath, optional, script, stage, wd })`
    // at <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/exec/lifecycle/src/runLifecycleHook.ts#L102>.
    Reporter::emit(&LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Script {
            dep_path: opts.dep_path.to_string(),
            optional: opts.optional,
            script: script.to_string(),
            stage: stage.to_string(),
            wd: pkg_root_str.clone(),
        },
    }));

    let env_opts = EnvOptions {
        stage,
        script,
        pkg_root: opts.pkg_root,
        init_cwd: opts.init_cwd,
        script_src_dir: opts.pkg_root,
        node_execpath: opts.node_execpath,
        npm_execpath: opts.npm_execpath,
        node_gyp_path: opts.node_gyp_path,
        user_agent: opts.user_agent,
        unsafe_perm: opts.unsafe_perm,
        extra_env: opts.extra_env,
    };
    let built = build_env(&env_opts, manifest, parent_env.clone());

    if let Some(tmpdir) = &built.tmpdir {
        // `fs::create_dir_all` is idempotent for existing
        // directories (it returns `Ok(())`), so the upstream
        // `EEXIST` swallow at index.js:97-102 doesn't translate.
        // Treat any error here — including `AlreadyExists`, which
        // signals a *file* at that path — as a real spawn failure.
        fs::create_dir_all(tmpdir).map_err(|error| LifecycleScriptError::Spawn {
            dep_path: opts.dep_path.to_string(),
            stage: stage.to_string(),
            source: error,
        })?;
    }

    // Mirrors the `env[PATH] = extendPath(...)` line in `lifecycle_`
    // at index.js:116, with the original PATH coming from the
    // (already-filtered) parent env captured during `build_env`.
    // Lookup is case-insensitive because Windows preserves the
    // system casing (typically `Path`) on env keys.
    let original_path = path_value(&built.env).map(OsString::from);
    let path_env = extend_path(
        opts.pkg_root,
        original_path.as_ref(),
        opts.node_gyp_bin,
        opts.extra_bin_paths,
        opts.scripts_prepend_node_path,
        opts.node_execpath,
    );

    // Pick the shell up front so a misconfigured `scriptShell` fails
    // before we touch the filesystem (TMPDIR etc. already created
    // above — that's a minor leak, but matches upstream where
    // `makeEnv` runs before the `runCmd_` shell pick anyway).
    let shell = select_shell(opts.script_shell, cfg!(windows)).map_err(|source| {
        LifecycleScriptError::ScriptShell {
            dep_path: opts.dep_path.to_string(),
            stage: stage.to_string(),
            source,
        }
    })?;

    // Drop any inherited PATH-like key (`Path` on Windows, `PATH`
    // on POSIX) from the env map before spawning — otherwise on
    // Windows the spawn would see both that and the explicit `PATH`
    // we set below, and `Command::env` deduplicates them with an
    // unspecified winner.
    let mut child_env = built.env;
    child_env.retain(|key, _| !key.eq_ignore_ascii_case("PATH"));
    child_env.insert("PATH".to_string(), path_env.to_string_lossy().into_owned());

    let mut cmd = Command::new(&shell.program);
    cmd.args(&shell.args);
    // Append the script body. The chain is broken here because the
    // Windows `cmd /d /s /c` path needs `raw_arg` rather than `arg`
    // (see [`push_script_arg`]) — a branch the method chain can't
    // express.
    push_script_arg(&mut cmd, script, shell.windows_verbatim_args);
    cmd.current_dir(opts.pkg_root)
        // Stripping inherited env so leftover npm_* keys from a wrapping
        // invocation cannot leak in. `build_env` already folded the
        // surviving parent keys into `built.env`.
        .env_clear()
        .envs(&child_env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|error| LifecycleScriptError::Spawn {
        dep_path: opts.dep_path.to_string(),
        stage: stage.to_string(),
        source: error,
    })?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdout_handle = stdout.map(|stream| {
        spawn_line_pump::<Reporter>(
            stream,
            LifecycleStdio::Stdout,
            opts.dep_path,
            stage,
            &pkg_root_str,
        )
    });
    let stderr_handle = stderr.map(|stream| {
        spawn_line_pump::<Reporter>(
            stream,
            LifecycleStdio::Stderr,
            opts.dep_path,
            stage,
            &pkg_root_str,
        )
    });

    let status = child.wait().map_err(|error| LifecycleScriptError::Wait {
        dep_path: opts.dep_path.to_string(),
        stage: stage.to_string(),
        source: error,
    })?;

    // Joining the pumps after `wait` ensures every line they read is
    // emitted before the `Exit` event below, matching pnpm's ordering.
    if let Some(h) = stdout_handle {
        let _ = h.join();
    }
    if let Some(h) = stderr_handle {
        let _ = h.join();
    }

    // Mirrors `lifecycleLogger.debug({ depPath, exitCode, optional, stage, wd })`
    // at <https://github.com/pnpm/pnpm/blob/80037699fb/exec/lifecycle/src/runLifecycleHook.ts#L165>.
    Reporter::emit(&LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Exit {
            dep_path: opts.dep_path.to_string(),
            exit_code: status.code().unwrap_or(-1),
            optional: opts.optional,
            stage: stage.to_string(),
            wd: pkg_root_str,
        },
    }));

    if !status.success() {
        return Err(LifecycleScriptError::ScriptFailed {
            dep_path: opts.dep_path.to_string(),
            stage: stage.to_string(),
            script: script.to_string(),
            status,
        });
    }

    Ok(())
}

/// Append the script body as the shell command's final argument.
///
/// On Windows the `cmd /d /s /c` path passes `windows_verbatim_args =
/// true`; the script is then appended with
/// `std::os::windows::process::CommandExt::raw_arg` so embedded quoting
/// (e.g. `node -e "..."`) reaches the child untouched. This mirrors
/// Node's `windowsVerbatimArguments` at
/// <https://github.com/pnpm/npm-lifecycle/blob/d2d8e790/index.js#L251>;
/// the default `arg` quoting would escape the inner `"` and break such
/// commands under `cmd.exe`. Everywhere else (POSIX `sh -c`, a custom
/// `scriptShell`) the standard `arg` is correct.
#[cfg(windows)]
pub fn push_script_arg(cmd: &mut Command, script: &str, windows_verbatim_args: bool) {
    use std::os::windows::process::CommandExt;
    if windows_verbatim_args {
        cmd.raw_arg(script);
    } else {
        cmd.arg(script);
    }
}

#[cfg(not(windows))]
pub fn push_script_arg(cmd: &mut Command, script: &str, _windows_verbatim_args: bool) {
    cmd.arg(script);
}

/// Spawn a thread that reads `reader` line-by-line and emits a
/// `LifecycleMessage::Stdio` event per line. Mirrors the per-chunk
/// logging callback at
/// <https://github.com/pnpm/pnpm/blob/80037699fb/exec/lifecycle/src/runLifecycleHook.ts#L147>.
fn spawn_line_pump<Reporter: self::Reporter>(
    reader: impl Read + Send + 'static,
    stdio: LifecycleStdio,
    dep_path: &str,
    stage: &str,
    wd: &str,
) -> thread::JoinHandle<()> {
    let dep_path = dep_path.to_string();
    let stage = stage.to_string();
    let wd = wd.to_string();
    thread::spawn(move || {
        let buf = BufReader::new(reader);
        for line in buf.lines() {
            let Ok(line) = line else {
                // Stop pumping on read error — an EBADF or EPIPE means
                // the child closed the stream. Errors are not fatal to
                // the install; the wait below will surface a non-zero
                // exit code if the child failed because of them.
                break;
            };
            Reporter::emit(&LogEvent::Lifecycle(LifecycleLog {
                level: LogLevel::Debug,
                message: LifecycleMessage::Stdio {
                    dep_path: dep_path.clone(),
                    line,
                    stage: stage.clone(),
                    stdio,
                    wd: wd.clone(),
                },
            }));
        }
    })
}

#[cfg(test)]
mod tests;
