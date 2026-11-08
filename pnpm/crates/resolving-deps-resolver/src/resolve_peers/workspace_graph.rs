//! The workspace graph rebuilt with full peer suffixes, before the dedupe passes.

use super::{
    ImporterPeerInput, Walker,
    context::importer_relative_link_dep_path,
    finalize::{FinalDepPaths, PeerIdRecording},
};
use crate::{
    PeerDependentsDedupe,
    dedupe_injected_deps::dedupe_injected_deps,
    dedupe_peer_dependents::{
        PeerSuffixes, collect_unbound_optional_peers, dedupe_peer_dependents,
    },
    dependencies_graph::DependenciesGraph,
    node_id::NodeId,
    peer_dependents_dedupe::UnboundOptionalPeers,
};
use pnpm_deps_path::{DepPath, PeerId};
use rustc_hash::FxHashMap as HashMap;
use std::{collections::BTreeMap, path::Path};

pub(super) struct FinishedWorkspaceGraph {
    pub(super) graph: DependenciesGraph,
    pub(super) direct_dependencies_by_importer: BTreeMap<String, BTreeMap<String, DepPath>>,
    pub(super) paths_by_node_id: HashMap<NodeId, DepPath>,
    peer_ids: HashMap<DepPath, Vec<PeerId>>,
}

impl FinishedWorkspaceGraph {
    pub(super) fn dedupe_peer_dependents(
        &mut self,
        peers_suffix_max_length: usize,
        dedupe: PeerDependentsDedupe,
        importers: &[&ImporterPeerInput],
    ) {
        if !dedupe.dedupes() {
            return;
        }
        let unbound_optional_peers = self.unbound_optional_peers(importers);
        dedupe_peer_dependents(
            &mut self.graph,
            &mut self.direct_dependencies_by_importer,
            &PeerSuffixes { peer_ids: &self.peer_ids, max_length: peers_suffix_max_length },
            dedupe,
            unbound_optional_peers,
        );
    }

    pub(super) fn dedupe_injected_deps(
        &mut self,
        importers: &[&ImporterPeerInput],
        lockfile_dir: &Path,
    ) {
        // Collected again: the peer-dependent dedupe renamed variants.
        let unbound_optional_peers = self.unbound_optional_peers(importers);
        dedupe_injected_deps(
            &mut self.graph,
            &mut self.direct_dependencies_by_importer,
            &importers
                .iter()
                .map(|importer| (importer.id.clone(), importer.root_dir.clone()))
                .collect(),
            lockfile_dir,
            &unbound_optional_peers,
        );
    }

    fn unbound_optional_peers(&self, importers: &[&ImporterPeerInput]) -> UnboundOptionalPeers {
        collect_unbound_optional_peers(
            &self.graph,
            &self.direct_dependencies_by_importer,
            importers
                .iter()
                .map(|importer| (importer.id.as_str(), &importer.rejected_optional_peers)),
        )
    }
}

/// Recompute depPaths with full peer suffixes once, after every importer
/// is walked, then rebuild the graph and re-key each importer's direct
/// deps.
pub(super) fn finish_workspace_graph(
    walker: &Walker<'_>,
    importers: &[&ImporterPeerInput],
    lockfile_dir: &Path,
    dedupe: PeerDependentsDedupe,
) -> FinishedWorkspaceGraph {
    let recording = if dedupe.dedupes() { PeerIdRecording::Record } else { PeerIdRecording::Skip };
    let FinalDepPaths {
        by_node_id: final_dep_paths,
        peer_ids,
        ..
    } = walker.build_final_dep_paths(recording);
    let direct_dependencies_by_importer = importers
        .iter()
        .map(|importer| {
            let anchor = crate::link_target::ImporterAnchor::new(&importer.root_dir, lockfile_dir);
            let direct_by_alias = importer.direct
                .iter()
                .map(|dep| {
                    let dep_path = importer_relative_link_dep_path(
                        &walker.final_dep_path_of(&dep.node_id, &final_dep_paths),
                        &anchor,
                        Some(lockfile_dir),
                        Some(&importer.root_dir),
                    );
                    (dep.alias.clone(), dep_path)
                })
                .collect();
            (importer.id.clone(), direct_by_alias)
        })
        .collect();
    FinishedWorkspaceGraph {
        graph: walker.build_final_graph(&final_dep_paths),
        direct_dependencies_by_importer,
        paths_by_node_id: walker.final_paths_by_node_id(&final_dep_paths),
        peer_ids,
    }
}
