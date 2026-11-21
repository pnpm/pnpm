//! `pnpm list` / `ls` / `ll` / `la`: list installed packages.

pub(in crate::cli_args) mod global;
pub(in crate::cli_args) mod recursive;
pub(crate) mod render;

pub(in crate::cli_args) use global::global_install_dir_config;

mod linked_projects;

use crate::cli_args::{
    deps_tree::{
        build::{
            BuildTreeOptions,
            DependenciesHierarchy,
            LoadedState,
            build_dependencies_tree,
            importer_root_ids,
        },
        get_tree::MaxDepth,
        graph::{
            BuildGraphOptions,
            build_dependency_graph,
        },
        search::Searcher,
    },
    deps_tree_finders::{
        evaluate_finders,
        finder_candidates,
        resolve_finders,
    },
    install::resolve_bool_override,
};
use clap::Args;
use linked_projects::SharedLinkedProjects;
use pnpm_config::Config;
use pnpm_modules_yaml::IncludedDependencies;
use render::{
    ProjectHierarchy,
    RenderParseableOptions,
    RenderTreeOptions,
};
use std::{
    collections::{
        HashMap,
        HashSet,
    },
    path::{
        Path,
        PathBuf,
    },
    sync::{
        Arc,
        OnceLock,
    },
};

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
    /// Exclude peer dependencies.
    #[clap(long)]
    pub exclude_peers: bool,
    /// Search by a finder function declared in `.pnpmfile.cjs`.
    #[clap(long = "find-by")]
    pub find_by: Vec<String>,
    #[clap(flatten)]
    pub output: TreeOutputArgs,
    #[clap(flatten)]
    pub dependencies: TreeDependencyArgs,
    #[clap(flatten)]
    pub graph: ListGraphArgs,
    /// Discovered once per command: a recursive listing with dedicated
    /// lockfiles expands the linked projects of every selected project.
    #[clap(skip)]
    linked_project_dirs: OnceLock<Arc<HashMap<PathBuf, PathBuf>>>,
}

