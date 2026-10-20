//! `pnpm why` — show the packages that depend on `<pkg>`.

use crate::{
    State,
    cli_args::{
        deps_tree::{
            build::{LoadedState, importer_root_ids, read_project_manifest, safe_importer_dir},
            dependents::{
                BuildDependentsOptions, DependentsTree, ImporterInfo, build_dependents_tree,
                merge_trees,
            },
            dependents_render::{
                RenderDependentsOptions, render_dependents_json, render_dependents_parseable,
                render_dependents_tree,
            },
            graph::{BuildGraphOptions, DependencyGraph, build_dependency_graph},
            pkg_info::PkgInfoEnv,
            search::Searcher,
        },
        deps_tree_finders::{FinderHandle, evaluate_finders, finder_candidates, resolve_finders},
        install::resolve_bool_override,
        list::{print_output, recursive::dedicated_project_config},
        parse_depth_limit,
        recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
    },
};
use clap::Args;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_workspace_projects_graph::BaseProject;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Args)]
pub struct WhyArgs {
    pub packages: Vec<String>,
    /// Max display depth of the reverse dependency tree. `Infinity`
    /// renders the whole tree.
    #[clap(long, value_parser = parse_depth_limit)]
    pub depth: Option<usize>,
    /// Exclude peer dependencies. Ignored: `pnpm why` always includes them.
    // The flag is there for parity: pnpm 11's `why` declares it and never
    // forwards it either.
    #[clap(long)]
    pub exclude_peers: bool,
    /// Search by a finder function declared in `.pnpmfile.cjs`.
    #[clap(long = "find-by")]
    pub find_by: Vec<String>,
    #[clap(flatten)]
    pub output: crate::cli_args::list::TreeOutputArgs,
    #[clap(flatten)]
    pub dependencies: crate::cli_args::list::TreeDependencyArgs,
}

impl WhyArgs {
    pub async fn run(self, state: State) -> miette::Result<()> {
        if self.packages.is_empty() && self.find_by.is_empty() {
            return Err(miette::miette!(
                code = "ERR_PNPM_MISSING_PACKAGE_NAME",
                "`pnpm why` requires the package name or --find-by=<finder-name>"
            ));
        }
        let finders = resolve_finders(state.config, state.lockfile_dir(), &self.find_by).await?;

        let trees = if state.config.recursive && !state.config.shares_one_lockfile() {
            let Some(trees) = self.trees_for_dedicated_lockfiles(&state, &finders).await? else {
                return Ok(());
            };
            trees
        } else {
            let lockfile_dir = state.lockfile_dir().to_path_buf();
            let project_dirs = state_project_dirs(&state, &lockfile_dir)?;
            let Some(trees) =
                self.trees_for_lockfile(state.config, &lockfile_dir, &project_dirs, &finders)
                    .await?
            else {
                return Ok(());
            };
            trees
        };

        print_output(&self.render(&trees));
        Ok(())
    }

    async fn trees_for_dedicated_lockfiles(
        &self,
        state: &State,
        finders: &[FinderHandle],
    ) -> miette::Result<Option<Vec<DependentsTree>>> {
        let project_dir = state.project_dir();
        let workspace_root = state.config.workspace_dir.as_deref().unwrap_or(project_dir);
        let (projects, _) = discover_workspace_projects(workspace_root, state.config)?;
        let selection = select_recursive_projects(
            &projects,
            state.config,
            project_dir,
            AutoExcludeRoot::Disabled,
        )?;
        let mut any_lockfile = false;
        let mut trees = Vec::new();
        for (selected_dir, project) in &selection.selected {
            let project_config = dedicated_project_config(
                state.config,
                selected_dir,
                project.package.manifest_name(),
            );
            if let Some(project_trees) = self.trees_for_lockfile(
                &project_config,
                selected_dir,
                std::slice::from_ref(selected_dir),
                finders,
            )
            .await?
            {
                any_lockfile = true;
                trees.extend(project_trees);
            }
        }
        if !any_lockfile {
            return Ok(None);
        }
        Ok(Some(merge_trees(trees)))
    }

