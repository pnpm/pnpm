//! Recursive `pacquet run` — run a package script across `--filter`-selected workspace projects.
//!
//! Without `--sort`, scripts execute in graph insertion order (the default). With `--sort`,
//! topological ordering is used. An empty filtered workspace is intentionally a no-op.

use super::{RunArgs, RunContext, run_stages};
use crate::cli_args::recursive::{
    AutoExcludeRoot, ExecutionStatus, Status, count_failures, discover_workspace_projects,
    get_resumed_package_chunks, select_recursive_projects, sort_filtered_projects,
    write_recursive_summary,
};
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_package_manager::{make_node_package_map_option, package_map_path_for_execution};
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    time::Instant,
};

/// Errors surfaced by a recursive run. Each variant carries a canonical pnpm error code
/// (via `#[diagnostic(code(...))]`) matching the TypeScript CLI's error-code contract.
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

/// Run `args.command` across the `--filter`-selected workspace projects.
pub fn run_recursive(args: &RunArgs, config: &Config, dir: &Path) -> miette::Result<()> {
    let Some(script_name) = args.command.as_deref() else {
        return Err(RecursiveRunError::ScriptNameRequired.into());
    };
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);

    let (projects, patterns) = discover_workspace_projects(workspace_root)?;
    let selection = select_recursive_projects(
        &projects,
        config,
        dir,
        AutoExcludeRoot::Enabled { workspace_patterns: patterns.as_deref() },
    )?;
    let graph = &selection.selected;
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

    let bail = !args.no_bail;
    let mut result: IndexMap<PathBuf, ExecutionStatus> =
        chunks.iter().flatten().map(|root| (root.clone(), ExecutionStatus::queued())).collect();
    let mut has_command = 0_usize;

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

    for chunk in &chunks {
        for root in chunk {
            let manifest = &graph[root].package.project.manifest;
            let Some(script) = manifest.script(script_name, true)? else {
                result[root].status = Status::Skipped;
                continue;
            };
            if script.is_empty() || (args.args.is_empty() && script == "npx only-allow pnpm") {
                result[root].status = Status::Skipped;
                continue;
            }
            if env::var_os("npm_lifecycle_event").is_some_and(|event| event == *script_name)
                && env::var_os("PNPM_SCRIPT_SRC_DIR")
                    .is_some_and(|src_dir| Path::new(&src_dir) == root)
            {
                continue;
            }
            if script_name.starts_with('.') && env::var_os("npm_lifecycle_event").is_none() {
                return Err(
                    super::RunError::HiddenScript { script: script_name.to_string() }.into()
                );
            }

            result[root].status = Status::Running;
            has_command += 1;
            let start = Instant::now();
            let ctx = RunContext {
                manifest,
                dir: root,
                init_cwd: &init_cwd,
                config,
                extra_env: &extra_env,
                silent: true,
                sequential: args.sequential,
            };
            let status = run_stages(&ctx, script_name, script, &args.args)?;
            let duration = start.elapsed().as_secs_f64() * 1e3;

            if status.success() {
                let entry = &mut result[root];
                entry.status = Status::Passed;
                entry.duration = Some(duration);
            } else {
                let prefix = root.to_string_lossy().into_owned();
                let entry = &mut result[root];
                entry.status = Status::Failure;
                entry.duration = Some(duration);
                entry.message =
                    Some(format!("command failed with exit code {}", status.code().unwrap_or(1)));
                entry.prefix = Some(prefix.clone());

                if bail {
                    if args.report_summary {
                        write_recursive_summary(workspace_root, &result)?;
                    }
                    return Err(RecursiveRunError::RecursiveRunFirstFail { prefix }.into());
                }
            }
        }
    }

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
