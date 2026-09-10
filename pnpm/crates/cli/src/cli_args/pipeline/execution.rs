use super::{
    CacheDisposition, Config, ExecutionStatus, GraphPkg, HashMap, Instant, IntoDiagnostic,
    LogEvent, LogLevel, Path, PathBuf, PipelineInvocation, PnpmLog, ProjectGraph, RunContext,
    RunReport, ScriptOutput, Status, SyncInjectedDeps, TaskCache, TaskNode, Value, capture,
    cargo_cache, env, make_node_package_map_option, make_node_require_option,
    package_map_path_for_execution, pnp_path_for_execution, run_stages, sync_injected_deps,
};

#[derive(Clone, Copy)]
pub(super) struct RunTaskOptions<'a, 'graph> {
    pub(super) node: &'a TaskNode,
    pub(super) graph: &'a ProjectGraph<GraphPkg<'graph>>,
    pub(super) config: &'a Config,
    pub(super) invocation: &'a PipelineInvocation,
    pub(super) cache: &'a TaskCache,
    pub(super) task_key: Option<&'a str>,
    pub(super) init_cwd: &'a Path,
    pub(super) base_extra_env: &'a HashMap<String, String>,
    pub(super) emit: fn(&LogEvent),
    pub(super) silent: bool,
    pub(super) report: &'a RunReport,
    pub(super) summary_key: &'a str,
}

/// Script failures are returned as statuses. Infrastructure errors abort the run.
pub(super) fn run_pipeline_task(
    options: &RunTaskOptions<'_, '_>,
) -> miette::Result<ExecutionStatus> {
    let root = options.node.project.as_path();
    let summary_key = options.summary_key;
    let settings = options.config.tasks.get(&options.node.task_name);
    let cache_key = options.task_key.filter(|_| task_cacheable(options.invocation, settings));
    let start = Instant::now();
    options.report.task_started(summary_key, options.task_key);

    if let Some(cache_key) = cache_key
        && let Some(restored) = try_restore(options, cache_key, start)?
    {
        return Ok(restored);
    }

    let execution = execute_task_with_cargo_cache(options, settings)?;
    let duration = start.elapsed().as_secs_f64() * 1e3;
    if execution.status == Status::Passed
        && let Some(cache_key) = cache_key
        && let Some(captured) = execution.captured
    {
        let outputs = settings.and_then(|settings| settings.outputs.as_deref()).unwrap_or_default();
        if let Err(error) = options.cache.store(cache_key, root, summary_key, outputs, captured) {
            (options.emit)(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!("{summary_key}: failed to store the task in the cache: {error}"),
                prefix: root.to_string_lossy().into_owned(),
            }));
        }
    }
    let disposition =
        if cache_key.is_some() { CacheDisposition::Miss } else { CacheDisposition::Bypass };
    options.report.task_finished(summary_key, execution.status, disposition, duration);
    Ok(ExecutionStatus {
        status: execution.status,
        duration: Some(duration),
        prefix: (execution.status == Status::Failure).then(|| root.to_string_lossy().into_owned()),
        message: execution.message,
    })
}

/// The status of a task served from the cache, or `None` when nothing is
/// stored under `cache_key` or the restore refused.
fn try_restore(
    options: &RunTaskOptions<'_, '_>,
    cache_key: &str,
    start: Instant,
) -> miette::Result<Option<ExecutionStatus>> {
    let root = options.node.project.as_path();
    let summary_key = options.summary_key;
    let Some(stored) = options.cache.lookup(cache_key) else {
        return Ok(None);
    };
    match options.cache.restore(&stored, root, summary_key) {
        Ok(()) => {
            capture::replay(&stored.scripts, root, options.emit);
            sync_injected_deps_if_configured(options.config, options.node, options.graph)?;
            let duration = start.elapsed().as_secs_f64() * 1e3;
            options.report.task_finished(
                summary_key,
                Status::Passed,
                CacheDisposition::Hit,
                duration,
            );
            (options.emit)(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Info,
                message: format!("{summary_key}: restored from cache"),
                prefix: root.to_string_lossy().into_owned(),
            }));
            Ok(Some(ExecutionStatus {
                status: Status::Passed,
                duration: Some(duration),
                prefix: None,
                message: None,
            }))
        }
        Err(reason) => {
            // A file the restore cannot account for is the user's;
            // overwriting it silently is how caches lose trust. The
            // task runs normally instead.
            (options.emit)(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!("{summary_key}: not restoring from cache: {reason}"),
                prefix: root.to_string_lossy().into_owned(),
            }));
            Ok(None)
        }
    }
}

