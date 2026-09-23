pub use output::StreamedScript;

use crate::{
    extend_path::extend_path,
    make_env::{EnvBuild, EnvOptions, build_env, path_value},
    process_tracker::{SpawnedChild, spawn_child},
    script_exit::ScriptExit,
    script_working_dir::{
        emulator_working_dir, is_refused_directory, script_working_dir, shorter_working_dirs,
    },
    shell::{ScriptShellError, SelectedShell, select_shell},
    shell_emulator::{EmulatedOutput, ShellEmulatorError, execute_emulated},
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_package_manifest::{PackageManifestError, safe_read_project_manifest_from_dir};
use pnpm_reporter::{LifecycleLog, LifecycleMessage, LifecycleStdio, LogEvent, LogLevel, Reporter};
use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    fs,
    io::{self, BufRead, BufReader, Read},
    path::Path,
    process::{Command, ExitStatus, Stdio},
    thread,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader as AsyncBufReader};

/// Error from running lifecycle scripts.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LifecycleScriptError {
    #[display("Failed to read package manifest at {path}: {source}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_READ_MANIFEST))]
    ReadManifest {
        path: String,
        #[error(source)]
        source: PackageManifestError,
    },

    #[display("{dep_path} {stage}: `{script}` exited with {status}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_LIFECYCLE_SCRIPT_FAILED))]
    ScriptFailed { dep_path: String, stage: String, script: String, status: ScriptExit },

    #[display("Failed to spawn lifecycle script for {dep_path} {stage} in {dir}: {source}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_SPAWN_LIFECYCLE))]
    Spawn {
        dep_path: String,
        stage: String,
        /// The directory the spawn needed: the package root the script
        /// runs in, or the temporary directory pnpm could not create.
        dir: String,
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
    /// When `false`, a per-package `node_modules/.tmp` directory is
    /// created and exposed as `TMPDIR`, and (on POSIX) lifecycle
    /// scripts run with a dropped uid/gid. Pacquet does not yet
    /// surface the privilege drop, so callers currently pass
    /// `true` everywhere.
    pub unsafe_perm: bool,
    /// Whether the dep is reachable only through optional edges
    /// (`snapshots[<key>].optional` in the v9 lockfile).
    /// Does NOT affect failure handling — `BuildModules` consults the
    /// same flag independently to decide whether to swallow a build
    /// failure (see [#397](https://github.com/pnpm/pacquet/issues/397) item 6).
    pub optional: bool,
    pub environment: crate::ScriptEnvironment<'a>,
    pub execution: crate::ScriptExecutionOptions<'a>,
}

/// The lifecycle stages pnpm runs for a *dependency* during the build
/// phase, in execution order.
const DEPENDENCY_LIFECYCLE_STAGES: [&str; 3] = ["preinstall", "install", "postinstall"];

/// The install lifecycle stages pnpm runs for each workspace *project* during
/// `pnpm deploy`, during `pnpm install` when devDependencies are excluded
/// (e.g. `--prod`), or when installing specific packages, in execution order.
pub const PROJECT_INSTALL_STAGES: [&str; 3] = ["preinstall", "install", "postinstall"];

/// The lifecycle stages pnpm runs for each workspace *project* during
/// `pnpm install`, in execution order.
pub const PROJECT_LIFECYCLE_STAGES: [&str; 6] =
    ["preinstall", "install", "postinstall", "preprepare", "prepare", "postprepare"];

/// The project stages `pnpm remove` runs before it unlinks anything.
pub const PROJECT_PRE_UNINSTALL_STAGES: [&str; 2] = ["preuninstall", "uninstall"];

/// The project stage `pnpm remove` runs after unlinking.
pub const PROJECT_POST_UNINSTALL_STAGES: [&str; 1] = ["postuninstall"];

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

/// Set by the TypeScript CLI when it delegates an install to pacquet
/// after running the root project's `preinstall` itself, so pacquet
/// runs neither its early copy ([`run_root_preinstall_hook`]) nor the
/// stage after linking. Unlike [`DEV_PREINSTALL_ALREADY_RAN_ENV`] it is
/// set on every delegation shape, because whether the TypeScript side
/// ran the hook depends on the command, not on the shape: a `pnpm add`
/// at a workspace root does not run the root's scripts there, and
/// pacquet then still owes the hook. Handled like its sibling
/// otherwise: private, and dropped from every script environment.
pub const ROOT_PREINSTALL_ALREADY_RAN_ENV: &str = "PNPM_INTERNAL_ROOT_PREINSTALL_ALREADY_RAN";

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
    run_project_lifecycle_stages::<Reporter>(opts, &PROJECT_LIFECYCLE_STAGES)
}

