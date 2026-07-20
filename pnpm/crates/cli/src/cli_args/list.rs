//! `pnpm list` / `ls` / `ll` / `la` — list installed packages.

use std::path::{Path, PathBuf};

use clap::Args;
use miette::IntoDiagnostic;
use pacquet_config::Config;
use pacquet_global::{ListReportAs, find_global_install_dirs, list_global_packages};
use pacquet_modules_yaml::IncludedDependencies;

use crate::cli_args::{
    deps_tree::{
        build::{BuildTreeOptions, DependenciesHierarchy, LoadedState, build_dependencies_tree},
        finders::{evaluate_finders, finder_candidates, resolve_finders},
        get_tree::MaxDepth,
        graph::{BuildGraphOptions, build_dependency_graph},
        search::Searcher,
    },
    recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
};

pub(crate) mod render;

use render::{ProjectHierarchy, RenderParseableOptions, RenderTreeOptions};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RecursionLimit {
    ProjectsOnly,
    Levels(u32),
    Unlimited,
}

fn parse_depth(text: &str) -> Result<RecursionLimit, String> {
    if text.eq_ignore_ascii_case("Infinity") || text == "-1" {
        return Ok(if text == "-1" {
            RecursionLimit::ProjectsOnly
        } else {
            RecursionLimit::Unlimited
        });
    }
    let n: u32 = text
        .parse()
        .map_err(|_| format!("expected a non-negative integer, Infinity, or -1, got `{text}`"))?;
    Ok(RecursionLimit::Levels(n))
}

impl RecursionLimit {
    fn max_depth(self) -> MaxDepth {
        match self {
            RecursionLimit::ProjectsOnly => MaxDepth::Finite(0),
            RecursionLimit::Levels(levels) => MaxDepth::Finite(u64::from(levels)),
            RecursionLimit::Unlimited => MaxDepth::Unlimited,
        }
    }
}

#[derive(Debug, Args)]
pub struct ListArgs {
    pub packages: Vec<String>,

    #[clap(short = 'g', long)]
    pub global: bool,

    /// Show extended information.
    #[clap(long)]
    pub long: bool,

    /// Show information in JSON format.
    #[clap(long)]
    pub json: bool,

    /// Show parseable output instead of tree view.
    #[clap(long)]
    pub parseable: bool,

    /// Max display depth of the dependency tree. `0` lists direct
    /// dependencies only; `-1` lists projects only.
    #[clap(long, default_value = "0", value_parser = parse_depth, allow_hyphen_values = true)]
    pub depth: RecursionLimit,

    /// Display only the dependency graph for packages in `dependencies`
    /// and `optionalDependencies`.
    #[clap(short = 'P', long = "prod")]
    pub production: bool,

    /// Display only the dependency graph for packages in `devDependencies`.
    #[clap(short = 'D', long)]
    pub dev: bool,

    /// Don't display packages from `optionalDependencies`.
    #[clap(long)]
    pub no_optional: bool,

    /// Exclude peer dependencies.
    #[clap(long)]
    pub exclude_peers: bool,

    /// Display only dependencies that are also projects within the
    /// workspace.
    #[clap(long)]
    pub only_projects: bool,

    /// List packages from the lockfile only, without checking
    /// `node_modules`.
    #[clap(long)]
    pub lockfile_only: bool,

    /// Search by a finder function declared in `.pnpmfile.cjs`.
    #[clap(long = "find-by")]
    pub find_by: Vec<String>,
}

impl ListArgs {
    pub async fn run(self, config: &Config, dir: &Path, recursive: bool) -> miette::Result<()> {
        let output = if self.global {
            self.run_global(config).await?
        } else if recursive {
            self.run_recursive(config, dir).await?
        } else {
            let lockfile_dir = local_lockfile_dir(config, dir);
            self.render_projects(config, &[dir.to_path_buf()], &self.packages, &lockfile_dir, true)
                .await?
        };
        print_output(&output);
        Ok(())
    }

