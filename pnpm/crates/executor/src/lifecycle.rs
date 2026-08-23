use crate::{
    extend_path::{ScriptsPrependNodePath, extend_path},
    make_env::{EnvOptions, build_env, path_value},
    script_exit::ScriptExit,
    shell::{ScriptShellError, SelectedShell, select_shell},
    shell_emulator::{EmulatedOutput, ShellEmulatorError, execute_emulated},
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_package_manifest::{PackageManifestError, safe_read_package_json_from_dir};
use pnpm_reporter::{LifecycleLog, LifecycleMessage, LifecycleStdio, LogEvent, LogLevel, Reporter};
use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    fs,
    io::{self, BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader as AsyncBufReader};

/// Error from running lifecycle scripts.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LifecycleScriptError {
    #[display("Failed to read package.json at {path}: {source}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_READ_MANIFEST))]
    ReadManifest {
        path: String,
        #[error(source)]
        source: PackageManifestError,
    },

    #[display("{dep_path} {stage}: `{script}` exited with {status}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_LIFECYCLE_SCRIPT_FAILED))]
    ScriptFailed { dep_path: String, stage: String, script: String, status: ScriptExit },

    #[display("Failed to spawn lifecycle script for {dep_path} {stage}: {source}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_SPAWN_LIFECYCLE))]
    Spawn {
        dep_path: String,
        stage: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed waiting for lifecycle script for {dep_path} {stage}: {source}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_WAIT_LIFECYCLE))]
    Wait {
        dep_path: String,
        stage: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Invalid script shell for {dep_path} {stage}: {source}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_INVALID_SCRIPT_SHELL))]
    ScriptShell {
        dep_path: String,
        stage: String,
        #[error(source)]
        source: ScriptShellError,
    },

    #[diagnostic(transparent)]
    ShellEmulator(#[error(source)] ShellEmulatorError),
}

/// Options for [`run_postinstall_hooks`] — the subset of lifecycle-hook
/// inputs the headless installer needs.
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
    /// `node-gyp` entry point written into `npm_config_node_gyp`.
    /// `None` leaves the variable unset, which is what pnpm does: the
    /// wrapper found through [`node_gyp_bin`](Self::node_gyp_bin) reads
    /// this variable and falls back to the shipped copy when it is
    /// unset, so setting it here would override a user's own choice.
    pub node_gyp_path: Option<&'a Path>,
    /// Value written into `npm_config_user_agent`. Caller-supplied
    /// (typically `"pnpm/<version>"`); `None` skips the stamp.
    pub user_agent: Option<&'a str>,
    /// When `false`, a per-package `node_modules/.tmp` directory is
    /// created and exposed as `TMPDIR`, and (on POSIX) lifecycle
    /// scripts run with a dropped uid/gid. Pacquet does not yet
    /// surface the privilege drop, so callers currently pass
    /// `true` everywhere.
    pub unsafe_perm: bool,
    /// Directory holding the shipped `node-gyp` wrapper, prepended to
    /// `PATH` so install scripts that shell out to `node-gyp` resolve
    /// it. Supplied by [`crate::bundled_node_gyp_bin`]; `None` when
    /// nothing was shipped beside the executable.
    pub node_gyp_bin: Option<&'a Path>,
    /// Tri-state from `scriptsPrependNodePath` config. `Never` is the
    /// safe default; `Always` appends `dirname(node)` to `PATH`.
    pub scripts_prepend_node_path: ScriptsPrependNodePath,
    /// Custom shell from `scriptShell` config (e.g. `bash`,
    /// `/usr/local/bin/bash`). `None` means use the platform default
    /// (`sh -c` on POSIX, `cmd /d /s /c` on Windows).
    pub script_shell: Option<&'a Path>,
    /// The `shellEmulator` config: run the script through pacquet's
    /// built-in shell rather than the platform's. Callers that mirror a
    /// pnpm call site which does not thread the setting — publishing,
    /// packing, patching, git package preparation — pass `false`.
    pub shell_emulator: bool,
    /// Whether the dep is reachable only through optional edges
    /// (`snapshots[<key>].optional` in the v9 lockfile).
    /// Does NOT affect failure handling — `BuildModules` consults the
    /// same flag independently to decide whether to swallow a build
    /// failure (see [#397](https://github.com/pnpm/pacquet/issues/397) item 6).
    pub optional: bool,
}

