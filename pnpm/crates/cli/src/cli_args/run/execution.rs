use super::{
    Config, ExecDirs, HashMap, IndexMap, IntoDiagnostic, Mutex, PackageManifest, Path,
    ProcessTracker, ReporterType, RunError, RunScript, ScheduleGraphOptions, ScriptExit,
    ScriptOutput, ScriptSelector, ScriptsPrependNodePath, SyncInjectedDeps, TaskCompletion, Value,
    env, exec_fallback, exit_like, make_node_package_map_option, make_node_require_option,
    package_map_path_for_execution, pnp_path_for_execution, run_script, schedule_graph,
    sync_injected_deps, throw_or_filter_hidden_scripts,
};
use crate::cli_args::concurrency_group::{
    SlotOutcome, acquire_concurrency_group_slot, with_held_group,
};
use pnpm_reporter::LogEvent;

/// Shared inputs for running a script, threaded through
/// [`run_stages`] and [`run_stage`] so neither grows an unwieldy
/// argument list. The submodule `recursive` builds a per-project
/// [`RunContext`] and reuses [`run_stages`], so the type and its
/// fields are visible up to the parent module.
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields are every input of a script spawn, bundled so the stage runners keep a short argument list."
    )
)]
pub(in super::super) struct RunContext<'a> {
    pub(in super::super) manifest: &'a PackageManifest,
    pub(in super::super) dir: &'a Path,
    pub(in super::super) init_cwd: &'a Path,
    pub(in super::super) config: &'a Config,
    pub(in super::super) extra_env: &'a HashMap<String, String>,
    pub(in super::super) silent: bool,
    pub(in super::super) output: ScriptOutput<'a>,
    pub(in super::super) process_tracker: Option<&'a ProcessTracker>,
    /// Where pnpm's own notices about the run go. Distinct from the
    /// streamed script output, which a pipeline may be capturing.
    pub(in super::super) emit: fn(&LogEvent),
}

/// Resolve `name` to a runnable main script body, or `Ok(None)` when
/// there's nothing to run (the manifest has no truthy `scripts[name]`
/// and `name` isn't `start`). An absent (or empty) `start` falls back
/// to `node server.js` provided `server.js` exists in the script
/// execution directory; otherwise [`RunError::NoScriptOrServer`].
fn resolve_main_script(ctx: &RunContext<'_>, name: &str) -> Result<Option<String>, RunError> {
    let get_script = |key: &str| -> Option<String> {
        ctx.manifest
            .value()
            .get("scripts")
            .and_then(|scripts| scripts.as_object())
            .and_then(|scripts| scripts.get(key))
            .and_then(|script| script.as_str())
            .map(str::to_string)
    };
    match get_script(name) {
        Some(body) if !body.is_empty() => Ok(Some(body)),
        _ if name == "start" => {
            if !ctx.dir.join("server.js").exists() {
                return Err(RunError::NoScriptOrServer);
            }
            Ok(Some("node server.js".to_string()))
        }
        _ => Ok(None),
    }
}