fn execute_task_with_cargo_cache(
    options: &RunTaskOptions<'_, '_>,
    settings: Option<&pnpm_config::TaskSettings>,
) -> miette::Result<TaskExecution> {
    let root = options.node.project.as_path();
    let Some(directory) = settings.and_then(|settings| settings.cargo_target_dir.as_deref()) else {
        return execute_task_scripts(options);
    };
    let cargo = cargo_cache::CargoCache::open(root, directory).into_diagnostic()?;
    let environment = cargo_cache::cache_environment(
        options.base_extra_env,
        settings.and_then(|settings| settings.env.as_deref()).unwrap_or_default(),
    );
    let cargo_cacheable = !options.invocation.no_cache
        && settings.is_some_and(|settings| settings.cache != Some(false));
    let snapshot = cargo_cacheable.then_some(options.task_key).flatten().and_then(|task_key| {
        cargo_cache::snapshot_entry(&options.config.cache_dir, root, task_key, &environment)
            .inspect_err(|error| cargo_cache_warning(options, &error.to_string()))
            .ok()
    });
    if let Some(snapshot) = &snapshot {
        restore_cargo_snapshot(options, &cargo, snapshot)?;
    }
    let extra_env = cargo_build_env(options.base_extra_env, &cargo);
    let execution =
        execute_task_scripts(&RunTaskOptions { base_extra_env: &extra_env, ..*options })?;
    if execution.status == Status::Passed
        && let Some(task_key) = options.task_key
        && let Some(snapshot) = &snapshot
    {
        publish_cargo_snapshot(options, &cargo, snapshot, task_key, &environment);
    }
    Ok(execution)
}

/// Restore the key's snapshot into the build directory, then ready the
/// directory for this key's run.
fn restore_cargo_snapshot(
    options: &RunTaskOptions<'_, '_>,
    cargo: &cargo_cache::CargoCache,
    (entry, key, _): &(PathBuf, String, Vec<String>),
) -> miette::Result<()> {
    match cargo.restore(entry, key) {
        Ok(true) => (options.emit)(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Info,
            message: format!("{}: restored Cargo build state", options.summary_key),
            prefix: options.node.project.to_string_lossy().into_owned(),
        })),
        // A snapshot that is not there yet is the ordinary first run.
        Ok(false) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => cargo_cache_warning(options, &error.to_string()),
    }
    cargo.prepare(key).into_diagnostic()
}

/// The task's environment with Cargo pointed at the cached build directory.
fn cargo_build_env(
    base_extra_env: &HashMap<String, String>,
    cargo: &cargo_cache::CargoCache,
) -> HashMap<String, String> {
    let mut extra_env = base_extra_env.clone();
    for name in ["CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR"] {
        extra_env.insert(name.to_string(), cargo.target.to_string_lossy().into_owned());
    }
    extra_env
}

/// Save the build directory as this task key's snapshot — but only when
/// the inputs still hash to the key the run started with, since a
/// concurrent edit would otherwise be published under the wrong key.
fn publish_cargo_snapshot(
    options: &RunTaskOptions<'_, '_>,
    cargo: &cargo_cache::CargoCache,
    (entry, key, local_packages): &(PathBuf, String, Vec<String>),
    task_key: &str,
    environment: &std::collections::BTreeMap<String, String>,
) {
    let root = options.node.project.as_path();
    match cargo_cache::snapshot_entry(&options.config.cache_dir, root, task_key, environment) {
        Ok((_, after, _)) if after == *key => {
            if let Err(error) = cargo.publish(entry, key, local_packages) {
                cargo_cache_warning(options, &error.to_string());
            }
        }
        Ok(_) => {
            cargo_cache_warning(options, "inputs changed during execution; snapshot was not saved");
        }
        Err(error) => cargo_cache_warning(options, &error.to_string()),
    }
}

fn cargo_cache_warning(options: &RunTaskOptions<'_, '_>, reason: &str) {
    (options.emit)(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: format!("{}: Cargo build cache: {reason}", options.summary_key),
        prefix: options.node.project.to_string_lossy().into_owned(),
    }));
}

struct TaskExecution {
    status: Status,
    message: Option<String>,
    captured: Option<Vec<capture::CapturedScript>>,
}

/// Run the task's scripts for real, capturing their output stream for
/// the cache alongside the live reporter rendering.
fn execute_task_scripts(options: &RunTaskOptions<'_, '_>) -> miette::Result<TaskExecution> {
    let root = options.node.project.as_path();
    let manifest = &options.graph[root].package.project.manifest;

    let extra_env = task_environment(options.config, root, options.base_extra_env);
    let capture_output = options.task_key.is_some()
        && task_cacheable(options.invocation, options.config.tasks.get(&options.node.task_name));
    let root_str = root.to_string_lossy().into_owned();
    let mut execution = TaskExecution {
        status: Status::Passed,
        message: None,
        captured: capture_output.then(Vec::new),
    };
    let mut captured_bytes = 0usize;
    let ctx = pipeline_script_context(options, &extra_env, &root_str, capture_output);
    for selected in &options.node.scripts {
        let Some(script) = runnable_script(manifest, selected, root)? else {
            continue;
        };
        let exit = run_stages(&ctx, selected, &script, &[]);
        if capture_output {
            drain_captured_output(
                &mut execution.captured,
                &mut captured_bytes,
                &root_str,
                selected,
                options.config.enable_pre_post_scripts,
            );
        }
        let exit = exit?;
        if !exit.success() {
            execution.status = Status::Failure;
            execution.message =
                Some(format!("command failed with exit code {}", exit.code().unwrap_or(1)));
            break;
        }
    }
    Ok(execution)
}