    async fn run_global(&self, config: &Config) -> miette::Result<String> {
        let global_pkg_dir = config.global_pkg_dir.clone().ok_or_else(|| {
            miette::miette!(
                code = "ERR_PNPM_NO_GLOBAL_BIN_DIR",
                "Unable to find the global packages directory"
            )
        })?;

        if matches!(self.depth, RecursionLimit::Levels(n) if n > 0)
            || self.depth == RecursionLimit::Unlimited
        {
            let all_install_dirs =
                find_global_install_dirs(&global_pkg_dir, &[]).into_diagnostic()?;
            if all_install_dirs.len() == 1 {
                // Single global install: keep params so the search can
                // cover the whole tree, matching regular `pnpm ls`.
                let install_dir = all_install_dirs[0].clone();
                return self
                    .render_projects(
                        config,
                        std::slice::from_ref(&install_dir),
                        &self.packages,
                        &install_dir,
                        true,
                    )
                    .await;
            }
            // Multiple installs — try to narrow to a single one via
            // params, matching against top-level aliases of each
            // install group.
            let matching_install_dirs =
                find_global_install_dirs(&global_pkg_dir, &self.packages).into_diagnostic()?;
            if matching_install_dirs.len() > 1
                || (matching_install_dirs.is_empty() && !all_install_dirs.is_empty())
            {
                return Err(miette::miette!(
                    code = "ERR_PNPM_GLOBAL_LS_DEPTH_NOT_SUPPORTED",
                    "Cannot list a merged dependency tree across multiple global packages. \
                     Each global package is installed in an isolated directory with its own lockfile, \
                     so transitive dependencies cannot be coherently merged. \
                     Filter to a single global package by its top-level name, or omit --depth."
                ));
            }
            if let [install_dir] = matching_install_dirs.as_slice() {
                // Params served their purpose of narrowing to a single
                // install group; passing them on would activate search
                // semantics, which prune the matched package's children.
                let install_dir = install_dir.clone();
                return self
                    .render_projects(
                        config,
                        std::slice::from_ref(&install_dir),
                        &[],
                        &install_dir,
                        true,
                    )
                    .await;
            }
        }

        let report_as = self.report_as();
        list_global_packages(
            &global_pkg_dir,
            &self.packages,
            global_report_as(report_as),
            self.long,
        )
        .into_diagnostic()
    }

    async fn run_recursive(&self, config: &Config, dir: &Path) -> miette::Result<String> {
        let workspace_root = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        let (projects, _) = discover_workspace_projects(&workspace_root)?;
        let selection =
            select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;
        let project_dirs: Vec<PathBuf> = selection.selected.keys().cloned().collect();

        let always_print_root_package = self.depth == RecursionLimit::ProjectsOnly;

        if config.shared_workspace_lockfile {
            return self
                .render_projects(
                    config,
                    &project_dirs,
                    &self.packages,
                    &workspace_root,
                    always_print_root_package,
                )
                .await;
        }

        // Per-project lockfiles: each project renders independently
        // (with its own legend and summary).
        let mut outputs = Vec::new();
        for project_dir in project_dirs {
            let output = self
                .render_projects(
                    config,
                    std::slice::from_ref(&project_dir),
                    &self.packages,
                    &project_dir,
                    always_print_root_package,
                )
                .await?;
            if !output.is_empty() {
                outputs.push(output);
            }
        }
        let joiner = if self.depth == RecursionLimit::ProjectsOnly { "\n" } else { "\n\n" };
        Ok(outputs.join(joiner))
    }

    fn report_as(&self) -> ReportAs {
        if self.parseable {
            ReportAs::Parseable
        } else if self.json {
            ReportAs::Json
        } else {
            ReportAs::Tree
        }
    }

    fn include(&self) -> IncludedDependencies {
        let has_both = self.production == self.dev;
        IncludedDependencies {
            dependencies: has_both || self.production,
            dev_dependencies: has_both || self.dev,
            optional_dependencies: !self.no_optional,
        }
    }

