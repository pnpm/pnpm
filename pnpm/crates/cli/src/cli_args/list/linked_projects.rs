//! `list --only-projects` across projects with dedicated lockfiles.

use super::{ListArgs, TreeRequest, recursive::dedicated_project_config};
use crate::cli_args::deps_tree::{
    DependencyNode,
    build::{DependenciesHierarchy, LoadedState, importer_id_for},
    get_tree::MaxDepth,
    pkg_info::PkgInfoEnv,
};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use std::{
    collections::HashSet,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
};

type BoxedResult<'a, Output> = Pin<Box<dyn Future<Output = miette::Result<Output>> + Send + 'a>>;

/// The walk of one listed project's linked projects.
struct LinkedProjects<'a> {
    config: &'a Config,
    params: &'a [String],
    lockfile_dir: &'a Path,
    importer_ids: &'a HashSet<String>,
    depth: MaxDepth,
    ancestors: HashSet<PathBuf>,
    rewrite_link_version_dir: &'a Path,
}

impl ListArgs {
    /// Attach the project dependencies of every linked project that has a
    /// lockfile of its own. With `sharedWorkspaceLockfile: false`, the
    /// lockfile the tree was built from knows nothing about the
    /// dependencies of the other workspace projects.
    pub(super) fn expand_linked_projects<'a>(
        &'a self,
        config: &'a Config,
        env: &'a PkgInfoEnv<'_>,
        lockfile_dir: &'a Path,
        request: &'a TreeRequest<'_>,
        hierarchies: &'a mut [(PathBuf, DependenciesHierarchy)],
    ) -> BoxedResult<'a, ()> {
        let importer_ids: HashSet<String> = env.current_lockfile.importers
            .keys()
            .map(ToString::to_string)
            .collect();
        Box::pin(async move {
            for (project_dir, hierarchy) in hierarchies {
                let mut ancestors = request.linked_project_ancestors.clone();
                ancestors.insert(project_dir.clone());
                let walk = LinkedProjects {
                    config,
                    params: request.params,
                    lockfile_dir,
                    importer_ids: &importer_ids,
                    depth: request.depth,
                    ancestors,
                    rewrite_link_version_dir: project_dir,
                };
                for nodes in [
                    &mut hierarchy.dependencies,
                    &mut hierarchy.dev_dependencies,
                    &mut hierarchy.optional_dependencies,
                ] {
                    *nodes =
                        self.expand_linked_project_nodes(&walk, std::mem::take(nodes), 0).await?;
                }
            }
            Ok(())
        })
    }

    fn expand_linked_project_nodes<'a>(
        &'a self,
        walk: &'a LinkedProjects<'a>,
        nodes: Vec<DependencyNode>,
        level: u64,
    ) -> BoxedResult<'a, Vec<DependencyNode>> {
        Box::pin(async move {
            let mut expanded = Vec::with_capacity(nodes.len());
            for mut node in nodes {
                if !node.dependencies.is_empty() {
                    node.dependencies =
                        self.expand_linked_project_nodes(walk, node.dependencies, level + 1).await?;
                    expanded.push(node);
                    continue;
                }
                let path = PathBuf::from(&node.package.path);
                if node.status.circular
                    || walk.importer_ids.contains(&importer_id_for(walk.lockfile_dir, &path))
                {
                    expanded.push(node);
                    continue;
                }
                if let Some(node) = self.expand_linked_project(walk, node, &path, level).await? {
                    expanded.push(node);
                }
            }
            Ok(expanded)
        })
    }

    /// `None` when the linked directory is not a project with a lockfile.
    async fn expand_linked_project(
        &self,
        walk: &LinkedProjects<'_>,
        mut node: DependencyNode,
        project_dir: &Path,
        level: u64,
    ) -> miette::Result<Option<DependencyNode>> {
        if !has_own_lockfile(project_dir) {
            return Ok(None);
        }
        if walk.ancestors.contains(project_dir) {
            node.status.circular = true;
            return Ok(Some(node));
        }
        let Some(depth) = depth_below(walk.depth, level) else {
            return Ok(Some(node));
        };
        let request = TreeRequest {
            params: walk.params,
            depth,
            linked_project_ancestors: walk.ancestors.clone(),
        };
        node.dependencies =
            self.load_linked_project_dependencies(walk.config, project_dir, &request).await?;
        rewrite_link_versions(&mut node.dependencies, walk.rewrite_link_version_dir);
        Ok(Some(node))
    }

    async fn load_linked_project_dependencies(
        &self,
        config: &Config,
        project_dir: &Path,
        request: &TreeRequest<'_>,
    ) -> miette::Result<Vec<DependencyNode>> {
        let manifest = crate::cli_args::deps_tree::build::read_project_manifest(project_dir);
        let config = &dedicated_project_config(config, project_dir, manifest.name.as_deref());
        let state = LoadedState::load(
            project_dir,
            Some(config.modules_dir.as_path()),
            self.graph.lockfile_only,
        )?;
        let Some(env) = state.env(
            project_dir,
            config.virtual_store_dir_max_length as usize,
            &config.resolved_registries(),
            config.registry_options_by_url.clone(),
        ) else {
            return Ok(Vec::new());
        };
        let project_dirs = [project_dir.to_path_buf()];
        let hierarchies =
            self.build_hierarchies(config, &state, &env, &project_dirs, project_dir, request)
                .await?;
        Ok(hierarchies
            .into_iter()
            .flat_map(|(_, hierarchy)| {
                [
                    hierarchy.dependencies,
                    hierarchy.dev_dependencies,
                    hierarchy.optional_dependencies,
                ]
            })
            .flatten()
            .collect())
    }
}

fn has_own_lockfile(project_dir: &Path) -> bool {
    Lockfile::load_wanted_from_dir(project_dir)
        .ok()
        .flatten()
        .is_some_and(|lockfile| lockfile.importers.contains_key("."))
}

/// The depth left for the dependencies of a node at `level` (0 for a
/// direct dependency), or `None` when they are out of reach.
fn depth_below(depth: MaxDepth, level: u64) -> Option<MaxDepth> {
    match depth {
        MaxDepth::Finite(depth) if level >= depth => None,
        MaxDepth::Finite(depth) => Some(MaxDepth::Finite(depth - level - 1)),
        MaxDepth::Unlimited => Some(MaxDepth::Unlimited),
    }
}

/// Express the `link:` versions of `nodes` relative to the listed
/// project, as the tree of a shared lockfile does.
fn rewrite_link_versions(nodes: &mut [DependencyNode], rewrite_link_version_dir: &Path) {
    for node in nodes {
        if node.package.version.starts_with("link:") {
            let path = Path::new(&node.package.path);
            let relative = pathdiff::diff_paths(path, rewrite_link_version_dir)
                .unwrap_or_else(|| path.to_path_buf());
            node.package.version =
                format!("link:{}", relative.to_string_lossy().replace('\\', "/"));
        }
        rewrite_link_versions(&mut node.dependencies, rewrite_link_version_dir);
    }
}
