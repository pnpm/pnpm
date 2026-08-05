//! Recursive `pacquet run` — run a package script across the
//! `--filter`-selected workspace projects, in topological order.
//!
//! `config.filter` / `config.filter_prod` (`--filter` / `--filter-prod`,
//! include and exclude selectors) narrow the selected set via
//! [`select_recursive_projects`]; the selection is then sorted
//! topologically by default, or kept in workspace order under `--no-sort`.
//! `--parallel` starts every selected project concurrently. `--reverse`
//! and bounded `--workspace-concurrency` parallelism are not supported yet.
//! The main-dispatch auto-exclusion of the workspace root is applied via
//! [`AutoExcludeRoot::Enabled`].

use super::{RunArgs, RunContext, ScriptSelector, render_project_commands, run_stages};
use crate::cli_args::recursive::{
    AutoExcludeRoot, ExecutionStatus, GraphPkg, Status, count_failures,
    discover_workspace_projects, get_resumed_package_chunks, select_recursive_projects,
    sort_filtered_projects, write_recursive_summary,
};
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Diagnostic, IntoDiagnostic, WrapErr};
use pacquet_config::Config;
use pacquet_package_manager::{make_node_package_map_option, package_map_path_for_execution};
use pacquet_reporter::{LogEvent, LogLevel, ScopeLog};
use pacquet_workspace_projects_graph::ProjectGraph;
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    time::Instant,
};

/// Errors surfaced by a recursive run. The codes are the shared pnpm
/// error codes, so log consumers and `pnpm.io/errors` references stay
/// valid.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RecursiveRunError {
    #[display("None of the packages has a \"{script_name}\" script")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT))]
    NoScript {
        #[error(not(source))]
        script_name: String,
    },

    #[display("None of the selected packages has a \"{script_name}\" script")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT))]
    NoSelectedScript {
        #[error(not(source))]
        script_name: String,
    },

    #[display("\"pnpm recursive run\" failed in {count} packages")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_FAIL))]
    RecursiveFail {
        #[error(not(source))]
        count: usize,
    },

    #[display("\"pnpm recursive run\" failed in {prefix}")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL))]
    RecursiveRunFirstFail {
        #[error(not(source))]
        prefix: String,
    },

    #[display("You must specify the script you want to run")]
    #[diagnostic(code(ERR_PNPM_SCRIPT_NAME_IS_REQUIRED))]
    ScriptNameRequired,
}

