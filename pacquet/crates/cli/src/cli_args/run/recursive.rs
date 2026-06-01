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
//! selected set, and the RegExp script selector are not ported yet — the
//! selected set is every workspace project, matching pacquet's
//! currently-unfiltered `install`.

use super::RunArgs;
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_executor::{RunScript, run_script};
use pacquet_package_manager::graph_sequencer;
use pacquet_package_manifest::DependencyGroup;
use pacquet_workspace::{
    FindWorkspaceProjectsOpts, Project, find_workspace_projects, read_workspace_manifest,
};
use pacquet_workspace_projects_graph::{
    BaseProject, CreateProjectsGraphOptions, GraphProject, ProjectGraph, create_projects_graph,
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
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
    #[display("Cannot find package {resume_from}. Could not determine where to resume from.")]
    #[diagnostic(code(ERR_PNPM_RESUME_FROM_NOT_FOUND))]
    ResumeFromNotFound {
        #[error(not(source))]
        resume_from: String,
    },

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
    // vary per project once.
    let init_cwd = env::current_dir().unwrap_or_else(|_| dir.to_path_buf());
    let mut extra_env: HashMap<String, String> = HashMap::new();
    if let Some(node_options) = &config.node_options {
        extra_env.insert("NODE_OPTIONS".to_string(), node_options.clone());
    }
    let scripts_prepend_node_path =
        super::exec_scripts_prepend_node_path(config.scripts_prepend_node_path);

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
            let status = run_script(RunScript {
                manifest: manifest.value(),
                stage: script_name,
                script,
                args: &args.args,
                pkg_root: root,
                init_cwd: &init_cwd,
                extra_bin_paths: &config.extra_bin_paths,
                script_shell: config.script_shell.as_deref().map(Path::new),
                scripts_prepend_node_path,
                node_execpath: None,
                npm_execpath: None,
                user_agent: Some("pnpm"),
                extra_env: &extra_env,
                // The per-package failure surface comes from the
                // ExecutionStatus summary, not a `$ <script>` echo.
                silent: true,
            })
            .into_diagnostic()?;
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

    throw_on_command_fail(&result)?;
    Ok(())
}

/// Sort the workspace graph into topologically ordered chunks: every
/// project in chunk `i` depends only on projects in earlier chunks, so
/// chunk `i` may run after chunks `0..i`.
///
/// Port of pnpm's
/// [`sortProjects`](https://github.com/pnpm/pnpm/blob/8eb1be4988/workspace/projects-sorter/src/index.ts):
/// build a node → in-set-dependencies map (dropping self-edges and edges
/// leaving the selected set) and run it through [`graph_sequencer`].
fn sort_projects(graph: &ProjectGraph<GraphPkg<'_>>) -> Vec<Vec<PathBuf>> {
    let keys: Vec<PathBuf> = graph.keys().cloned().collect();
    let key_set: HashSet<&PathBuf> = keys.iter().collect();
    let dependency_graph: HashMap<PathBuf, Vec<PathBuf>> = graph
        .iter()
        .map(|(root, node)| {
            let dependencies = node
                .dependencies
                .iter()
                .filter(|dependency| *dependency != root && key_set.contains(dependency))
                .cloned()
                .collect();
            (root.clone(), dependencies)
        })
        .collect();
    graph_sequencer(&dependency_graph, &keys).chunks
}

/// Drop every chunk before the one containing the `resume_from` package,
/// so execution resumes from that package.
///
/// Port of pnpm's
/// [`getResumedPackageChunks`](https://github.com/pnpm/pnpm/blob/8eb1be4988/exec/commands/src/exec.ts#L100-L118):
/// the package is located by manifest name; an unknown name is a
/// `RESUME_FROM_NOT_FOUND` error.
fn get_resumed_package_chunks(
    resume_from: &str,
    chunks: Vec<Vec<PathBuf>>,
    graph: &ProjectGraph<GraphPkg<'_>>,
) -> Result<Vec<Vec<PathBuf>>, RecursiveRunError> {
    let resume_root = graph
        .iter()
        .find(|(_, node)| node.package.manifest_name() == Some(resume_from))
        .map(|(root, _)| root.clone())
        .ok_or_else(|| RecursiveRunError::ResumeFromNotFound {
            resume_from: resume_from.to_string(),
        })?;
    let position = chunks
        .iter()
        .position(|chunk| chunk.contains(&resume_root))
        .expect("the resume-from package is present in the sorted chunks");
    Ok(chunks.into_iter().skip(position).collect())
}

