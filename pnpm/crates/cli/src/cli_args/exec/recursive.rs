//! Recursive `pacquet exec` — run a command across the `--filter`-selected
//! workspace projects, in topological order.
//!
//! Reuses the shared graph / summary machinery in
//! [`crate::cli_args::recursive`].
//!
//! `config.filter` / `config.filter_prod` (`--filter` / `--filter-prod`,
//! include and exclude selectors) narrow the selected set via
//! [`select_recursive_projects`]; the selection is then sorted
//! topologically by default, or kept in workspace order under `--no-sort`,
//! reversed under `--reverse`, and run with `workspaceConcurrency` operations
//! in flight within each dependency-independent chunk.

use super::{ExecArgs, prepare_command, read_package_name, spawn_in_dir};
use crate::cli_args::recursive::{
    AutoExcludeRoot, ExecutionStatus, Status, count_failures, discover_workspace_projects,
    get_resumed_package_chunks, run_workspace_chunk, select_recursive_projects,
    sort_filtered_projects, write_recursive_summary,
};
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_executor::ScriptOutput;
use pnpm_reporter::LogEvent;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

/// Errors surfaced by a recursive exec. Codes mirror pnpm's so log
/// consumers and `pnpm.io/errors` references stay valid across the two
/// implementations.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RecursiveExecError {
    #[display("No package found in this workspace")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_EXEC_NO_PACKAGE))]
    NoPackage,

    #[display("\"pnpm recursive exec\" failed in {count} packages")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_FAIL))]
    RecursiveFail {
        #[error(not(source))]
        count: usize,
    },

    #[display("\"pnpm recursive exec\" failed in {prefix}")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL))]
    RecursiveExecFirstFail {
        #[error(not(source))]
        prefix: String,
    },
}

struct ProjectExecution {
    duration: f64,
    message: Option<String>,
}

/// Run `args.command` across the `--filter`-selected workspace projects,
/// sorted topologically. `dir` is the canonicalized working directory; the
/// workspace root (and the directory the summary is written to) is
/// `config.workspace_dir`, falling back to `dir` when no
/// `pnpm-workspace.yaml` exists.
pub fn exec_recursive(
    args: &ExecArgs,
    config: &Config,
    dir: &Path,
    emit: fn(&LogEvent),
) -> miette::Result<()> {
    let command = prepare_command(args.command.clone())?;
    // Unlike `run`'s `--stream`, `exec` prefixes its output only when
    // the user turned the hiding off explicitly — pnpm gates on
    // `reporterHidePrefix === false`, not on its falsiness.
    let show_prefix = config.reporter_hide_prefix == Some(false);
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);

    let (projects, patterns) = discover_workspace_projects(workspace_root, config)?;
    // Empty workspace errors; an empty `--filter` selection (below) is a
    // no-op — so this guard is on the discovered set, not the filtered.
    if projects.is_empty() {
        return Err(RecursiveExecError::NoPackage.into());
    }

    let selection = select_recursive_projects(
        &projects,
        config,
        dir,
        AutoExcludeRoot::Enabled { workspace_patterns: patterns.as_deref() },
    )?;
    let graph = &selection.selected;
    // An empty `--filter` selection is a no-op (exit 0).
    if graph.is_empty() {
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
        let mut roots = graph.keys().cloned().collect::<Vec<_>>();
        roots.sort_unstable();
        vec![roots]
    };
    if args.reverse {
        chunks.reverse();
    }
    if let Some(resume_from) = &args.resume_from {
        chunks = get_resumed_package_chunks(resume_from, chunks, graph)?;
    }

    let bail = !args.no_bail;
    let mut result: IndexMap<PathBuf, ExecutionStatus> =
        chunks.iter().flatten().map(|root| (root.clone(), ExecutionStatus::queued())).collect();

    for chunk in &chunks {
        let workspace_concurrency =
            if args.parallel { u32::MAX } else { config.workspace_concurrency };
        let concurrency = usize::try_from(workspace_concurrency).unwrap_or(usize::MAX).max(1);
        let batch_size = if bail { concurrency } else { chunk.len().max(1) };
        for batch in chunk.chunks(batch_size) {
            for root in batch {
                result[root].status = Status::Running;
            }
            let executions = run_workspace_chunk(batch, workspace_concurrency, |root| {
                let start = Instant::now();
                let dep_path = show_prefix.then(|| {
                    read_package_name(root).unwrap_or_else(|| {
                        pathdiff::diff_paths(root, dir)
                            .unwrap_or_else(|| root.to_path_buf())
                            .to_string_lossy()
                            .into_owned()
                    })
                });
                let output = match &dep_path {
                    Some(dep_path) => ScriptOutput::Streamed { dep_path, emit },
                    None => ScriptOutput::Inherit,
                };
                // A spawn / resolution error (e.g. command not found) is a
                // per-project failure rather than a hard error: the error is
                // recorded and the loop bails or continues like any other
                // non-zero result.
                let outcome = spawn_in_dir(&command, root, config, args.shell_mode, output);
                let duration = start.elapsed().as_secs_f64() * 1e3;

                let message = match outcome {
                    Ok(status) if status.success() => None,
                    Ok(status) => Some(format!(
                        "command failed with exit code {}",
                        status.code().unwrap_or(1),
                    )),
                    Err(error) => Some(error.to_string()),
                };
                ProjectExecution { duration, message }
            })?;
            for (root, execution) in batch.iter().zip(executions) {
                let prefix = root.to_string_lossy().into_owned();
                let entry = &mut result[root];
                entry.duration = Some(execution.duration);
                match execution.message {
                    None => entry.status = Status::Passed,
                    Some(message) => {
                        entry.status = Status::Failure;
                        entry.message = Some(message);
                        entry.prefix = Some(prefix.clone());
                        if bail {
                            if args.report_summary {
                                write_recursive_summary(workspace_root, &result)?;
                            }
                            return Err(
                                RecursiveExecError::RecursiveExecFirstFail { prefix }.into()
                            );
                        }
                    }
                }
            }
        }
    }

    if args.report_summary {
        write_recursive_summary(workspace_root, &result)?;
    }

    let failures = count_failures(&result);
    if failures > 0 {
        return Err(RecursiveExecError::RecursiveFail { count: failures }.into());
    }
    Ok(())
}
