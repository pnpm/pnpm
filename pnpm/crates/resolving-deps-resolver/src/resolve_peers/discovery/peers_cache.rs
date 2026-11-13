use super::{Arc, HashMap, HashSet, NodeId, PeersCacheItem};
use std::collections::{BTreeMap, btree_map};

#[derive(Debug, Default)]
struct PkgCacheEntries {
    next_id: usize,
    items: BTreeMap<usize, PeersCacheItem>,
}

#[derive(Debug, Default)]
pub(in super::super) struct PeersCache {
    by_pkg: HashMap<Arc<str>, PkgCacheEntries>,
    items_by_provider: HashMap<NodeId, HashSet<(Arc<str>, usize)>>,
}

impl PeersCache {
    pub(in super::super) fn get(
        &self,
        pkg_id: &str,
    ) -> Option<btree_map::Values<'_, usize, PeersCacheItem>> {
        self.by_pkg.get(pkg_id).map(|entries| entries.items.values())
    }

    pub(in super::super) fn push(&mut self, pkg_id: &Arc<str>, item: PeersCacheItem) {
        let entries = self.by_pkg
            .entry(Arc::clone(pkg_id))
            .or_default();
        let item_id = entries.next_id;
        entries.next_id += 1;
        for provider_id in item.resolved_peers.values() {
            self.items_by_provider
                .entry(provider_id.clone())
                .or_default()
                .insert((Arc::clone(pkg_id), item_id));
        }
        entries.items.insert(item_id, item);
    }

    pub(in super::super) fn evict_referencing(&mut self, changed_node_id: &NodeId) {
        let Some(refs) = self.items_by_provider.remove(changed_node_id) else {
            return;
        };
        for (pkg_id, item_id) in refs {
            self.remove_item(&pkg_id, item_id, changed_node_id);
        }
    }

    fn remove_item(&mut self, pkg_id: &Arc<str>, item_id: usize, changed_node_id: &NodeId) {
        let Some(removed) = self.take_pkg_item(pkg_id, item_id) else {
            return;
        };
        for provider_id in removed.resolved_peers.values() {
            if provider_id != changed_node_id {
                self.unlink_provider_ref(provider_id, pkg_id, item_id);
            }
        }
    }

    fn take_pkg_item(&mut self, pkg_id: &Arc<str>, item_id: usize) -> Option<PeersCacheItem> {
        let entries = self.by_pkg.get_mut(pkg_id)?;
        let removed = entries.items.remove(&item_id);
        if entries.items.is_empty() {
            self.by_pkg.remove(pkg_id);
        }
        removed
    }

    fn unlink_provider_ref(&mut self, provider_id: &NodeId, pkg_id: &Arc<str>, item_id: usize) {
        let Some(refs) = self.items_by_provider.get_mut(provider_id) else {
            return;
        };
        refs.remove(&(Arc::clone(pkg_id), item_id));
        if refs.is_empty() {
            self.items_by_provider.remove(provider_id);
        }
    }
}
