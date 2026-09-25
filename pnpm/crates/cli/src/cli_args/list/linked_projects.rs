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
use pnpm_fs::lexical_normalize;
use pnpm_workspace::Project;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
};

type BoxedResult<'a, Output> = Pin<Box<dyn Future<Output = miette::Result<Output>> + Send + 'a>>;

pub(super) struct SharedLinkedProjects {
    /// The workspace project each directory that dependents link to belongs
    /// to.
    linked_project_dirs: Arc<HashMap<PathBuf, PathBuf>>,
    /// A linked project met again at the same depth is marked deduped
    /// instead of walked again.
    expanded: Mutex<HashMap<(PathBuf, MaxDepth), u64>>,
}

impl SharedLinkedProjects {
    fn new(linked_project_dirs: Arc<HashMap<PathBuf, PathBuf>>) -> Self {
        SharedLinkedProjects { linked_project_dirs, expanded: Mutex::default() }
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

struct ReadLockfile<'a> {
    dir: &'a Path,
    importer_ids: HashSet<String>,
}

impl ReadLockfile<'_> {
    fn has_importer_for(&self, project_dir: &Path) -> bool {
        self.importer_ids.contains(&importer_id_for(self.dir, project_dir))
    }
}

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
    /// that the lockfile has no importer for. With
    /// `sharedWorkspaceLockfile: false`, the lockfile the tree was built
    /// from knows nothing about the dependencies of the other workspace
    /// projects.
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
                None => Arc::new(SharedLinkedProjects::new(self.linked_project_dirs(config)?)),
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

    /// Discover the projects under `workspace_root` and remember them as the
    /// ones `--only-projects` follows, so a recursive listing follows the
    /// projects it selects from, even without a workspace manifest.
    pub(super) fn discover_listed_projects(
        &self,
        workspace_root: &Path,
        config: &Config,
    ) -> miette::Result<Vec<Project>> {
        let (projects, _) = discover_workspace_projects(workspace_root, config)?;
        let _ = self.linked_project_dirs.set(Arc::new(linked_project_dirs(&projects)));
        Ok(projects)
    }

    fn linked_project_dirs(
        &self,
        config: &Config,
    ) -> miette::Result<Arc<HashMap<PathBuf, PathBuf>>> {
        if let Some(dirs) = self.linked_project_dirs.get() {
            return Ok(Arc::clone(dirs));
        }
        let dirs = match &config.workspace_dir {
            Some(workspace_dir) => {
                linked_project_dirs(&discover_workspace_projects(workspace_dir, config)?.0)
            }
            None => HashMap::new(),
        };
        Ok(Arc::clone(self.linked_project_dirs.get_or_init(|| Arc::new(dirs))))
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
        linked_dir: &Path,
        level: u64,
    ) -> miette::Result<Option<DependencyNode>> {
        let Some(project_dir) = walk.shared.linked_project_dirs.get(linked_dir) else {
            return Ok(None);
        };
        let project_dir = project_dir.as_path();
        node.package.path = project_dir.to_string_lossy().into_owned();
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
        rewrite_link_versions(&mut node.dependencies, project_dir, walk.rewrite_link_version_dir);
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
        let Some(env) = state.env_for_config(project_dir, config) else {
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

/// Express the `link:` versions of `nodes`, relative to `linked_project_dir`,
/// relative to the listed project, as the tree of a shared lockfile does.
fn rewrite_link_versions(
    nodes: &mut [DependencyNode],
    linked_project_dir: &Path,
    rewrite_link_version_dir: &Path,
) {
    for node in nodes {
        if let Some(link_target) = node.package.version.strip_prefix("link:") {
            let path = lexical_normalize(&linked_project_dir.join(link_target));
            let relative = pathdiff::diff_paths(&path, rewrite_link_version_dir).unwrap_or(path);
            node.package.version =
                format!("link:{}", relative.to_string_lossy().replace('\\', "/"));
        }
        rewrite_link_versions(&mut node.dependencies, linked_project_dir, rewrite_link_version_dir);
    }
}

/// The workspace project each directory that dependents link to belongs to:
/// its own directory, and its publish directory when it sets
/// `publishConfig.directory` without `publishConfig.linkDirectory: false`.
/// A project's own directory wins over a publish directory pointing at it.
fn linked_project_dirs(projects: &[Project]) -> HashMap<PathBuf, PathBuf> {
    let mut dirs: HashMap<PathBuf, PathBuf> = projects
        .iter()
        .map(|project| (project.root_dir.clone(), project.root_dir.clone()))
        .collect();
    for project in projects {
        let publish_config = project.manifest.value().get("publishConfig");
        let publish_directory = publish_config
            .and_then(|publish_config| publish_config.get("directory"))
            .and_then(serde_json::Value::as_str);
        let links_publish_directory = publish_config
            .and_then(|publish_config| publish_config.get("linkDirectory"))
            .and_then(serde_json::Value::as_bool)
            != Some(false);
        if links_publish_directory && let Some(publish_directory) = publish_directory {
            dirs.entry(lexical_normalize(&project.root_dir.join(publish_directory)))
                .or_insert_with(|| project.root_dir.clone());
        }
    }
    dirs
}