/// Run pre / main / post for `name` around an already-resolved
/// `main_body`. The contract:
///
/// - `main_body` is non-empty.
/// - `main_body` is not `"npx only-allow pnpm"` when `args` is empty
///   (otherwise the main stage's [`run_stage`] would no-op).
///
/// Both callers — single-project [`RunArgs::run`](super::RunArgs::run) and the recursive
/// runner — validate these conditions before calling: single-project
/// via [`resolve_main_script`] plus an inline npx-only-allow skip,
/// recursive via its outer per-project filter. Given that, the main
/// stage is guaranteed to actually run, so this function returns a
/// plain [`ScriptExit`] instead of `Option<ScriptExit>` and the callers
/// don't need to defensively handle a "nothing ran" case.
///
/// On the first non-success stage (pre / main / post) the function
/// short-circuits and returns that stage's status; the caller decides
/// what to do with the failure (single-project: `process::exit`;
/// recursive: record `Failure` and bail or continue). A failing stage
/// skips the remaining stages.
///
/// For `run start` with no `start` script but a `prestart`/`poststart`
/// and `enablePrePostScripts`, the hooks run around the `node server.js`
/// fallback, so the `pre`/`post` substring guard runs against the
/// resolved `main_body` here.
/// Run the matched scripts, in parallel when the run allows it. One
/// script — or a sequential run — needs no scheduler; running it inline
/// keeps its output attached to this process.
pub(super) fn run_selected_scripts(
    ctx: &RunContext<'_>,
    outcome: &ScriptOutcome<'_>,
    args: &[String],
    concurrency: usize,
) -> miette::Result<()> {
    let tasks: IndexMap<String, Vec<String>> = outcome.scripts
        .iter()
        .map(|name| (name.clone(), Vec::new()))
        .collect();
    let run_script = |name: String| run_one_script(ctx, outcome, &name, args);
    if concurrency == 1 || tasks.len() == 1 {
        for name in tasks.keys() {
            if !matches!(run_script(name.clone()), TaskCompletion::Passed) && outcome.bail {
                break;
            }
        }
        return Ok(());
    }
    let on_script_skipped = |_: &String| {};
    schedule_graph(
        &tasks,
        &ScheduleGraphOptions::new(concurrency, outcome.bail, &run_script, &on_script_skipped),
    )
    .into_diagnostic()
}

/// Resolve one script's main body (with the `start` → `node server.js`
/// fallback) and apply the args-aware `npx only-allow pnpm` no-op skip.
/// After both pass, [`run_stages`] is guaranteed to actually run the
/// main stage, so its return is a plain [`ScriptExit`].
fn run_one_script(
    ctx: &RunContext<'_>,
    outcome: &ScriptOutcome<'_>,
    name: &str,
    args: &[String],
) -> TaskCompletion {
    let main = match resolve_main_script(ctx, name) {
        Ok(Some(main)) => main,
        Ok(None) => return TaskCompletion::Passed,
        Err(error) => return outcome.abort(miette::Report::new(error)),
    };
    if args.is_empty() && main == "npx only-allow pnpm" {
        return TaskCompletion::Passed;
    }
    match run_stages(ctx, name, &main, args) {
        Ok(status) if status.success() => TaskCompletion::Passed,
        Ok(status) => outcome.fail(name, status),
        Err(error) => outcome.abort(error),
    }
}