    async fn trees_for_lockfile(
        &self,
        config: &Config,
        lockfile_dir: &Path,
        project_dirs: &[PathBuf],
        finders: &[FinderHandle],
    ) -> miette::Result<Option<Vec<DependentsTree>>> {
        let loaded = LoadedState::load(lockfile_dir, Some(config.modules_dir.as_path()), false)?;
        let Some(env) = loaded.env_for_config(lockfile_dir, config) else {
            return Ok(None);
        };
        let lockfile = env.current_lockfile;
        let importer_info = collect_importer_info(lockfile, lockfile_dir);
        let include = self.included_dependencies(config);
        let root_ids = importer_root_ids(lockfile, lockfile_dir, project_dirs);
        let graph = build_dependency_graph(
            &root_ids,
            &BuildGraphOptions {
                lockfile,
                include,
                only_projects: false,
                peer_edges: config.peer_edge_options(),
            },
        );
        let searcher = self.searcher(&env, &graph, finders).await?;
        Ok(Some(build_dependents_tree(&BuildDependentsOptions {
            env: &env,
            graph: &graph,
            search: &searcher,
            importer_info: &importer_info,
            manifest_fields: &[],
        })))
    }

    fn included_dependencies(&self, config: &Config) -> IncludedDependencies {
        let has_both = self.dependencies.production == self.dependencies.dev;
        IncludedDependencies {
            dependencies: has_both || self.dependencies.production,
            dev_dependencies: has_both || self.dependencies.dev,
            optional_dependencies: resolve_bool_override(
                self.dependencies.optional,
                self.dependencies.no_optional,
                config.optional,
            ),
        }
    }

    /// The package queries, with the `--find-by` finders' matches folded in.
    async fn searcher(
        &self,
        env: &PkgInfoEnv<'_>,
        graph: &DependencyGraph,
        finders: &[FinderHandle],
    ) -> miette::Result<Searcher> {
        let mut searcher = Searcher::from_queries(&self.packages)?;
        if !finders.is_empty() {
            let candidates = finder_candidates(env, graph);
            let results = evaluate_finders(env, finders, candidates).await?;
            searcher.set_finder_results(results);
        }
        Ok(searcher)
    }

    fn render(&self, trees: &[DependentsTree]) -> String {
        let render_opts = RenderDependentsOptions { long: self.output.long, depth: self.depth };
        if self.output.parseable {
            render_dependents_parseable(trees, &render_opts)
        } else if self.output.json {
            render_dependents_json(trees, &render_opts)
        } else {
            render_dependents_tree(trees, &render_opts)
        }
    }
}

/// The name and version to show for each importer of the lockfile.
fn collect_importer_info(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
) -> HashMap<String, ImporterInfo> {
    let mut importer_info = HashMap::new();
    for importer_id in lockfile.importers.keys() {
        // A key that cannot be safely joined (a malformed or hostile
        // lockfile) is never dereferenced; the raw key still names the
        // importer in the output.
        let manifest = safe_importer_dir(lockfile_dir, importer_id)
            .map(|importer_dir| read_project_manifest(&importer_dir))
            .unwrap_or_default();
        let name = manifest.name.unwrap_or_else(|| importer_display_name(importer_id));
        importer_info.insert(
            importer_id.clone(),
            ImporterInfo { name, version: manifest.version.unwrap_or_default() },
        );
    }
    importer_info
}

/// What to call an importer whose manifest carries no name.
fn importer_display_name(importer_id: &str) -> String {
    if importer_id == "." { "the root project".to_string() } else { importer_id.to_string() }
}

/// The projects `pnpm why` walks: the selected workspace projects under
/// `--recursive`, otherwise the project the command ran in.
fn why_project_dirs(
    config: &Config,
    lockfile_dir: &Path,
    project_dir: PathBuf,
) -> miette::Result<Vec<PathBuf>> {
    if !config.recursive {
        return Ok(vec![project_dir]);
    }
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(lockfile_dir);
    let (projects, _) = discover_workspace_projects(workspace_root, config)?;
    Ok(select_recursive_projects(&projects, config, &project_dir, AutoExcludeRoot::Disabled)?
        .selected
        .keys()
        .cloned()
        .collect())
}

fn state_project_dirs(state: &State, lockfile_dir: &Path) -> miette::Result<Vec<PathBuf>> {
    why_project_dirs(state.config, lockfile_dir, state.project_dir().to_path_buf())
}