/// Write the recursive summary to `pnpm-exec-summary.json` under `dir`.
///
/// Port of pnpm's
/// [`writeRecursiveSummary`](https://github.com/pnpm/pnpm/blob/8eb1be4988/exec/commands/src/exec.ts#L120-L124):
/// the per-package map is nested under an `executionStatus` key.
fn write_recursive_summary(
    dir: &Path,
    summary: &IndexMap<PathBuf, ExecutionStatus>,
) -> miette::Result<()> {
    let execution_status = summary
        .iter()
        .map(|(root, status)| (root.to_string_lossy().into_owned(), status.clone()))
        .collect();
    let path = dir.join("pnpm-exec-summary.json");
    let mut contents =
        serde_json::to_string_pretty(&ExecSummaryFile { execution_status }).into_diagnostic()?;
    contents.push('\n');
    std::fs::write(&path, contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("writing {}", path.display()))
}

/// Fail when any package's script failed.
///
/// Port of pnpm's
/// [`throwOnCommandFail`](https://github.com/pnpm/pnpm/blob/8eb1be4988/cli/utils/src/recursiveSummary.ts#L28-L33).
fn throw_on_command_fail(
    summary: &IndexMap<PathBuf, ExecutionStatus>,
) -> Result<(), RecursiveRunError> {
    let count = summary.values().filter(|status| status.status == Status::Failure).count();
    if count > 0 {
        return Err(RecursiveRunError::RecursiveFail { count });
    }
    Ok(())
}

/// Adapter that lets a [`Project`] feed [`create_projects_graph`]. Owns
/// nothing beyond a borrow of the project; the graph reads the manifest
/// name, version, and dependency groups through it.
struct GraphPkg<'a> {
    project: &'a Project,
}

impl BaseProject for GraphPkg<'_> {
    fn root_dir(&self) -> &Path {
        &self.project.root_dir
    }

    fn manifest_name(&self) -> Option<&str> {
        self.project.manifest.value().get("name").and_then(|name| name.as_str())
    }
}

impl GraphProject for GraphPkg<'_> {
    fn manifest_version(&self) -> Option<&str> {
        self.project.manifest.value().get("version").and_then(|version| version.as_str())
    }

    fn merged_dependencies(&self, ignore_dev_deps: bool) -> Vec<(String, String)> {
        // Precedence mirrors upstream's `createNode` spread: peer, then
        // dev (unless excluded), then optional, then prod, with a later
        // group overwriting an earlier duplicate's specifier while
        // keeping the first-seen position.
        let mut merged: IndexMap<String, String> = IndexMap::new();
        let mut absorb = |group: DependencyGroup| {
            for (name, spec) in self.project.manifest.dependencies([group]) {
                merged.insert(name.to_string(), spec.to_string());
            }
        };
        absorb(DependencyGroup::Peer);
        if !ignore_dev_deps {
            absorb(DependencyGroup::Dev);
        }
        absorb(DependencyGroup::Optional);
        absorb(DependencyGroup::Prod);
        merged.into_iter().collect()
    }
}

/// `pnpm-exec-summary.json` top-level shape: `{ "executionStatus": { ... } }`.
#[derive(Serialize)]
struct ExecSummaryFile {
    #[serde(rename = "executionStatus")]
    execution_status: IndexMap<String, ExecutionStatus>,
}

/// One package's entry in the recursive summary. `duration` is in
/// milliseconds and present only once the script has run; `prefix` and
/// `message` are filled in for failures.
#[derive(Debug, Clone, Serialize)]
struct ExecutionStatus {
    status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl ExecutionStatus {
    fn queued() -> Self {
        ExecutionStatus { status: Status::Queued, duration: None, prefix: None, message: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Queued,
    Running,
    Passed,
    Skipped,
    Failure,
}
