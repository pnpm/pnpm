//! Recursive `pacquet exec` — run a command in every project of the
//! workspace, in topological order.
//!
//! Port of the recursive path of pnpm's
//! [`exec`](https://github.com/pnpm/pnpm/blob/8eb1be4988/exec/commands/src/exec.ts)
//! handler, reusing the shared graph / summary machinery in
//! [`crate::cli_args::recursive`].
//!
//! Scope versus upstream: projects are sorted topologically (upstream's
//! default) and run sequentially. `--filter` narrowing of the selected
//! set and `--workspace-concurrency` parallelism are not ported yet — the
//! selected set is every workspace project, matching the recursive `run`
//! runner.

use super::{ExecArgs, prepare_command, spawn_in_dir};
use crate::cli_args::recursive::{
    ExecutionStatus, GraphPkg, Status, count_failures, get_resumed_package_chunks, sort_projects,
    write_recursive_summary,
};
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_workspace::{
    FindWorkspaceProjectsOpts, find_workspace_projects, read_workspace_manifest,
    workspace_package_patterns,
};
use pacquet_workspace_projects_graph::{CreateProjectsGraphOptions, create_projects_graph};
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

/// Run `args.command` across every workspace project, sorted
/// topologically. `dir` is the canonicalized working directory; the
/// workspace root (and the directory the summary is written to) is
/// `config.workspace_dir`, falling back to `dir` when no
/// `pnpm-workspace.yaml` exists.
pub fn exec_recursive(args: &ExecArgs, config: &Config, dir: &Path) -> miette::Result<()> {
    let command = prepare_command(args.command.clone())?;
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);

    let patterns = read_workspace_manifest(workspace_root)
        .into_diagnostic()
        .wrap_err("reading pnpm-workspace.yaml")?
        .map(|manifest| workspace_package_patterns(&manifest));
    let projects = find_workspace_projects(workspace_root, &FindWorkspaceProjectsOpts { patterns })
        .wrap_err("finding workspace projects")?;
    // pnpm throws `RECURSIVE_EXEC_NO_PACKAGE` when the selected set is
    // empty (exec.ts:207-209).
    if projects.is_empty() {
        return Err(RecursiveExecError::NoPackage.into());
    }

    let adapters = projects.iter().map(|project| GraphPkg { project }).collect();
    let graph = create_projects_graph(adapters, &CreateProjectsGraphOptions::default()).graph;

    let mut chunks = sort_projects(&graph);
    if let Some(resume_from) = &args.resume_from {
        chunks = get_resumed_package_chunks(resume_from, chunks, &graph)?;
    }

    let bail = !args.no_bail;
    let mut result: IndexMap<PathBuf, ExecutionStatus> =
        chunks.iter().flatten().map(|root| (root.clone(), ExecutionStatus::queued())).collect();

    for chunk in &chunks {
        for root in chunk {
            result[root].status = Status::Running;
            let start = Instant::now();
            // A spawn / resolution error (e.g. command not found) is a
            // per-project failure rather than a hard error, matching
            // pnpm's per-package `try/catch` (exec.ts:296-336): the error
            // is recorded and the loop bails or continues like any other
            // non-zero result.
            let outcome = spawn_in_dir(&command, root, config, args.shell_mode);
            let duration = start.elapsed().as_secs_f64() * 1e3;

            let message = match outcome {
                Ok(status) if status.success() => None,
                Ok(status) => {
                    Some(format!("command failed with exit code {}", status.code().unwrap_or(1)))
                }
                Err(error) => Some(error.to_string()),
            };

            let prefix = root.to_string_lossy().into_owned();
            let entry = &mut result[root];
            entry.duration = Some(duration);
            match message {
                None => entry.status = Status::Passed,
                Some(message) => {
                    entry.status = Status::Failure;
                    entry.message = Some(message);
                    entry.prefix = Some(prefix.clone());
                    if bail {
                        if args.report_summary {
                            write_recursive_summary(workspace_root, &result)?;
                        }
                        return Err(RecursiveExecError::RecursiveExecFirstFail { prefix }.into());
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
