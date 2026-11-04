use super::{CacheMatch, HashSet, NodeId, ParentRef, Walker};
use pnpm_deps_path::index_of_dep_path_suffix;
use std::collections::VecDeque;

impl Walker<'_> {
    pub(super) fn parent_ref_matches_cached(
        &self,
        current_ref: &ParentRef,
        cached_node_id: &NodeId,
    ) -> bool {
        let Some(current_node_id) = current_ref.node_id.as_ref() else {
            return false;
        };
        match self.provider_identity_match(current_node_id, cached_node_id) {
            CacheMatch::Match => true,
            CacheMatch::NoMatch => false,
            CacheMatch::Ambiguous => self.pure_provider_matches(current_node_id, cached_node_id),
        }
    }

    /// Equal provisional paths still need equal nested providers. Unknown
    /// nested bindings prevent reuse; repeated pairs terminate peer cycles.
    pub(super) fn provider_identity_match(
        &self,
        current_node_id: &NodeId,
        cached_node_id: &NodeId,
    ) -> CacheMatch {
        let current = self.cache_owner_node_id(current_node_id);
        let cached = self.cache_owner_node_id(cached_node_id);
        if current == cached {
            return CacheMatch::Match;
        }
        let mut pending = VecDeque::new();
        let root_match = self.computed_provider_match(current, cached, &mut pending);
        if root_match != CacheMatch::Match || pending.is_empty() {
            return root_match;
        }
        let mut compared = HashSet::default();
        let mut verdict = CacheMatch::Match;
        while let Some((current, cached)) = pending.pop_front() {
            let current = self.cache_owner_node_id(current);
            let cached = self.cache_owner_node_id(cached);
            if current == cached || !compared.insert((current, cached)) {
                continue;
            }
            match self.computed_provider_match(current, cached, &mut pending) {
                CacheMatch::NoMatch => return CacheMatch::NoMatch,
                CacheMatch::Ambiguous => verdict = CacheMatch::Ambiguous,
                CacheMatch::Match => {}
            }
        }
        verdict
    }

    fn computed_provider_match<'a>(
        &'a self,
        current: &'a NodeId,
        cached: &'a NodeId,
        pending: &mut VecDeque<(&'a NodeId, &'a NodeId)>,
    ) -> CacheMatch {
        let (Some(current_path), Some(cached_path)) =
            (self.caches.node_dep_paths.get(current), self.caches.node_dep_paths.get(cached))
        else {
            return CacheMatch::Ambiguous;
        };
        if current_path != cached_path {
            return CacheMatch::NoMatch;
        }
        if index_of_dep_path_suffix(current_path.as_str()).peers_index.is_none() {
            return CacheMatch::Match;
        }
        let (Some(current_peers), Some(cached_peers)) =
            (self.nodes.external_peers.get(current), self.nodes.external_peers.get(cached))
        else {
            return CacheMatch::Ambiguous;
        };
        if current_peers.len() != cached_peers.len() {
            return CacheMatch::NoMatch;
        }
        for (name, current_peer) in current_peers.iter() {
            let Some(cached_peer) = cached_peers.get(name) else {
                return CacheMatch::NoMatch;
            };
            let current_peer = self.cache_owner_node_id(current_peer);
            let cached_peer = self.cache_owner_node_id(cached_peer);
            if current_peer != cached_peer {
                pending.push_back((current_peer, cached_peer));
            }
        }
        CacheMatch::Match
    }

    fn pure_provider_matches(&self, current_node_id: &NodeId, cached_node_id: &NodeId) -> bool {
        let (Some(cached_tree_node), Some(current_tree_node)) = (
            self.tree.dependencies_tree.get(cached_node_id),
            self.tree.dependencies_tree.get(current_node_id),
        ) else {
            return false;
        };
        let parent_pkg_id = &current_tree_node.resolved_package_id;
        if parent_pkg_id != &cached_tree_node.resolved_package_id {
            return false;
        }
        self.caches.pure_pkgs.contains_key(&**parent_pkg_id)
    }
}
