use super::ResolvePeersOptions;
use crate::{NodeId, ResolvedTree, TreeChildren};
use pnpm_deps_path::{DepPath, index_of_dep_path_suffix};
use pnpm_lockfile::Lockfile;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

pub(crate) fn has_contexts(tree: &ResolvedTree) -> bool {
    tree.dependencies_tree
        .values()
        .any(|node| node.locked_peer_context().is_some())
}

pub(crate) fn mark_changed_children(tree: &mut ResolvedTree, lockfile: Option<&Lockfile>) {
    let changed: Vec<_> = tree.dependencies_tree
        .iter()
        .filter_map(|(id, node)| {
            let TreeChildren::Realized(children) = &node.children else { return None };
            let snapshot = node
                .previous_dep_path()
                .and_then(|path| path.as_str().parse().ok())
                .and_then(|key| lockfile?.snapshots.as_ref()?.get(&key));
            let aliases: HashSet<_> = children
                .iter()
                .filter(|(alias, child)| {
                    let previous = locked_child_path(snapshot, alias);
                    let current = tree.dependencies_tree
                        .get(*child)
                        .and_then(|node| node.previous_dep_path());
                    match (previous, current) {
                        (Some(previous), Some(current)) => previous.to_string() != current.as_str(),
                        _ => true,
                    }
                })
                .map(|(alias, _)| alias.clone())
                .collect();
            (!aliases.is_empty()).then(|| (id.clone(), aliases))
        })
        .collect();
    for (id, aliases) in changed {
        tree.dependencies_tree
            .get_mut(&id)
            .unwrap()
            .locked_mut()
            .dependency_names_whose_current_provider_must_win = Some(aliases);
    }
}

pub(crate) fn has_pinnable_context(tree: &ResolvedTree, paths: &HashMap<NodeId, DepPath>) -> bool {
    let providers: HashSet<_> = paths
        .keys()
        .filter_map(|id| stable_provider_path(tree, id, paths))
        .collect();
    tree.dependencies_tree
        .iter()
        .any(|(id, node)| {
            node.previous_dep_path() != paths.get(id)
                && node
                    .locked_peer_context()
                    .is_some_and(|context| {
                        context
                            .values()
                            .any(|path| providers.contains(path))
                    })
        })
}

pub(crate) fn stable_provider_path<'a>(
    tree: &'a ResolvedTree,
    id: &NodeId,
    paths: &HashMap<NodeId, DepPath>,
) -> Option<&'a DepPath> {
    let node = tree.dependencies_tree.get(id)?;
    if !tree.packages.get(&node.resolved_package_id)?.is_leaf {
        return None;
    }
    let previous = node.previous_dep_path()?;
    (paths.get(id) == Some(previous)
        && index_of_dep_path_suffix(previous.as_str()).peers_index.is_none())
    .then_some(previous)
}

/// The first stable node in deterministic id order for each recorded provider path.
/// Empty unless a locked-peer reuse pass was requested.
pub(super) fn build_node_ids_by_previous_dep_path(
    tree: &ResolvedTree,
    opts: &ResolvePeersOptions,
) -> HashMap<DepPath, NodeId> {
    let mut map = HashMap::default();
    let Some(paths) = &opts.scope.resolved_peer_provider_paths else { return map };
    let mut node_ids: Vec<&NodeId> = tree.dependencies_tree.keys().collect();
    node_ids.sort();
    for node_id in node_ids {
        if let Some(previous) = stable_provider_path(tree, node_id, paths)
            && !map.contains_key(previous)
        {
            map.insert(previous.clone(), node_id.clone());
        }
    }
    map
}

fn locked_child_path(
    snapshot: Option<&pnpm_lockfile::SnapshotEntry>,
    alias: &str,
) -> Option<pnpm_lockfile::PkgNameVerPeer> {
    let snapshot = snapshot?;
    let name = alias.parse().ok()?;
    snapshot.dependencies
        .as_ref()
        .and_then(|deps| deps.get(&name))
        .or_else(|| snapshot.optional_dependencies.as_ref()?.get(&name))?
        .resolve(&name)
}