/// Run `args.command` across the `--filter`-selected workspace projects,
/// sorted topologically. `dir` is the canonicalized working directory; the
/// workspace root (and the directory the summary is written to) is
/// `config.workspace_dir`, falling back to `dir` when no
/// `pnpm-workspace.yaml` exists.
pub fn run_recursive(
    args: &RunArgs,
    config: &Config,
    dir: &Path,
    emit: fn(&LogEvent),
    silent: bool,
) -> miette::Result<()> {
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);

    let (projects, patterns) = discover_workspace_projects(workspace_root)?;
    let selection = select_recursive_projects(
        &projects,
        config,
        dir,
        AutoExcludeRoot::Enabled { workspace_patterns: patterns.as_deref() },
    )?;
    let graph = &selection.selected;
    let Some(script_name) = args.script_name() else {
        if graph.len() != 1 {
            return Err(RecursiveRunError::ScriptNameRequired.into());
        }
        let project = graph.values().next().expect("graph contains exactly one project");
        let root_manifest = projects
            .iter()
            .find(|candidate| {
                candidate.root_dir == workspace_root
                    && candidate.root_dir != project.package.project.root_dir
            })
            .map(|project| project.manifest.value());
        println!(
            "{}",
            render_project_commands(project.package.project.manifest.value(), root_manifest),
        );
        return Ok(());
    };
    // Report what the `--filter` selection resolved to before running a
    // single script, so the user can confirm it covers what they meant.
    emit(&LogEvent::Scope(ScopeLog {
        level: LogLevel::Debug,
        selected: graph.len(),
        total: Some(projects.len()),
        workspace_prefix: config
            .workspace_dir
            .as_deref()
            .map(|dir| dir.to_string_lossy().into_owned()),
    }));
    // An empty `--filter` selection is a no-op (exit 0); an empty
    // workspace instead falls through to the no-script error below.
    if !projects.is_empty() && graph.is_empty() {
        return Ok(());
    }

    let mut chunks = if args.sort {
        sort_filtered_projects(
            graph,
            selection.full_graph(),
            selection.prod_all.as_ref(),
            &selection.prod_only_selected,
        )
    } else {
        graph.keys().cloned().map(|root| vec![root]).collect()
    };
    if let Some(resume_from) = &args.resume_from {
        chunks = get_resumed_package_chunks(resume_from, chunks, graph)?;
    }

    // Compiled once for the whole run, not per project.
    let selector = ScriptSelector::new(script_name)?;
    let bail = !args.no_bail;
    let mut result: IndexMap<PathBuf, ExecutionStatus> =
        chunks.iter().flatten().map(|root| (root.clone(), ExecutionStatus::queued())).collect();
    let mut has_command = 0_usize;

    // Lifecycle env reused per project: each recursive script sets up
    // `node_modules/.bin` on `PATH`, the `npm_*` env, the configured
    // `script_shell`, and the user-agent. Compute the bits that don't
    // vary per project once; the per-project `RunContext` reuses them.
    let init_cwd = env::current_dir().unwrap_or_else(|_| dir.to_path_buf());
    let mut extra_env: HashMap<String, String> = config.extra_env.clone();
    if let Some(node_options) = &config.node_options {
        extra_env.insert("NODE_OPTIONS".to_string(), node_options.clone());
    }
    if let Some(package_map_path) = package_map_path_for_execution(config, dir) {
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env.insert(
            "NODE_OPTIONS".to_string(),
            make_node_package_map_option(&package_map_path, node_options),
        );
    }

    if args.parallel {
        let roots = chunks.iter().flatten().collect::<Vec<_>>();
        let executions = std::thread::scope(|scope| -> miette::Result<Vec<_>> {
            let handles = roots
                .iter()
                .map(|root| {
                    std::thread::Builder::new()
                        .spawn_scoped(scope, || {
                            run_project(RunProjectOptions {
                                root,
                                graph,
                                selector: &selector,
                                args,
                                init_cwd: &init_cwd,
                                config,
                                extra_env: &extra_env,
                                bail,
                                silent,
                            })
                        })
                        .into_diagnostic()
                        .wrap_err_with(|| {
                            format!("failed to start parallel script runner for {}", root.display())
                        })
                })
                .collect::<miette::Result<Vec<_>>>()?;
            handles
                .into_iter()
                .map(|handle| {
                    handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
                .collect::<miette::Result<Vec<_>>>()
        })?;
        for (root, execution) in roots.into_iter().zip(executions) {
            has_command += execution.has_command;
            result.insert(root.clone(), execution.status);
        }
        if bail
            && let Some((root, _)) =
                result.iter().find(|(_, execution)| execution.status == Status::Failure)
        {
            if args.report_summary {
                write_recursive_summary(workspace_root, &result)?;
            }
            return Err(RecursiveRunError::RecursiveRunFirstFail {
                prefix: root.to_string_lossy().into_owned(),
            }
            .into());
        }
    } else {
        for chunk in &chunks {
            for root in chunk {
                let execution = run_project(RunProjectOptions {
                    root,
                    graph,
                    selector: &selector,
                    args,
                    init_cwd: &init_cwd,
                    config,
                    extra_env: &extra_env,
                    bail,
                    silent,
                })?;
                has_command += execution.has_command;
                let failed = execution.status.status == Status::Failure;
                result.insert(root.clone(), execution.status);
                if bail && failed {
                    if args.report_summary {
                        write_recursive_summary(workspace_root, &result)?;
                    }
                    return Err(RecursiveRunError::RecursiveRunFirstFail {
                        prefix: root.to_string_lossy().into_owned(),
                    }
                    .into());
                }
            }
        }
    }

    // `test` is exempt because `pnpm test` falls back to a default and
    // should not error on a workspace with no `test` script; otherwise a
    // recursive run that matched nothing is a user error, unless
    // `--if-present` opted out of it.
    if script_name != "test" && has_command == 0 && !args.if_present {
        let script_name = script_name.to_string();
        return Err(if graph.len() == projects.len() {
            RecursiveRunError::NoScript { script_name }
        } else {
            RecursiveRunError::NoSelectedScript { script_name }
        }
        .into());
    }

    if args.report_summary {
        write_recursive_summary(workspace_root, &result)?;
    }

    let failures = count_failures(&result);
    if failures > 0 {
        return Err(RecursiveRunError::RecursiveFail { count: failures }.into());
    }
    Ok(())
}

struct ProjectExecution {
    status: ExecutionStatus,
    has_command: usize,
}

#[derive(Clone, Copy)]
struct RunProjectOptions<'a, 'project> {
    root: &'a Path,
    graph: &'a ProjectGraph<GraphPkg<'project>>,
    selector: &'a ScriptSelector<'a>,
    args: &'a RunArgs,
    init_cwd: &'a Path,
    config: &'a Config,
    extra_env: &'a HashMap<String, String>,
    bail: bool,
    silent: bool,
}

fn run_project(options: RunProjectOptions<'_, '_>) -> miette::Result<ProjectExecution> {
    let RunProjectOptions {
        root,
        graph,
        selector,
        args,
        init_cwd,
        config,
        extra_env,
        bail,
        silent,
    } = options;
    let manifest = &graph[root].package.project.manifest;
    let specified = selector.select(manifest.value());
    if specified.is_empty() {
        let mut status = ExecutionStatus::queued();
        status.status = Status::Skipped;
        return Ok(ProjectExecution { status, has_command: 0 });
    }

    let mut execution = ProjectExecution { status: ExecutionStatus::queued(), has_command: 0 };
    let mut project_failed = false;
    for selected in &specified {
        let Some(script) = manifest.script(selected, true)? else {
            continue;
        };
        if script.is_empty() || (args.script_args().is_empty() && script == "npx only-allow pnpm") {
            continue;
        }
        if env::var_os("npm_lifecycle_event").is_some_and(|event| event == **selected)
            && env::var_os("PNPM_SCRIPT_SRC_DIR").is_some_and(|src_dir| Path::new(&src_dir) == root)
        {
            continue;
        }
        if selected.starts_with('.') && env::var_os("npm_lifecycle_event").is_none() {
            return Err(super::RunError::HiddenScript { script: selected.clone() }.into());
        }

        if !project_failed {
            execution.status.status = Status::Running;
        }
        execution.has_command += 1;
        let start = Instant::now();
        let ctx = RunContext {
            manifest,
            dir: root,
            init_cwd,
            config,
            extra_env,
            silent,
            sequential: args.sequential,
        };
        let status = run_stages(&ctx, selected, script, args.script_args())?;
        let duration = start.elapsed().as_secs_f64() * 1e3;

        if status.success() {
            if !project_failed {
                execution.status.status = Status::Passed;
                execution.status.duration = Some(duration);
            }
        } else {
            project_failed = true;
            execution.status.status = Status::Failure;
            execution.status.duration = Some(duration);
            execution.status.message =
                Some(format!("command failed with exit code {}", status.code().unwrap_or(1)));
            execution.status.prefix = Some(root.to_string_lossy().into_owned());
            if bail {
                break;
            }
        }
    }
    Ok(execution)
}
