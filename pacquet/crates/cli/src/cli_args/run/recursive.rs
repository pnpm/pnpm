//! Recursive `pacquet run` — run a package script in every project of
//! the workspace, in topological order.
//!
//! Port of pnpm's
//! [`runRecursive`](https://github.com/pnpm/pnpm/blob/8eb1be4988/exec/commands/src/runRecursive.ts)
//! together with the `getResumedPackageChunks` / `writeRecursiveSummary`
//! helpers from
//! [`exec.ts`](https://github.com/pnpm/pnpm/blob/8eb1be4988/exec/commands/src/exec.ts)
//! and the `throwOnCommandFail` failure check from
//! [`@pnpm/cli.utils`](https://github.com/pnpm/pnpm/blob/8eb1be4988/cli/utils/src/recursiveSummary.ts).
//!
//! Scope versus upstream: projects are sorted topologically (upstream's
//! default) and run sequentially. `--no-sort`, `--reverse`,
//! `--workspace-concurrency` parallelism, `--filter` narrowing of the
//! selected set, and the `RegExp` script selector are not ported yet — the
//! selected set is every workspace project, matching pacquet's
//! currently-unfiltered `install`.

use super::{RunArgs, RunContext, run_stages};
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
};
use pacquet_workspace_projects_graph::{CreateProjectsGraphOptions, create_projects_graph};
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    time::Instant,
};

/// Errors surfaced by a recursive run. Codes mirror pnpm's so log
/// consumers and `pnpm.io/errors` references stay valid across the two
/// implementations.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RecursiveRunError {
    #[display("None of the packages has a \"{script_name}\" script")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT))]
    NoScript {
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

/// Run `args.command` across every workspace project, sorted
/// topologically. `dir` is the canonicalized working directory; the
/// workspace root (and the directory the summary is written to) is
/// `config.workspace_dir`, falling back to `dir` when no
/// `pnpm-workspace.yaml` exists.
pub fn run_recursive(args: &RunArgs, config: &Config, dir: &Path) -> miette::Result<()> {
    // `RunArgs::command` is optional so single-project `run` can list
    // scripts; recursive mode has no such "list" behavior, so a missing
    // script name is a usage error. Mirrors pnpm's
    // `PnpmError('SCRIPT_NAME_IS_REQUIRED', ...)` at
    // exec/commands/src/runRecursive.ts:50-52.
    let Some(script_name) = args.command.as_deref() else {
        return Err(RecursiveRunError::ScriptNameRequired.into());
    };
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);

    let patterns = read_workspace_manifest(workspace_root)
        .into_diagnostic()
        .wrap_err("reading pnpm-workspace.yaml")?
        .and_then(|manifest| manifest.packages);
    let projects = find_workspace_projects(workspace_root, &FindWorkspaceProjectsOpts { patterns })
        .wrap_err("finding workspace projects")?;

    let adapters = projects.iter().map(|project| GraphPkg { project }).collect();
    let graph = create_projects_graph(adapters, &CreateProjectsGraphOptions::default()).graph;

    let mut chunks = sort_projects(&graph);
    if let Some(resume_from) = &args.resume_from {
        chunks = get_resumed_package_chunks(resume_from, chunks, &graph)?;
    }

    let bail = !args.no_bail;
    let mut result: IndexMap<PathBuf, ExecutionStatus> =
        chunks.iter().flatten().map(|root| (root.clone(), ExecutionStatus::queued())).collect();
    let mut has_command = 0_usize;

    // Lifecycle env reused per project: pnpm runs each recursive script
    // through `runLifecycleHook` (runRecursive.ts:124-149), which sets up
    // `node_modules/.bin` on `PATH`, the `npm_*` env, the configured
    // `script_shell`, and the user-agent. Compute the bits that don't
    // vary per project once; the per-project `RunContext` reuses them.
    let init_cwd = env::current_dir().unwrap_or_else(|_| dir.to_path_buf());
    let mut extra_env: HashMap<String, String> = HashMap::new();
    if let Some(node_options) = &config.node_options {
        extra_env.insert("NODE_OPTIONS".to_string(), node_options.clone());
    }

    for chunk in &chunks {
        for root in chunk {
            let manifest = &graph[root].package.project.manifest;
            let Some(script) = manifest.script(script_name, true)? else {
                result[root].status = Status::Skipped;
                continue;
            };
            // Match pnpm's per-stage no-ops. `runRecursive.ts:107`
            // treats an empty body (`!manifest.scripts[name]`) as
            // absent → skip, and `runLifecycleHook.ts:100` skips
            // when the post-args command is exactly `npx only-allow
            // pnpm`. Without these guards the recursive loop would
            // fork a useless shell per project and (for the npm
            // guard) might run the wrong-package-manager warning.
            if script.is_empty() || (args.args.is_empty() && script == "npx only-allow pnpm") {
                result[root].status = Status::Skipped;
                continue;
            }
            // Recursion guard: pnpm's `runRecursive.ts:108-110` skips a
            // project when `npm_lifecycle_event` matches the requested
            // script AND `PNPM_SCRIPT_SRC_DIR` matches the project root
            // — i.e. this very project is already executing this very
            // script and we're now inside its child invocation. Without
            // this, a `build` script that itself calls `pacquet -r run
            // build` from within a workspace project recurses without
            // bound (every child sees the same env and walks the
            // workspace again). Status stays Queued, matching pnpm's
            // bare `return` from the per-project closure.
            if env::var_os("npm_lifecycle_event").is_some_and(|event| event == *script_name)
                && env::var_os("PNPM_SCRIPT_SRC_DIR")
                    .is_some_and(|src_dir| Path::new(&src_dir) == root)
            {
                continue;
            }
            // Hidden-script gate. Mirrors `runRecursive.ts:113-115`:
            // checked *after* the truthy-body skip above so a hidden
            // name that no project defines surfaces as
            // `ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT` rather than
            // `ERR_PNPM_HIDDEN_SCRIPT` — matching pnpm's error
            // precedence.
            if script_name.starts_with('.') && env::var_os("npm_lifecycle_event").is_none() {
                return Err(
                    super::RunError::HiddenScript { script: script_name.to_string() }.into()
                );
            }

            result[root].status = Status::Running;
            has_command += 1;
            let start = Instant::now();
            // Per-project pre/main/post via the same machinery
            // single-project `run` uses. The outer manifest /
            // empty-body / `npx only-allow pnpm` guards above
            // discharge `run_stages`' precondition (non-empty body
            // that isn't the args-less npx no-op), so the main stage
            // is guaranteed to run and `run_stages` returns a plain
            // `ExitStatus`. Mirrors pnpm's `runRecursive.ts:147,156`
            // calling `runScript` with `enablePrePostScripts`. The
            // per-package failure surface comes from the
            // `ExecutionStatus` summary, not the `$ <script>` echo.
            let ctx = RunContext {
                manifest,
                dir: root,
                init_cwd: &init_cwd,
                config,
                extra_env: &extra_env,
                silent: true,
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

    // `test` is exempt because `pnpm test` falls back to a default and
    // should not error on a workspace with no `test` script; otherwise a
    // recursive run that matched nothing is a user error, unless
    // `--if-present` opted out of it.
    if script_name != "test" && has_command == 0 && !args.if_present {
        return Err(RecursiveRunError::NoScript { script_name: script_name.to_string() }.into());
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