/// What a run does when the manifest declares no matching script:
/// `--if-present` succeeds, a shorthand invocation falls back to `exec`,
/// and anything else is an error.
pub(super) fn no_matching_script(
    script_name: &str,
    args: &[String],
    dirs: ExecDirs<'_>,
    config: &Config,
    reporter: ReporterType,
    if_present: bool,
    fallback_to_exec: bool,
) -> miette::Result<()> {
    if if_present {
        return Ok(());
    }
    if fallback_to_exec {
        return exec_fallback(script_name, args, dirs, config, reporter);
    }
    Err(RunError::NoScript {
        script: script_name.to_owned(),
        hint: format!(r#"Command "{script_name}" not found."#),
    }
    .into())
}

/// The scripts the selector matches. Hidden scripts (names starting
/// with `.`) can only be invoked from within another script, detected by
/// an inherited `npm_lifecycle_event`.
pub(super) fn selected_scripts(
    manifest: &PackageManifest,
    script_name: &str,
    sequential: bool,
) -> miette::Result<Vec<String>> {
    let specified =
        ScriptSelector::new(script_name)?.select_with_start(manifest.value(), sequential);
    if env::var_os("npm_lifecycle_event").is_some() {
        return Ok(specified);
    }
    Ok(throw_or_filter_hidden_scripts(specified, script_name)?)
}

/// The environment the scripts run under, with the resolver each
/// non-default linker needs prepended to `NODE_OPTIONS`.
pub(super) fn script_extra_env(config: &Config, dir: &Path) -> HashMap<String, String> {
    let mut extra_env = config.extra_env_with_node_options();
    if let Some(pnp_path) = pnp_path_for_execution(config, dir) {
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env.insert(
            "NODE_OPTIONS".to_string(),
            make_node_require_option(&pnp_path, node_options),
        );
    }
    if let Some(package_map_path) = package_map_path_for_execution(config, dir) {
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env.insert(
            "NODE_OPTIONS".to_string(),
            make_node_package_map_option(&package_map_path, node_options),
        );
    }
    extra_env
}

/// How many of the matched scripts run at once. `--parallel` runs them
/// all, `--sequential` one at a time, and the default follows the
/// workspace concurrency setting.
pub(super) fn script_concurrency(
    config: &Config,
    script_count: usize,
    parallel: bool,
    sequential: bool,
) -> usize {
    if parallel {
        return script_count;
    }
    if sequential {
        return 1;
    }
    usize::try_from(config.workspace_concurrency).unwrap_or(usize::MAX).max(1)
}

/// Where the failures of the selected `scripts` are recorded. Under
/// `--bail` the first failure is the one the command exits with, and the
/// process tracker cancels the scripts still running beside it. Under
/// `--no-bail` every script runs and the failures are reported together
/// as [`RunError::SomeScriptsFailed`], in selection order.
pub(super) struct ScriptOutcome<'a> {
    pub(super) failures: Mutex<Vec<(String, ScriptExit)>>,
    pub(super) abort: Mutex<Option<miette::Report>>,
    pub(super) process_tracker: Option<&'a ProcessTracker>,
    pub(super) bail: bool,
    pub(super) scripts: Vec<String>,
}

impl ScriptOutcome<'_> {
    /// The command's own result: an error that stopped a script, the end
    /// of the first script that failed, or the `--no-bail` summary.
    pub(super) fn into_result(self) -> miette::Result<()> {
        if let Some(error) = self.abort.into_inner().expect("run abort lock is not poisoned") {
            return Err(error);
        }
        let mut failures = self.failures.into_inner().expect("run failure lock is not poisoned");
        if self.bail {
            if let Some((_, exit)) = failures.first() {
                // `run_stage` already emitted the `[ELIFECYCLE]` line.
                exit_like(*exit);
            }
            return Ok(());
        }
        if failures.is_empty() {
            return Ok(());
        }
        failures.sort_by_key(|(name, _)| {
            self.scripts
                .iter()
                .position(|script| script == name)
        });
        let hint = failures
            .iter()
            .map(|(name, exit)| format!("{name}: {exit}"))
            .collect::<Vec<_>>();
        Err(RunError::SomeScriptsFailed {
            failed: failures.len(),
            total: self.scripts.len(),
            hint: hint.join("\n"),
        }
        .into())
    }

    pub(super) fn fail(&self, name: &str, exit: ScriptExit) -> TaskCompletion {
        self.failures
            .lock()
            .expect("run failure lock is not poisoned")
            .push((name.to_owned(), exit));
        self.cancel_siblings();
        TaskCompletion::Failed
    }

    pub(super) fn abort(&self, error: miette::Report) -> TaskCompletion {
        let mut abort = self.abort.lock().expect("run abort lock is not poisoned");
        if abort.is_none() {
            *abort = Some(error);
        }
        self.cancel_siblings();
        TaskCompletion::Aborted
    }

    fn cancel_siblings(&self) {
        if let Some(process_tracker) = self.process_tracker {
            process_tracker.cancel();
        }
    }
}