    async fn render_projects(
        &self,
        config: &Config,
        project_dirs: &[PathBuf],
        params: &[String],
        lockfile_dir: &Path,
        always_print_root_package: bool,
    ) -> miette::Result<String> {
        let include = self.include();
        let searching = !params.is_empty() || !self.find_by.is_empty();

        let state = LoadedState::load(
            lockfile_dir,
            Some(config.modules_dir.as_path()),
            self.lockfile_only,
        )?;
        let env = state.env(lockfile_dir, config.virtual_store_dir_max_length as usize);

        let mut hierarchies: Vec<(PathBuf, DependenciesHierarchy)> = Vec::new();
        if self.depth == RecursionLimit::ProjectsOnly || env.is_none() {
            for project_dir in project_dirs {
                hierarchies.push((project_dir.clone(), DependenciesHierarchy::default()));
            }
        } else if let Some(env) = &env {
            let searcher = if searching {
                let mut searcher = Searcher::from_queries(params)?;
                if !self.find_by.is_empty() {
                    let finders = resolve_finders(config, lockfile_dir, &self.find_by).await?;
                    let graph_root_ids: Vec<_> = project_dirs
                        .iter()
                        .map(|dir| {
                            crate::cli_args::deps_tree::TreeNodeId::Importer(
                                crate::cli_args::deps_tree::build::importer_id_for(
                                    lockfile_dir,
                                    dir,
                                ),
                            )
                        })
                        .collect();
                    let graph = build_dependency_graph(
                        &graph_root_ids,
                        &BuildGraphOptions {
                            lockfile: env.current_lockfile,
                            include,
                            only_projects: self.only_projects,
                        },
                    );
                    let candidates = finder_candidates(env, &graph);
                    let results = evaluate_finders(env, &finders, candidates).await?;
                    searcher.set_finder_results(results);
                }
                Some(searcher)
            } else {
                None
            };

            hierarchies = build_dependencies_tree(
                &state,
                env,
                project_dirs,
                &BuildTreeOptions {
                    lockfile_dir,
                    depth: self.depth.max_depth(),
                    include,
                    exclude_peer_dependencies: self.exclude_peers,
                    only_projects: self.only_projects,
                    search: searcher.as_ref(),
                    show_deduped_search_matches: searcher.is_some(),
                    modules_dir_opt: Some(config.modules_dir.as_path()),
                },
            )?;
        }

        let projects: Vec<ProjectHierarchy> = hierarchies
            .into_iter()
            .map(|(project_dir, hierarchy)| {
                let manifest =
                    crate::cli_args::deps_tree::build::read_project_manifest(&project_dir);
                ProjectHierarchy {
                    name: manifest.name,
                    version: manifest.version,
                    private: manifest.private,
                    path: project_dir.to_string_lossy().into_owned(),
                    hierarchy,
                }
            })
            .collect();

        Ok(match self.report_as() {
            ReportAs::Tree => render::render_tree(
                &projects,
                &RenderTreeOptions {
                    always_print_root_package,
                    depth_above_projects_only: self.depth != RecursionLimit::ProjectsOnly,
                    long: self.long,
                    show_extraneous: false,
                    show_summary: true,
                },
            ),
            ReportAs::Parseable => render::render_parseable(
                &projects,
                &RenderParseableOptions { long: self.long, always_print_root_package },
            ),
            ReportAs::Json => render::render_json(&projects, self.long),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportAs {
    Tree,
    Json,
    Parseable,
}

fn global_report_as(report_as: ReportAs) -> ListReportAs {
    match report_as {
        ReportAs::Tree => ListReportAs::Tree,
        ReportAs::Json => ListReportAs::Json,
        ReportAs::Parseable => ListReportAs::Parseable,
    }
}

/// The directory the lockfile is read from for a non-recursive `list`:
/// the workspace root under a shared workspace lockfile, the project
/// itself otherwise.
pub(crate) fn local_lockfile_dir(config: &Config, dir: &Path) -> PathBuf {
    if config.shared_workspace_lockfile {
        config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf())
    } else {
        dir.to_path_buf()
    }
}

/// Print command output the way the TypeScript CLI does: nothing for an
/// empty string, exactly one trailing newline otherwise.
pub(crate) fn print_output(output: &str) {
    if output.is_empty() {
        return;
    }
    if output.ends_with('\n') {
        print!("{output}");
    } else {
        println!("{output}");
    }
}

#[cfg(test)]
mod tests;