/// Take the output one script produced into the capture buffer. A task
/// whose output outgrows the cap is not cached at all: a truncated log
/// would replay as a complete one.
fn drain_captured_output(
    captured: &mut Option<Vec<capture::CapturedScript>>,
    captured_bytes: &mut usize,
    root_str: &str,
    selected: &str,
    enable_pre_post_scripts: bool,
) {
    let Some(stages) = capture::drain_task(root_str, selected, enable_pre_post_scripts) else {
        *captured = None;
        return;
    };
    *captured_bytes += stages
        .iter()
        .flat_map(|stage| &stage.lines)
        .map(|line| line.line.len() + std::mem::size_of::<capture::CapturedLine>())
        .sum::<usize>();
    if *captured_bytes > capture::MAX_CAPTURE_BYTES {
        *captured = None;
    }
    if let Some(captured) = captured.as_mut() {
        captured.extend(stages);
    }
}

/// The script body to run for one selected script name. `None` when the
/// project declares none, when it is empty or the `only-allow` guard, or
/// when running it would re-enter the script pnpm is already inside.
fn runnable_script(
    manifest: &pnpm_package_manifest::PackageManifest,
    selected: &str,
    root: &Path,
) -> miette::Result<Option<String>> {
    let Some(script) = manifest.script(selected, true).map_err(miette::Report::new)? else {
        return Ok(None);
    };
    if script.is_empty() || script == "npx only-allow pnpm" {
        return Ok(None);
    }
    if env::var_os("npm_lifecycle_event").is_some_and(|event| event == *selected)
        && env::var_os("PNPM_SCRIPT_SRC_DIR").is_some_and(|src_dir| Path::new(&src_dir) == root)
    {
        return Ok(None);
    }
    Ok(Some(script.to_owned()))
}

/// A cache hit skips the script but must not skip the injected-deps sync
/// that would have followed it — consumers of an injected dependency
/// would otherwise keep seeing the previous build.
fn sync_injected_deps_if_configured(
    config: &Config,
    node: &TaskNode,
    graph: &ProjectGraph<GraphPkg<'_>>,
) -> miette::Result<()> {
    if !config.sync_injected_deps_after_scripts.iter().any(|script| node.scripts.contains(script)) {
        return Ok(());
    }
    let manifest = graph[node.project.as_path()].package.project.manifest.value();
    sync_injected_deps(&SyncInjectedDeps {
        pkg_name: manifest.get("name").and_then(Value::as_str),
        pkg_root_dir: &node.project,
        workspace_dir: config.workspace_dir.as_deref(),
        manifest_before_scripts: Some(manifest),
    })?;
    Ok(())
}

fn task_cacheable(
    invocation: &PipelineInvocation,
    settings: Option<&pnpm_config::TaskSettings>,
) -> bool {
    !invocation.no_cache
        && settings.is_some_and(|settings| {
            settings.outputs.is_some()
                && settings.cache != Some(false)
                && settings.cargo_target_dir.is_none()
        })
}

pub(super) fn task_environment(
    config: &Config,
    root: &Path,
    base_extra_env: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut extra_env = base_extra_env.clone();
    if let Some(pnp_path) = pnp_path_for_execution(config, root) {
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env
            .insert("NODE_OPTIONS".to_string(), make_node_require_option(&pnp_path, node_options));
    }
    if let Some(package_map_path) = package_map_path_for_execution(config, root) {
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env.insert(
            "NODE_OPTIONS".to_string(),
            make_node_package_map_option(&package_map_path, node_options),
        );
    }

    extra_env
}

fn pipeline_script_context<'a>(
    options: &'a RunTaskOptions<'_, '_>,
    extra_env: &'a HashMap<String, String>,
    root_str: &'a str,
    capture_output: bool,
) -> RunContext<'a> {
    let root = options.node.project.as_path();
    RunContext {
        manifest: &options.graph[root].package.project.manifest,
        dir: root,
        init_cwd: options.init_cwd,
        config: options.config,
        extra_env,
        silent: options.silent,
        output: ScriptOutput::Streamed {
            dep_path: root_str,
            emit: if capture_output { capture::capturing_emit } else { options.emit },
        },
        // The pipeline never bails, so there is no cancellation to
        // propagate into running children.
        process_tracker: None,
    }
}