pub(in super::super) fn run_stages(
    ctx: &RunContext<'_>,
    name: &str,
    main_body: &str,
    args: &[String],
) -> miette::Result<ScriptExit> {
    let project_env = project_extra_env(ctx);
    let ctx = &RunContext { extra_env: &project_env, ..*ctx };
    // Held across every stage, so a `pre` script cannot hand the slot
    // to another process between it and the main script.
    let cancelled = || ctx.process_tracker.is_some_and(ProcessTracker::is_cancelled);
    let slot = match acquire_concurrency_group_slot(ctx.config, name, ctx.emit, &cancelled)? {
        SlotOutcome::Ungated => return run_script_stages(ctx, name, main_body, args),
        SlotOutcome::Held(slot) => slot,
        // The callers read the tracker after a script ends, as they do for
        // a child the cancellation killed.
        SlotOutcome::Cancelled => return Ok(ScriptExit::Emulated(1)),
    };
    let held_env = with_held_group(ctx.extra_env, slot.group());
    run_script_stages(&RunContext { extra_env: &held_env, ..*ctx }, name, main_body, args)
}

fn project_extra_env(ctx: &RunContext<'_>) -> HashMap<String, String> {
    let mut env = ctx.extra_env.clone();
    ctx.config.prepend_project_node_path::<pnpm_config::Host>(
        &mut env,
        ctx.dir,
        &project_modules_dir_name(ctx),
    );
    env
}

fn project_modules_dir_name<'a>(ctx: &RunContext<'a>) -> std::borrow::Cow<'a, std::ffi::OsStr> {
    let project_name = ctx.manifest
        .value()
        .get("name")
        .and_then(Value::as_str);
    ctx.config.modules_dir_name_for(ctx.dir, project_name)
}

fn run_script_stages(
    ctx: &RunContext<'_>,
    name: &str,
    main_body: &str,
    args: &[String],
) -> miette::Result<ScriptExit> {
    let mut main_status = None;
    for (stage, script) in
        get_run_script_stages(ctx.manifest, name, main_body, ctx.config.enable_pre_post_scripts)
    {
        let is_main = stage == name;
        let Some(status) = run_stage(ctx, &stage, &script, if is_main { args } else { &[] })?
        else {
            continue;
        };
        // A failing stage stops the script, and its status is the
        // script's.
        if !status.success() {
            return Ok(status);
        }
        if is_main {
            main_status = Some(status);
        }
    }
    let main_status = main_status.expect(
        "caller validated main_body is neither empty nor the args-less `npx only-allow pnpm` no-op",
    );

    if ctx.config.sync_injected_deps_after_scripts
        .iter()
        .any(|script| script == name)
    {
        sync_injected_deps(&SyncInjectedDeps {
            pkg_name: ctx.manifest
                .value()
                .get("name")
                .and_then(Value::as_str),
            pkg_root_dir: ctx.dir,
            workspace_dir: ctx.config.workspace_dir.as_deref(),
            modules_dir_name: ctx.config.modules_dir_name(),
            workspace_modules_dir: &ctx.config.modules_dir,
            extend_node_path: ctx.config.extend_node_path,
            // Read before the script ran, so a bin it drops can still be named.
            manifest_before_scripts: Some(ctx.manifest.value()),
            ignored_directories: ctx.config.managed_directories(),
        })?;
    }

    Ok(main_status)
}

pub(in super::super) fn get_run_script_commands(
    manifest: &PackageManifest,
    name: &str,
    main_body: &str,
    enable_pre_post_scripts: bool,
) -> Vec<String> {
    get_run_script_stages(manifest, name, main_body, enable_pre_post_scripts)
        .into_iter()
        .map(|(_, script)| script)
        .collect()
}