/// The lifecycle stages pnpm runs for a *dependency* during the build
/// phase, in execution order.
const DEPENDENCY_LIFECYCLE_STAGES: [&str; 3] = ["preinstall", "install", "postinstall"];

/// The lifecycle stages pnpm runs for each workspace *project* during
/// `pnpm install`, in execution order.
pub const PROJECT_LIFECYCLE_STAGES: [&str; 6] =
    ["preinstall", "install", "postinstall", "preprepare", "prepare", "postprepare"];

/// The pnpm-specific hook the root project may define to prepare state
/// the install itself depends on. It runs before resolution, so unlike
/// [`PROJECT_LIFECYCLE_STAGES`] it cannot rely on `node_modules`.
pub const DEV_PREINSTALL_STAGE: &str = "pnpm:devPreinstall";

/// Set by the TypeScript CLI when it delegates a *resolving* install to
/// pacquet, to say it already ran the root project's
/// [`DEV_PREINSTALL_STAGE`] script itself. That path passes no flags of
/// its own — a frozen delegation is distinguishable by its
/// `--ignore-manifest-check` — so without this marker the hook would run
/// once on each side of the handover.
///
/// A private handshake between the two stacks for the lifetime of one
/// delegated install, which is why it sits outside the user-facing
/// `PNPM_CONFIG_*` namespace and why [`build_env`] drops it from every
/// script environment it builds: it describes the install currently
/// running, not any install a script of that install may start.
/// Its counterpart lives in the TypeScript CLI's `runPacquet.ts`.
///
/// [`build_env`]: crate::build_env
pub const DEV_PREINSTALL_ALREADY_RAN_ENV: &str = "PNPM_INTERNAL_DEV_PREINSTALL_ALREADY_RAN";

/// Run the preinstall, install, and postinstall lifecycle scripts for
/// a single dependency.
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

/// Run the root project's [`DEV_PREINSTALL_STAGE`] script, if it has one.
///
/// Returns `true` when the script was present and executed.
pub fn run_dev_preinstall_hook<Reporter: self::Reporter>(
    opts: &RunPostinstallHooks<'_>,
) -> Result<bool, LifecycleScriptError> {
    run_lifecycle_stages::<Reporter>(opts, &[DEV_PREINSTALL_STAGE])
}

/// Read the manifest at `opts.pkg_root` and run each of `stages` whose
/// script is present, in order. Shared by [`run_postinstall_hooks`],
/// [`run_project_lifecycle_scripts`], and [`run_dev_preinstall_hook`].
///
/// The `install` stage falls back to `node-gyp rebuild` when neither
/// `install` nor `preinstall` is defined and a `binding.gyp` exists.
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
/// `parent_env` is captured by the caller so multi-stage callers (the
/// [`run_postinstall_hooks`] wrapper and `pnpm-git-fetcher`'s
/// package-preparation step) can snapshot once and reuse across stages,
/// so each stage sees the same parent env regardless of what siblings
/// wrote into the process's own env.
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
        // directories (it returns `Ok(())`), so no `EEXIST` swallow is
        // needed. Treat any error here — including `AlreadyExists`,
        // which signals a *file* at that path — as a real spawn failure.
        fs::create_dir_all(tmpdir).map_err(|error| LifecycleScriptError::Spawn {
            dep_path: opts.dep_path.to_string(),
            stage: stage.to_string(),
            source: error,
        })?;
    }

    // Set PATH via `extend_path`, with the original PATH coming from
    // the (already-filtered) parent env captured during `build_env`.
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
    // above — that's a minor leak, but the env is built before the
    // shell pick anyway). The pick also runs when the emulator will
    // take over below, because pnpm rejects a `.bat` / `.cmd`
    // `scriptShell` regardless of `shellEmulator`.
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

    let status = if opts.shell_emulator {
        run_in_emulator::<Reporter>(script, opts, stage, &child_env, &pkg_root_str)?
    } else {
        run_in_shell::<Reporter>(&shell, script, opts, stage, &child_env, &pkg_root_str)?
    };

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

