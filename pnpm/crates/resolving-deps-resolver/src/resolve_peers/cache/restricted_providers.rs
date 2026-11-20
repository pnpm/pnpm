use super::{
    Arc,
    HashMap,
    ParentRef,
    ParentRefs,
    PeersCacheItem,
    Walker,
};

impl Walker<'_> {
    /// The version of `parent` when it is restricted: a hoisted provider
    /// that some optional peer range in the tree rejects. Only such a
    /// provider can resolve a subtree differently from a declared one, so
    /// a cached verdict is reused only under the restriction it was
    /// recorded under.
    pub(in super::super) fn restricted_version<'refs>(
        &self,
        peer_name: &str,
        parent: &'refs ParentRef,
    ) -> Option<&'refs Arc<str>> {
        let is_restricted = parent.hoisted_provider
            && self.caches.peers_cache.optional_peer_ranges.rejects(
                self.tree,
                self.caches.view_generation(),
                (peer_name, &parent.version),
            );
        is_restricted.then_some(&parent.version)
    }

    /// The [`PeersCacheItem::restricted_providers`] of a verdict over
    /// `peer_names`, resolved against `parent_refs`.
    pub(in super::super) fn restricted_providers<'names>(
        &self,
        parent_refs: &ParentRefs,
        peer_names: impl Iterator<Item = &'names String>,
    ) -> HashMap<String, Arc<str>> {
        peer_names
            .filter_map(|name| {
                let version = self.restricted_version(name, parent_refs.get(name)?)?;
                Some((name.clone(), Arc::clone(version)))
            })
            .collect()
    }

    pub(in super::super) fn has_restriction_of(
        &self,
        item: &PeersCacheItem,
        peer_name: &str,
        current_ref: &ParentRef,
    ) -> bool {
        self.restricted_version(peer_name, current_ref)
            == item.restricted_providers.get(peer_name)
    }

    /// A missing peer stays missing when nothing provides it, or when the
    /// restricted provider that the cached walk rejected provides it.
    pub(in super::super) fn missing_peer_still_missing(
        &self,
        item: &PeersCacheItem,
        peer_name: &str,
        parent_refs: &ParentRefs,
    ) -> bool {
        parent_refs
            .get(peer_name)
            .is_none_or(|current_ref| {
                item.restricted_providers.contains_key(peer_name)
                    && self.has_restriction_of(item, peer_name, current_ref)
            })
    }
}
