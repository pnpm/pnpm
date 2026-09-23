//! `list --only-projects` across projects with dedicated lockfiles.

use crate::cli_args::{
    deps_tree::{
        DependencyNode,
        build::{DependenciesHierarchy, LoadedState, importer_id_for},
        get_tree::MaxDepth,
        pkg_info::PkgInfoEnv,
    },
    list::{ListArgs, TreeRequest, recursive::dedicated_project_config},
    recursive::discover_workspace_projects,
};
use pnpm_config::Config;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
};

type BoxedResult<'a, Output> = Pin<Box<dyn Future<Output = miette::Result<Output>> + Send + 'a>>;

/// The state every linked-project walk of one `list` run shares.
pub(super) struct SharedLinkedProjects {
    workspace_project_dirs: HashSet<PathBuf>,
    /// The number of dependencies under each linked project already
    /// expanded in the output, by directory and depth, so that a repeated
    /// one is marked deduped instead of walked.
    expanded: Mutex<HashMap<(PathBuf, MaxDepth), u64>>,
}

impl SharedLinkedProjects {
    fn load(config: &Config) -> miette::Result<Self> {
        let workspace_project_dirs = match &config.workspace_dir {
            Some(workspace_dir) => discover_workspace_projects(workspace_dir, config)?
                .0
                .into_iter()
                .map(|project| project.root_dir)
                .collect(),
            None => HashSet::new(),
        };
        Ok(SharedLinkedProjects { workspace_project_dirs, expanded: Mutex::default() })
    }

    fn previously_expanded(&self, key: &(PathBuf, MaxDepth)) -> Option<u64> {
        self.expanded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .copied()
    }

    fn record_expanded(&self, key: (PathBuf, MaxDepth), count: u64) {
        self.expanded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, count);
    }
}

/// The lockfile a tree was built from.
struct ReadLockfile<'a> {
    dir: &'a Path,
    importer_ids: HashSet<String>,
}

impl ReadLockfile<'_> {
    fn has_importer_for(&self, project_dir: &Path) -> bool {
        self.importer_ids.contains(&importer_id_for(self.dir, project_dir))
    }
}

/// The walk of one listed project's linked projects.
struct LinkedProjects<'a> {
    config: &'a Config,
    params: &'a [String],
    lockfile: &'a ReadLockfile<'a>,
    depth: MaxDepth,
    shared: Arc<SharedLinkedProjects>,
    ancestors: HashSet<PathBuf>,
    rewrite_link_version_dir: &'a Path,
    searching: bool,
}

impl ListArgs {
    /// Attach the project dependencies of every linked workspace project
    /// that the lockfile has no importer for. With `sharedWorkspaceLockfile: false`, the
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
        let lockfile = ReadLockfile {
            dir: lockfile_dir,
            importer_ids: env.current_lockfile.importers
                .keys()
                .map(ToString::to_string)
                .collect(),
        };
        Box::pin(async move {
            let shared = match &request.linked_projects {
                Some(shared) => Arc::clone(shared),
                None => Arc::new(SharedLinkedProjects::load(config)?),
            };
            for (project_dir, hierarchy) in hierarchies {
                let mut ancestors = request.linked_project_ancestors.clone();
                ancestors.insert(project_dir.clone());
                let walk = LinkedProjects {
                    config,
                    params: request.params,
                    lockfile: &lockfile,
                    depth: request.depth,
                    shared: Arc::clone(&shared),
                    ancestors,
                    rewrite_link_version_dir: project_dir,
                    searching: !request.params.is_empty() || !self.find_by.is_empty(),
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
                    expanded.extend(keep_searched(node, walk));
                    continue;
                }
                let path = PathBuf::from(&node.package.path);
                if node.status.circular || walk.lockfile.has_importer_for(&path) {
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

    /// `None` when the linked directory is not a workspace project, or when
    /// neither it nor its project dependencies match the search.
    async fn expand_linked_project(
        &self,
        walk: &LinkedProjects<'_>,
        mut node: DependencyNode,
        project_dir: &Path,
        level: u64,
    ) -> miette::Result<Option<DependencyNode>> {
        if !walk.shared.workspace_project_dirs.contains(project_dir) {
            return Ok(None);
        }
        if walk.ancestors.contains(project_dir) {
            node.status.circular = true;
            return Ok(keep_searched(node, walk));
        }
        let Some(depth) = depth_below(walk.depth, level) else {
            return Ok(keep_searched(node, walk));
        };
        let key = (project_dir.to_path_buf(), depth);
        if let Some(count) = walk.shared.previously_expanded(&key) {
            if count > 0 {
                node.status.deduped = true;
                node.status.deduped_dependencies_count = Some(count);
                return Ok(Some(node));
            }
            return Ok(keep_searched(node, walk));
        }
        let request = TreeRequest {
            params: walk.params,
            depth,
            linked_project_ancestors: walk.ancestors.clone(),
            linked_projects: Some(Arc::clone(&walk.shared)),
        };
        node.dependencies =
            self.load_linked_project_dependencies(walk.config, project_dir, &request).await?;
        rewrite_link_versions(&mut node.dependencies, walk.rewrite_link_version_dir);
        walk.shared.record_expanded(key, count_nodes(&node.dependencies));
        Ok(keep_searched(node, walk))
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

fn count_nodes(nodes: &[DependencyNode]) -> u64 {
    nodes
        .iter()
        .map(|node| 1 + count_nodes(&node.dependencies))
        .sum()
}

fn keep_searched(node: DependencyNode, walk: &LinkedProjects<'_>) -> Option<DependencyNode> {
    let pruned = walk.searching && !node.search.matched && node.dependencies.is_empty();
    (!pruned).then_some(node)
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