#[derive(Debug, Clone, clap::Args)]
pub struct TreeOutputArgs {
    /// Show extended information.
    #[clap(long)]
    pub long: bool,
    /// Show information in JSON format.
    #[clap(long)]
    pub json: bool,
    /// Show parseable output instead of tree view.
    #[clap(long)]
    pub parseable: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct TreeDependencyArgs {
    /// Display only the dependency graph for packages in `dependencies`
    /// and `optionalDependencies`.
    #[clap(short = 'P', long = "prod", visible_alias = "production")]
    pub production: bool,
    /// Display only the dependency graph for packages in `devDependencies`.
    #[clap(short = 'D', long)]
    pub dev: bool,
    /// Don't display packages from `optionalDependencies`.
    #[clap(long, overrides_with = "optional")]
    pub no_optional: bool,
    /// Include packages from `optionalDependencies`.
    #[clap(long, overrides_with = "no_optional")]
    pub optional: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct ListGraphArgs {
    /// Max display depth of the dependency tree. `0` lists direct
    /// dependencies only; `-1` lists projects only.
    #[clap(long, default_value = "0", value_parser = parse_depth, allow_hyphen_values = true)]
    pub(crate) depth: RecursionLimit,
    /// Display only dependencies that are also projects within the
    /// workspace.
    #[clap(long)]
    pub only_projects: bool,
    /// List packages from the lockfile only, without checking
    /// `node_modules`.
    #[clap(long)]
    pub lockfile_only: bool,
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

    pub(super) fn report_as(&self) -> ReportAs {
        if self.output.parseable {
            ReportAs::Parseable
        } else if self.output.json {
            ReportAs::Json
        } else {
            ReportAs::Tree
        }
    }

    fn include(&self, include_optional: bool) -> IncludedDependencies {
        let has_both = self.dependencies.production == self.dependencies.dev;
        IncludedDependencies {
            dependencies: has_both || self.dependencies.production,
            dev_dependencies: has_both || self.dependencies.dev,
            optional_dependencies: resolve_bool_override(
                self.dependencies.optional,
                self.dependencies.no_optional,
                include_optional,
            ),
        }
    }

    pub(super) async fn render_projects(
        &self,
        config: &Config,
        project_dirs: &[PathBuf],
        params: &[String],
        lockfile_dir: &Path,
        always_print_root_package: bool,
    ) -> miette::Result<String> {
        let projects =
            self.load_project_hierarchies(config, project_dirs, params, lockfile_dir).await?;
        self.render_project_hierarchies(&projects, always_print_root_package)
    }

    async fn load_project_hierarchies(
        &self,
        config: &Config,
        project_dirs: &[PathBuf],
        params: &[String],
        lockfile_dir: &Path,
    ) -> miette::Result<Vec<ProjectHierarchy>> {
        let state = LoadedState::load(
            lockfile_dir,
            Some(config.modules_dir.as_path()),
            self.graph.lockfile_only,
        )?;
        let env = state.env_for_config(lockfile_dir, config);

        let hierarchies = match env
            .as_ref()
            .filter(|_| self.graph.depth != RecursionLimit::ProjectsOnly)
        {
            Some(env) => {
                let request = TreeRequest::new(params, self.graph.depth.max_depth());
                self.build_hierarchies(config, &state, env, project_dirs, lockfile_dir, &request)
                    .await?
            }
            // Without a materialized `node_modules` there is no tree to
            // walk; every project reports its own line and nothing under
            // it.
            None => project_dirs
                .iter()
                .map(|project_dir| (project_dir.clone(), DependenciesHierarchy::default()))
                .collect(),
        };

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

        Ok(projects)
    }
    fn render_project_hierarchies(
        &self,
        projects: &[ProjectHierarchy],
        always_print_root_package: bool,
    ) -> miette::Result<String> {
        Ok(match self.report_as() {
            ReportAs::Tree => render::render_tree(
                projects,
                &RenderTreeOptions {
                    always_print_root_package,
                    depth_above_projects_only: self.graph.depth != RecursionLimit::ProjectsOnly,
                    long: self.output.long,
                    show_extraneous: false,
                    show_summary: true,
                },
            ),
            ReportAs::Parseable => render::render_parseable(
                projects,
                &RenderParseableOptions { long: self.output.long, always_print_root_package },
            ),
            ReportAs::Json => render::render_json(projects, self.output.long),
        })
    }

    /// Walk the dependency graph of every listed project, applying the
    /// search queries and `--find-by` finders when the command has any.
    async fn build_hierarchies(
        &self,
        config: &Config,
        state: &LoadedState,
        env: &pnpm_deps_inspection::pkg_info::PkgInfoEnv<'_>,
        project_dirs: &[PathBuf],
        lockfile_dir: &Path,
        request: &TreeRequest<'_>,
    ) -> miette::Result<Vec<(PathBuf, DependenciesHierarchy)>> {
        let params = request.params;
        let include = self.include(config.optional);
        let root_ids = importer_root_ids(env.current_lockfile, lockfile_dir, project_dirs);
        let graph = build_dependency_graph(
            &root_ids,
            &BuildGraphOptions {
                lockfile: env.current_lockfile,
                include,
                only_projects: self.graph.only_projects,
                peer_edges: config.peer_edge_options(),
                skip_peer_edges: false,
            },
        );
        let searcher = self.build_searcher(config, env, &graph, lockfile_dir, params).await?;
        let mut hierarchies = build_dependencies_tree(
            state,
            env,
            &graph,
            project_dirs,
            &BuildTreeOptions {
                lockfile_dir,
                depth: request.depth,
                include,
                exclude_peer_dependencies: self.exclude_peers,
                only_projects: self.graph.only_projects,
                search: searcher.as_ref(),
                show_deduped_search_matches: searcher.is_some(),
                unsaved_modules_dir_name: Some(Path::new(config.modules_dir_name())),
            },
        )?;
        if self.graph.only_projects {
            self.expand_linked_projects(config, env, lockfile_dir, request, &mut hierarchies)
                .await?;
        }
        Ok(hierarchies)
    }

    /// The searcher the tree walk filters through. `None` when the
    /// command named no query and no finder.
    async fn build_searcher(
        &self,
        config: &Config,
        env: &pnpm_deps_inspection::pkg_info::PkgInfoEnv<'_>,
        graph: &pnpm_deps_inspection::graph::DependencyGraph,
        lockfile_dir: &Path,
        params: &[String],
    ) -> miette::Result<Option<Searcher>> {
        if params.is_empty() && self.find_by.is_empty() {
            return Ok(None);
        }
        let mut searcher = Searcher::from_queries(params)?;
        if !self.find_by.is_empty() {
            let finder_dir = if self.global {
                config.global_pkg_dir.as_deref().unwrap_or(lockfile_dir)
            } else {
                lockfile_dir
            };
            let finders = resolve_finders(config, finder_dir, &self.find_by).await?;
            let candidates = finder_candidates(env, graph);
            let results = evaluate_finders(env, &finders, candidates).await?;
            searcher.set_finder_results(results);
        }
        Ok(Some(searcher))
    }
}

struct TreeRequest<'a> {
    params: &'a [String],
    depth: MaxDepth,
    /// Linked projects whose trees enclose this walk, to stop at cycles
    /// between projects with dedicated lockfiles.
    linked_project_ancestors: HashSet<PathBuf>,
    linked_projects: Option<Arc<SharedLinkedProjects>>,
}

impl<'a> TreeRequest<'a> {
    fn new(params: &'a [String], depth: MaxDepth) -> Self {
        TreeRequest {
            params,
            depth,
            linked_project_ancestors: HashSet::new(),
            linked_projects: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReportAs {
    Tree,
    Json,
    Parseable,
}

/// The directory the lockfile is read from for a non-recursive `list`.
pub(crate) fn local_lockfile_dir(config: &Config, dir: &Path) -> PathBuf {
    config.lockfile_dir_for(dir).to_path_buf()
}

/// Print command output the way the TypeScript CLI does: nothing for an
/// empty string, exactly one trailing newline otherwise.
pub(crate) fn print_output(output: &str) {
    if output.is_empty() {
        return;
    }
    if output.ends_with('\n') {
        pnpm_diagnostics::print!("{output}");
    } else {
        pnpm_diagnostics::println!("{output}");
    }
}

#[cfg(test)]
mod tests;
