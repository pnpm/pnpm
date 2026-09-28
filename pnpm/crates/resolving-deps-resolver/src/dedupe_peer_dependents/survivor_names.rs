use pnpm_deps_path::{DepPath, PeerId, create_peer_dep_graph_hash, index_of_dep_path_suffix};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{borrow::Cow, collections::hash_map::Entry};

use crate::{
    dedupe_injected_deps::{DirectByImporter, prune_unreachable},
    dependencies_graph::DependenciesGraph,
    resolve_peers::split_peer_suffix_segments,
};

/// How the peer resolution built the suffixes of the depPaths it emitted.
pub(crate) struct PeerSuffixes<'a> {
    /// The peer ids each peer-suffixed depPath was built from.
    pub(crate) peer_ids: &'a HashMap<DepPath, Vec<PeerId>>,
    /// `peersSuffixMaxLength`: the suffix length above which it is hashed.
    pub(crate) max_length: usize,
}

/// Rename every node whose peer suffix names a collapsed variant, so the
/// suffix names the variant that absorbed it.
///
/// Rewrites the graph keys, the child edges and each importer's direct
/// deps. When several nodes end up with one name, a node already keyed by
/// it wins, otherwise the lowest old depPath does, and the nodes only the
/// losers reached are dropped.
pub(super) fn rename_survivors(
    graph: &mut DependenciesGraph,
    direct_by_importer: &mut DirectByImporter,
    collapsed: &HashMap<DepPath, DepPath>,
    peer_suffixes: &PeerSuffixes<'_>,
) {
    let renames = survivor_renames(graph, collapsed, peer_suffixes);
    if renames.is_empty() {
        return;
    }
    let merged = rekey(graph, &renames);
    let renames: HashMap<DepPath, DepPath> = renames.into_iter().collect();
    let dep_paths = graph
        .values_mut()
        .flat_map(|node| node.edges.children.values_mut())
        .chain(direct_by_importer.values_mut().flat_map(|direct| direct.values_mut()));
    for dep_path in dep_paths {
        if let Some(name) = renames.get(dep_path) {
            *dep_path = name.clone();
        }
    }
    if merged {
        prune_unreachable(graph, direct_by_importer);
    }
}

/// `old → new` for every graph key whose name changes, sorted by old key.
fn survivor_renames(
    graph: &DependenciesGraph,
    collapsed: &HashMap<DepPath, DepPath>,
    peer_suffixes: &PeerSuffixes<'_>,
) -> Vec<(DepPath, DepPath)> {
    let mut namer = SurvivorNamer {
        collapsed,
        peer_suffixes,
        names: HashMap::default(),
        naming: HashSet::default(),
    };
    let mut renames: Vec<(DepPath, DepPath)> = graph
        .keys()
        .filter_map(|dep_path| {
            let name = namer.name_of(dep_path);
            (name != *dep_path).then(|| (dep_path.clone(), name))
        })
        .collect();
    renames.sort();
    renames
}

/// Move each renamed node to its new key. Returns whether any node was
/// dropped because its new key was already taken.
fn rekey(graph: &mut DependenciesGraph, renames: &[(DepPath, DepPath)]) -> bool {
    let renamed_nodes: Vec<_> = renames
        .iter()
        .map(|(old, new)| {
            let node = graph.remove(old).expect("renamed depPaths come from the graph keys");
            (new.clone(), node)
        })
        .collect();
    let mut merged = false;
    for (new, mut node) in renamed_nodes {
        match graph.entry(new) {
            Entry::Vacant(entry) => {
                node.dep_path = entry.key().clone();
                entry.insert(node);
            }
            Entry::Occupied(_) => merged = true,
        }
    }
    merged
}

struct SurvivorNamer<'a> {
    collapsed: &'a HashMap<DepPath, DepPath>,
    peer_suffixes: &'a PeerSuffixes<'a>,
    names: HashMap<DepPath, DepPath>,
    naming: HashSet<DepPath>,
}

impl<'a> SurvivorNamer<'a> {
    fn name_of(&mut self, dep_path: &DepPath) -> DepPath {
        let dep_path = self.collapsed.get(dep_path).unwrap_or(dep_path);
        if let Some(name) = self.names.get(dep_path) {
            return name.clone();
        }
        if !self.naming.insert(dep_path.clone()) {
            return dep_path.clone();
        }
        let name = self.rename_peers(dep_path).unwrap_or_else(|| dep_path.clone());
        self.naming.remove(dep_path);
        self.names.insert(dep_path.clone(), name.clone());
        name
    }

    /// `None` when no peer in the suffix was renamed.
    fn rename_peers(&mut self, dep_path: &DepPath) -> Option<DepPath> {
        let peers_index = index_of_dep_path_suffix(dep_path.as_str()).peers_index?;
        let (pkg_id, suffix) = dep_path.as_str().split_at(peers_index);
        let peer_suffixes: &'a PeerSuffixes<'a> = self.peer_suffixes;
        let peer_ids: Cow<'a, [PeerId]> = match peer_suffixes.peer_ids.get(dep_path) {
            Some(peer_ids) => Cow::Borrowed(peer_ids),
            // A depPath the peer resolution did not emit as final is the
            // provisional one it put in a suffix to break a cycle. Its
            // segments are read back from the text.
            None => split_peer_suffix_segments(suffix)?
                .into_iter()
                .map(|segment| PeerId::DepPath(DepPath::from(segment)))
                .collect(),
        };
        let mut renamed = false;
        let peer_ids: Vec<PeerId> = peer_ids
            .iter()
            .map(|peer_id| match peer_id {
                PeerId::DepPath(peer_dep_path) => {
                    let name = self.name_of(peer_dep_path);
                    renamed |= name != *peer_dep_path;
                    PeerId::DepPath(name)
                }
                PeerId::Pair { .. } => peer_id.clone(),
            })
            .collect();
        renamed.then(|| {
            let suffix = create_peer_dep_graph_hash(&peer_ids, self.peer_suffixes.max_length);
            DepPath::from(format!("{pkg_id}{suffix}"))
        })
    }
}
