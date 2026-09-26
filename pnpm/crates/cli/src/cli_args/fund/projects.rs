//! The installed dependency trees `pnpm fund` reports on.

use crate::cli_args::{
    deps_tree::{
        DependencyNode,
        build::{
            BuildTreeOptions, DependenciesHierarchy, LoadedState, build_dependencies_tree,
            importer_root_ids,
        },
        get_tree::MaxDepth,
        graph::{BuildGraphOptions, build_dependency_graph},
    },
    listed_projects::listed_projects,
};
use miette::IntoDiagnostic;
use pnpm_config::Config;
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_manifest::safe_read_project_manifest_from_dir;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// One listed project and its whole installed dependency tree.
pub struct ProjectDependencies {
    pub dir: PathBuf,
    pub manifest: Option<Value>,
    /// Every dependency group, in the order the tree walk expanded them,
    /// so a package's first occurrence is the one listing its dependencies.
    pub dependencies: Vec<DependencyNode>,
}

/// The dependency trees of the listed projects: the `--filter` selection
/// under `--recursive`, the project in `dir` otherwise.
pub fn load_projects(
    config: &Config,
    dir: &Path,
    recursive: bool,
) -> miette::Result<Vec<ProjectDependencies>> {
    let projects = listed_projects(config, dir, recursive)?;
    let hierarchies = if config.shares_one_lockfile() || config.workspace_dir.is_none() {
        let project_dirs: Vec<PathBuf> = projects
            .into_iter()
            .map(|(project_dir, _)| project_dir)
            .collect();
        load_hierarchies(config, config.lockfile_dir_for(dir), &project_dirs)?
    } else {
        let mut hierarchies = Vec::with_capacity(projects.len());
        for (project_dir, name) in projects {
            let mut project_config = config.clone();
            project_config.anchor_dedicated_project(&project_dir, name.as_deref());
            hierarchies.extend(load_hierarchies(
                &project_config,
                &project_dir,
                std::slice::from_ref(&project_dir),
            )?);
        }
        hierarchies
    };
    hierarchies
        .into_iter()
        .map(|(dir, hierarchy)| {
            let manifest = safe_read_project_manifest_from_dir(&dir).into_diagnostic()?;
            Ok(ProjectDependencies { dir, manifest, dependencies: merged_dependencies(hierarchy) })
        })
        .collect()
}

fn load_hierarchies(
    config: &Config,
    lockfile_dir: &Path,
    project_dirs: &[PathBuf],
) -> miette::Result<Vec<(PathBuf, DependenciesHierarchy)>> {
    let state = LoadedState::load(lockfile_dir, Some(config.modules_dir.as_path()), false)?;
    let Some(env) = state.env_for_config(lockfile_dir, config) else {
        return Ok(project_dirs
            .iter()
            .map(|project_dir| (project_dir.clone(), DependenciesHierarchy::default()))
            .collect());
    };
    let include = IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: config.optional,
    };
    let root_ids = importer_root_ids(env.current_lockfile, lockfile_dir, project_dirs);
    let graph = build_dependency_graph(
        &root_ids,
        &BuildGraphOptions {
            lockfile: env.current_lockfile,
            include,
            only_projects: false,
            peer_edges: config.peer_edge_options(),
        },
    );
    build_dependencies_tree(
        &state,
        &env,
        &graph,
        project_dirs,
        &BuildTreeOptions {
            lockfile_dir,
            depth: MaxDepth::Unlimited,
            include,
            exclude_peer_dependencies: false,
            only_projects: false,
            search: None,
            show_deduped_search_matches: false,
            modules_dir_opt: Some(config.modules_dir.as_path()),
        },
    )
}

/// The tree walk expands a project's dependencies in alias order across
/// all groups, so they are walked in that order too.
fn merged_dependencies(hierarchy: DependenciesHierarchy) -> Vec<DependencyNode> {
    let DependenciesHierarchy {
        dependencies,
        dev_dependencies,
        optional_dependencies,
        ..
    } = hierarchy;
    let mut merged: Vec<DependencyNode> = [dependencies, dev_dependencies, optional_dependencies]
        .into_iter()
        .flatten()
        .collect();
    merged.sort_by(|left, right| left.alias.cmp(&right.alias));
    merged
}
