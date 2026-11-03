use super::{
    CacheMatch, FastProvider, FastProviderQuery, NodeId, ParentRefs, PeersCacheItem, TreeChildren,
    Walker,
};

impl Walker<'_> {
    pub(in super::super) fn find_fast_hit(
        &self,
        node_id: &NodeId,
        parent_refs: &ParentRefs,
        pkg_id: &str,
    ) -> Option<&PeersCacheItem> {
        if !matches!(self.tree.dependencies_tree.get(node_id)?.children, TreeChildren::Lazy) {
            return None;
        }
        self.find_fast_hit_for_lazy(parent_refs, pkg_id)
    }

    pub(super) fn find_fast_hit_for_lazy(
        &self,
        parent_refs: &ParentRefs,
        pkg_id: &str,
    ) -> Option<&PeersCacheItem> {
        if !super::reuse_peer_subtrees(&self.opts) {
            return None;
        }
        let canonical_scc = self.canonical_scc();
        let query = FastProviderQuery { canonical_scc: &canonical_scc, parent_refs, pkg_id };
        self.caches.peers_cache
            .get(pkg_id)?
            .iter()
            .find(|item| matches!(self.fast_cache_item_matches(query, item), CacheMatch::Match))
    }

    pub(super) fn fast_cache_item_matches(
        &self,
        query: FastProviderQuery<'_>,
        item: &PeersCacheItem,
    ) -> CacheMatch {
        let mut ambiguous = false;
        for (name, cached_node_id) in item.resolved_peers.iter() {
            match self.fast_resolved_peer_matches(query, name, cached_node_id) {
                CacheMatch::NoMatch => return CacheMatch::NoMatch,
                CacheMatch::Ambiguous => ambiguous = true,
                CacheMatch::Match => {}
            }
        }
        for name in item.missing_peers.keys() {
            match self.fast_provider_for_name(query, name) {
                FastProvider::Missing => {}
                FastProvider::Inherited(_) | FastProvider::Child(_) => {
                    return CacheMatch::NoMatch;
                }
                FastProvider::Ambiguous => ambiguous = true,
            }
        }
        if ambiguous { CacheMatch::Ambiguous } else { CacheMatch::Match }
    }

    pub(super) fn fast_resolved_peer_matches(
        &self,
        query: FastProviderQuery<'_>,
        name: &str,
        cached_node_id: &NodeId,
    ) -> CacheMatch {
        match self.fast_provider_for_name(query, name) {
            FastProvider::Missing => CacheMatch::NoMatch,
            FastProvider::Inherited(current_ref) => {
                if self.parent_ref_matches_cached(current_ref, cached_node_id) {
                    CacheMatch::Match
                } else {
                    CacheMatch::NoMatch
                }
            }
            FastProvider::Child(child_pkg_id) => {
                self.fast_child_provider_matches(child_pkg_id, cached_node_id)
            }
            FastProvider::Ambiguous => CacheMatch::Ambiguous,
        }
    }

    /// A child provider only settles the match when the child cannot itself be
    /// re-resolved into a different variant later on.
    pub(super) fn fast_child_provider_matches(
        &self,
        child_pkg_id: &str,
        cached_node_id: &NodeId,
    ) -> CacheMatch {
        let Some(cached_tree_node) = self.tree.dependencies_tree.get(cached_node_id) else {
            return CacheMatch::NoMatch;
        };
        if &*cached_tree_node.resolved_package_id != child_pkg_id {
            return CacheMatch::NoMatch;
        }
        let child_is_stable = self.caches.pure_pkgs.contains_key(child_pkg_id)
            || matches!(
                cached_node_id,
                NodeId::Leaf(cached_pkg_id) if cached_pkg_id.as_ref() == child_pkg_id,
            );
        if child_is_stable { CacheMatch::Match } else { CacheMatch::Ambiguous }
    }

    pub(super) fn fast_provider_for_name<'a>(
        &'a self,
        query: FastProviderQuery<'a>,
        name: &str,
    ) -> FastProvider<'a> {
        let FastProviderQuery { canonical_scc, parent_refs, pkg_id } = query;
        let inherited = parent_refs.get(name);
        let Some(children) = self.tree.children_by_id.get(pkg_id) else {
            return inherited.map_or(FastProvider::Missing, FastProvider::Inherited);
        };
        let Some(edge_indices) = self.caches.peer_providers.children_by_pkg_id
            .get(pkg_id)
            .and_then(|providers| providers.edge_indices_by_name.get(name))
        else {
            return inherited.map_or(FastProvider::Missing, FastProvider::Inherited);
        };

        let mut child_pkg_id = None;
        for &edge_index in edge_indices {
            let edge = &children[edge_index];
            if Self::cuts_cycle_edge(canonical_scc, pkg_id, &edge.pkg_id) {
                continue;
            }
            if child_pkg_id.is_some() {
                return FastProvider::Ambiguous;
            }
            child_pkg_id = Some(&*edge.pkg_id);
        }

        match (inherited, child_pkg_id) {
            (Some(_), Some(_)) => FastProvider::Ambiguous,
            (Some(parent_ref), None) => FastProvider::Inherited(parent_ref),
            (None, Some(pkg_id)) => FastProvider::Child(pkg_id),
            (None, None) => FastProvider::Missing,
        }
    }
}