/// Spawn `script` under `shell`, pumping the child's output to the
/// reporter line by line, and return how it exited.
fn run_in_shell<Reporter: self::Reporter>(
    shell: &SelectedShell,
    script: &str,
    opts: &RunPostinstallHooks<'_>,
    stage: &str,
    env: &HashMap<String, String>,
    wd: &str,
) -> Result<ScriptExit, LifecycleScriptError> {
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
        .envs(env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|error| LifecycleScriptError::Spawn {
        dep_path: opts.dep_path.to_string(),
        stage: stage.to_string(),
        source: error,
    })?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let target = StreamedScript { dep_path: opts.dep_path, stage, wd, emit: Reporter::emit };
    let stdout_handle = stdout.map(|stream| target.pump_stream(stream, LifecycleStdio::Stdout));
    let stderr_handle = stderr.map(|stream| target.pump_stream(stream, LifecycleStdio::Stderr));

    let status = child.wait().map_err(|error| LifecycleScriptError::Wait {
        dep_path: opts.dep_path.to_string(),
        stage: stage.to_string(),
        source: error,
    })?;

    // Joining the pumps after `wait` ensures every line they read is
    // emitted before the caller's `Exit` event, matching pnpm's ordering.
    if let Some(handle) = stdout_handle {
        let _ = handle.join();
    }
    if let Some(handle) = stderr_handle {
        let _ = handle.join();
    }

    Ok(ScriptExit::Process(status))
}

/// Run `script` in the built-in shell (`shellEmulator`), emitting the
/// same per-line events as [`run_in_shell`], and return how it exited.
fn run_in_emulator<Reporter: self::Reporter>(
    script: &str,
    opts: &RunPostinstallHooks<'_>,
    stage: &str,
    env: &HashMap<String, String>,
    wd: &str,
) -> Result<ScriptExit, LifecycleScriptError> {
    let target = StreamedScript { dep_path: opts.dep_path, stage, wd, emit: Reporter::emit };
    let emit_line = |stdio, line| target.emit_line(stdio, line);
    execute_emulated(script, opts.pkg_root, env, EmulatedOutput::Lines(&emit_line))
        .map(ScriptExit::Emulated)
        .map_err(LifecycleScriptError::ShellEmulator)
}

/// Append the script body as the shell command's final argument.
///
/// On Windows the `cmd /d /s /c` path passes `windows_verbatim_args =
/// true`; the script is then appended with
/// `std::os::windows::process::CommandExt::raw_arg` so embedded quoting
/// (e.g. `node -e "..."`) reaches the child untouched, the same as
/// Node's `windowsVerbatimArguments`. The default `arg` quoting would
/// escape the inner `"` and break such commands under `cmd.exe`.
/// Everywhere else (POSIX `sh -c`, a custom `scriptShell`) the standard
/// `arg` is correct.
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

/// A script whose output is republished as `pnpm:lifecycle` events
/// rather than written straight to the terminal.
///
/// The three identity fields travel on every event the script produces:
/// `dep_path` groups them, `stage` names the script, and `wd` is what the
/// reporter renders as the project prefix.
#[derive(Clone, Copy)]
pub struct StreamedScript<'a> {
    pub dep_path: &'a str,
    pub stage: &'a str,
    pub wd: &'a str,
    pub emit: fn(&LogEvent),
}