fn get_run_script_stages(
    manifest: &PackageManifest,
    name: &str,
    main_body: &str,
    enable_pre_post_scripts: bool,
) -> Vec<(String, String)> {
    let scripts = manifest
        .value()
        .get("scripts")
        .and_then(Value::as_object);
    let mut stages = vec![(name.to_string(), main_body.to_string())];
    if !enable_pre_post_scripts {
        return stages;
    }
    let pre = format!("pre{name}");
    if let Some(script) = scripts
        .and_then(|scripts| scripts.get(&pre))
        .and_then(Value::as_str)
        .filter(|script| !script.is_empty() && !main_body.contains(&pre))
    {
        stages.insert(0, (pre, script.to_string()));
    }
    let post = format!("post{name}");
    if let Some(script) = scripts
        .and_then(|scripts| scripts.get(&post))
        .and_then(Value::as_str)
        .filter(|script| !script.is_empty() && !main_body.contains(&post))
    {
        stages.push((post, script.to_string()));
    }
    stages
}

/// Run one lifecycle stage. Returns `Ok(None)` when pnpm's per-stage
/// no-op guards apply (empty body, or `npx only-allow pnpm` with no
/// args), so the caller can record "didn't actually run" without
/// inventing a synthetic exit. A non-success [`ScriptExit`] is
/// returned to the caller — single-project `RunArgs::run` exits with
/// the code; recursive `run_recursive` records `Failure` and decides
/// whether to bail.
pub(in super::super) fn run_stage(
    ctx: &RunContext<'_>,
    stage: &str,
    script: &str,
    args: &[String],
) -> miette::Result<Option<ScriptExit>> {
    // The `npx only-allow pnpm` guard script is a no-op, so a lifecycle
    // stage whose final command is exactly that string is skipped. Args
    // are appended *before* this check, so a stage invoked with args
    // (which lengthen the command past the literal) is never skipped;
    // pre/post stages always pass `args = &[]`.
    // An empty script body is a no-op: any stage whose (post-arg) command
    // is falsy is skipped, and pre/post are gated on the body being
    // truthy, so an empty `pre<name>`/`post<name>` never runs.
    if (args.is_empty() && script == "npx only-allow pnpm") || script.is_empty() {
        return Ok(None);
    }

    let modules_bin_dir = ctx.dir
        .join(project_modules_dir_name(ctx))
        .join(".bin");
    let status = run_script(&RunScript {
        environment: super::script_environment(ctx.config, ctx.init_cwd, ctx.extra_env),
        execution: pnpm_executor::ScriptExecutionOptions {
            extra_bin_paths: &pnpm_python_installer::execution_paths(ctx.config, ctx.dir),
            node_gyp_bin: None,
            prepend_node_path: exec_scripts_prepend_node_path(ctx.config.scripts_prepend_node_path),
            shell: ctx.config.script_shell.as_deref().map(Path::new),
            shell_emulator: ctx.config.shell_emulator,
            wd_bin_dir: Some(&modules_bin_dir),
        },
        invocation: pnpm_executor::ScriptInvocation { stage, script, args },
        manifest: ctx.manifest.value(),

        pkg_root: ctx.dir,

        silent: ctx.silent,
        output: ctx.output,
        process_tracker: ctx.process_tracker,
    })
    .map_err(miette::Report::new)?;

    if !status.success() {
        if let Some(signal) = status.signal_name() {
            eprintln!("[ELIFECYCLE] Command failed with signal {signal}.");
        } else if stage == "test" {
            eprintln!("[ELIFECYCLE] Test failed. See above for more details.");
        } else if let Some(code) = status.code() {
            eprintln!("[ELIFECYCLE] Command failed with exit code {code}.");
        } else {
            eprintln!("[ELIFECYCLE] Command failed.");
        }
    }
    Ok(Some(status))
}

pub(crate) fn exec_scripts_prepend_node_path(
    value: pnpm_config::ScriptsPrependNodePath,
) -> ScriptsPrependNodePath {
    match value {
        pnpm_config::ScriptsPrependNodePath::Always => ScriptsPrependNodePath::Always,
        pnpm_config::ScriptsPrependNodePath::Never => ScriptsPrependNodePath::Never,
        pnpm_config::ScriptsPrependNodePath::WarnOnly => ScriptsPrependNodePath::WarnOnly,
    }
}