/// Run `stages` of a workspace project's own lifecycle scripts, in order.
///
/// Returns `true` if any script was present and executed.
pub fn run_project_lifecycle_stages<Reporter: self::Reporter>(
    opts: &RunPostinstallHooks<'_>,
    stages: &[&str],
) -> Result<bool, LifecycleScriptError> {
    run_lifecycle_stages::<Reporter>(opts, stages)
}

/// [`run_project_lifecycle_scripts`] without its `preinstall` stage, for
/// the root project, whose `preinstall` [`run_root_preinstall_hook`] ran
/// before the install began.
///
/// Returns `true` if any script was present and executed.
pub fn run_project_lifecycle_scripts_after_preinstall<Reporter: self::Reporter>(
    opts: &RunPostinstallHooks<'_>,
) -> Result<bool, LifecycleScriptError> {
    run_lifecycle_stages::<Reporter>(opts, &PROJECT_LIFECYCLE_STAGES[1..])
}

/// Run the root project's `preinstall` script, if it has one.
///
/// Like [`run_dev_preinstall_hook`] it runs before resolution, so a guard
/// such as `npx only-allow yarn` can refuse the install before any
/// dependency reaches `node_modules`.
///
/// Returns `true` when the script was present and executed.
pub fn run_root_preinstall_hook<Reporter: self::Reporter>(
    opts: &RunPostinstallHooks<'_>,
) -> Result<bool, LifecycleScriptError> {
    run_lifecycle_stages::<Reporter>(opts, &PROJECT_LIFECYCLE_STAGES[..1])
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
/// [`run_project_lifecycle_stages`], and [`run_dev_preinstall_hook`].
///
/// The `install` stage falls back to `node-gyp rebuild` when neither
/// `install` nor `preinstall` is defined and a `binding.gyp` exists.
/// The `npx only-allow pnpm` guard script is skipped — it does nothing
/// under pnpm/pacquet.
fn run_lifecycle_stages<Reporter: self::Reporter>(
    opts: &RunPostinstallHooks<'_>,
    stages: &[&str],
) -> Result<bool, LifecycleScriptError> {
    let Some(manifest) = read_lifecycle_manifest(opts.pkg_root)? else { return Ok(false) };

    let scripts = manifest.get("scripts").and_then(|v| v.as_object());
    let get_script = |name: &str| -> Option<&str> {
        scripts
            .and_then(|s| s.get(name))
            .and_then(|v| v.as_str())
    };

    // Snapshot the process env once for this package. Every stage reads
    // from this snapshot, which keeps the runs observably consistent
    // and avoids one call to `env::vars()` per stage over a
    // thread-shared global.
    let parent_env: HashMap<String, String> = env::vars().collect();

    let mut ran_any = false;

    for &stage in stages {
        let script = if stage == "install" {
            get_script("install")
                .map(String::from)
                .or_else(|| {
                    (get_script("preinstall").is_none()
                        && opts.pkg_root.join("binding.gyp").exists())
                    .then_some("node-gyp rebuild")
                    .map(String::from)
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

fn read_lifecycle_manifest(
    pkg_root: &Path,
) -> Result<Option<serde_json::Value>, LifecycleScriptError> {
    safe_read_project_manifest_from_dir(pkg_root)
        .map_err(|source| LifecycleScriptError::ReadManifest {
            path: pnpm_package_manifest::project_manifest_path(pkg_root).display().to_string(),
            source,
        })
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

    let built = lifecycle_env(stage, script, opts, manifest, parent_env);
    let path_env = prepare_lifecycle_path(opts, stage, &built)?;

    // Pick the shell up front so a misconfigured `scriptShell` fails
    // before we touch the filesystem (TMPDIR etc. already created
    // above — that's a minor leak, but the env is built before the
    // shell pick anyway). The pick also runs when the emulator will
    // take over below, because pnpm rejects a `.bat` / `.cmd`
    // `scriptShell` regardless of `shellEmulator`.
    let shell = select_shell(opts.execution.shell, cfg!(windows))
        .map_err(|source| LifecycleScriptError::ScriptShell {
            dep_path: opts.dep_path.to_string(),
            stage: stage.to_string(),
            source,
        })?;

    // Drop any inherited PATH-like key (`Path` on Windows, `PATH`
    // on POSIX) from the env map before spawning — otherwise on
    // Windows the spawn would see both that and the explicit `PATH`
    // we set below, and `Command::env` deduplicates them with an
    // unspecified winner.
    let mut child_env = built.env;
    child_env.retain(|key, _| !key.eq_ignore_ascii_case("PATH"));
    child_env.insert("PATH".to_string(), path_env.to_string_lossy().into_owned());

    let status = if opts.execution.shell_emulator {
        run_in_emulator::<Reporter>(script, opts, stage, &child_env, &pkg_root_str)?
    } else {
        run_in_shell::<Reporter>(&shell, script, opts, stage, &child_env, &pkg_root_str)?
    };

    finish_lifecycle_hook::<Reporter>(stage, script, opts, pkg_root_str, status)
}

fn finish_lifecycle_hook<Reporter: self::Reporter>(
    stage: &str,
    script: &str,
    opts: &RunPostinstallHooks<'_>,
    pkg_root_str: String,
    status: ScriptExit,
) -> Result<(), LifecycleScriptError> {
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

fn lifecycle_env(
    stage: &str,
    script: &str,
    opts: &RunPostinstallHooks<'_>,
    manifest: &Value,
    parent_env: &HashMap<String, String>,
) -> EnvBuild {
    let env_opts = EnvOptions {
        environment: opts.environment,
        stage,
        script,
        pkg_root: opts.pkg_root,

        script_src_dir: opts.pkg_root,

        unsafe_perm: opts.unsafe_perm,
    };
    build_env(&env_opts, manifest, parent_env.clone())
}

fn prepare_lifecycle_path(
    opts: &RunPostinstallHooks<'_>,
    stage: &str,
    built: &EnvBuild,
) -> Result<OsString, LifecycleScriptError> {
    if let Some(tmpdir) = &built.tmpdir {
        // `fs::create_dir_all` is idempotent for existing
        // directories (it returns `Ok(())`), so no `EEXIST` swallow is
        // needed. Treat any error here — including `AlreadyExists`,
        // which signals a *file* at that path — as a real spawn failure.
        fs::create_dir_all(tmpdir)
            .map_err(|error| LifecycleScriptError::Spawn {
                dep_path: opts.dep_path.to_string(),
                stage: stage.to_string(),
                dir: tmpdir.display().to_string(),
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
        opts.execution.wd_bin_dir,
        original_path.as_ref(),
        opts.execution.node_gyp_bin,
        opts.execution.extra_bin_paths,
        opts.execution.prepend_node_path,
        opts.environment.node_execpath,
    );

    Ok(path_env)
}

/// Start `cmd` in `pkg_root`, and retry shorter spellings when Windows
/// refuses that working directory.
///
/// The original refusal is returned when no spelling works because it
/// names the directory the install computed.
fn spawn_in_pkg_root<'tracker>(
    cmd: &mut Command,
    pkg_root: &Path,
) -> io::Result<SpawnedChild<'tracker>> {
    cmd.current_dir(pkg_root);
    let refusal = match spawn_child(cmd, None) {
        Err(error) if is_refused_directory(&error) => error,
        result => return result,
    };
    for spelling in shorter_working_dirs(pkg_root) {
        cmd.current_dir(&spelling);
        match spawn_child(cmd, None) {
            Err(error) if is_refused_directory(&error) => continue,
            result => return result,
        }
    }
    Err(refusal)
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
    let pkg_root = script_working_dir(opts.pkg_root);
    let mut cmd = Command::new(&shell.program);
    cmd.args(&shell.args);
    // Append the script body. The chain is broken here because the
    // Windows `cmd /d /s /c` path needs `raw_arg` rather than `arg`
    // (see [`push_script_arg`]) — a branch the method chain can't
    // express.
    push_script_arg(&mut cmd, script, shell.windows_verbatim_args);
    // Stripping inherited env so leftover npm_* keys from a wrapping
    // invocation cannot leak in. `build_env` already folded the
    // surviving parent keys into `built.env`.
    cmd.env_clear()
        .envs(env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = spawn_in_pkg_root(&mut cmd, pkg_root)
        .map_err(|error| LifecycleScriptError::Spawn {
            dep_path: opts.dep_path.to_string(),
            stage: stage.to_string(),
            dir: pkg_root.display().to_string(),
            source: error,
        })?;

    let stdout = child.child_mut().stdout.take();
    let stderr = child.child_mut().stderr.take();

    let target = StreamedScript { dep_path: opts.dep_path, stage, wd, emit: Reporter::emit };
    let stdout_handle = stdout.map(|stream| target.pump_stream(stream, LifecycleStdio::Stdout));
    let stderr_handle = stderr.map(|stream| target.pump_stream(stream, LifecycleStdio::Stderr));

    let status = child
        .wait()
        .map_err(|error| LifecycleScriptError::Wait {
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
    let pkg_root = emulator_working_dir(opts.pkg_root);
    let target = StreamedScript { dep_path: opts.dep_path, stage, wd, emit: Reporter::emit };
    let emit_line = |stdio, line| target.emit_line(stdio, line);
    execute_emulated(script, &pkg_root, env, EmulatedOutput::Lines(&emit_line), None)
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

#[cfg(test)]
mod tests;

mod output;