impl StreamedScript<'_> {
    /// Announce the script that is about to run.
    pub fn started(&self, script: &str) {
        (self.emit)(&LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Script {
                dep_path: self.dep_path.to_string(),
                optional: false,
                script: script.to_string(),
                stage: self.stage.to_string(),
                wd: self.wd.to_string(),
            },
        }));
    }

    /// Announce how the script ended. `-1` stands for a child killed by a
    /// signal, which carries no exit code.
    pub fn finished(&self, exit_code: i32) {
        (self.emit)(&LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Exit {
                dep_path: self.dep_path.to_string(),
                exit_code,
                optional: false,
                stage: self.stage.to_string(),
                wd: self.wd.to_string(),
            },
        }));
    }

    /// Drain `child`'s piped stdout and stderr into one event per line,
    /// then wait for it. The pumps are joined after the wait, so every
    /// line is emitted before the caller's [`Self::finished`] — the
    /// ordering pnpm's reporter renders against.
    ///
    /// The child must have been spawned with both streams piped;
    /// whichever is absent is simply not pumped.
    pub fn pump(&self, child: &mut Child) -> io::Result<ExitStatus> {
        let stdout_handle =
            child.stdout.take().map(|stream| self.pump_stream(stream, LifecycleStdio::Stdout));
        let stderr_handle =
            child.stderr.take().map(|stream| self.pump_stream(stream, LifecycleStdio::Stderr));
        let status = child.wait();
        if let Some(handle) = stdout_handle {
            let _ = handle.join();
        }
        if let Some(handle) = stderr_handle {
            let _ = handle.join();
        }
        status
    }

    /// Asynchronously drain a tokio child's piped stdout and stderr into
    /// lifecycle events, then wait for it.
    pub async fn pump_async(&self, child: &mut tokio::process::Child) -> io::Result<ExitStatus> {
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdout_pump = async {
            if let Some(stream) = stdout {
                self.pump_async_stream(stream, LifecycleStdio::Stdout).await;
            }
        };
        let stderr_pump = async {
            if let Some(stream) = stderr {
                self.pump_async_stream(stream, LifecycleStdio::Stderr).await;
            }
        };
        let child_wait = child.wait();
        let (status, (), ()) = tokio::join!(child_wait, stdout_pump, stderr_pump);
        status
    }

    /// Spawn a thread that reads `reader` line-by-line and republishes
    /// each line.
    ///
    /// Read as bytes and decoded lossily rather than through
    /// [`BufRead::lines`], whose `Err` on non-UTF-8 would stop the drain
    /// while the child is still writing — the child then blocks on a
    /// full pipe and the caller's `wait` never returns. pnpm decodes the
    /// same output lossily.
    fn pump_stream(
        &self,
        reader: impl Read + Send + 'static,
        stdio: LifecycleStdio,
    ) -> thread::JoinHandle<()> {
        let (dep_path, stage, wd) =
            (self.dep_path.to_string(), self.stage.to_string(), self.wd.to_string());
        let emit = self.emit;
        thread::spawn(move || {
            let target = StreamedScript { dep_path: &dep_path, stage: &stage, wd: &wd, emit };
            let mut reader = BufReader::new(reader);
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line) {
                    // EOF. A final line with no newline was already
                    // emitted by the read that returned it.
                    Ok(0) => break,
                    Ok(_) => {}
                    // An EBADF or EPIPE means the child closed the
                    // stream. Not fatal — the caller's `wait` surfaces a
                    // non-zero exit code if the child failed over it.
                    Err(_) => break,
                }
                target.emit_bytes_line(stdio, &mut line);
            }
        })
    }

    async fn pump_async_stream(&self, reader: impl AsyncRead + Unpin, stdio: LifecycleStdio) {
        let mut reader = AsyncBufReader::new(reader);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line).await {
                Ok(0) => break,
                Ok(_) => self.emit_bytes_line(stdio, &mut line),
                Err(_) => break,
            }
        }
    }

    fn emit_bytes_line(&self, stdio: LifecycleStdio, line: &mut Vec<u8>) {
        if line.last() == Some(&b'\n') {
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
        }
        self.emit_line(stdio, String::from_utf8_lossy(line).into_owned());
    }

    /// Republish one line of the script's output.
    pub fn emit_line(&self, stdio: LifecycleStdio, line: String) {
        (self.emit)(&LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Stdio {
                dep_path: self.dep_path.to_string(),
                line,
                stage: self.stage.to_string(),
                stdio,
                wd: self.wd.to_string(),
            },
        }));
    }
}

#[cfg(test)]
mod tests;
