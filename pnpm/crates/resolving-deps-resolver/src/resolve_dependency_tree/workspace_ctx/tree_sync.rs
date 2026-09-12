use super::{
    Arc, HashMap, NodeId, ResolvedTree, SyncCursor, SyncLog, WorkspaceTreeCtx, lock_recoverable,
    merge_synced_child_spec,
};

impl WorkspaceTreeCtx {
    /// See the `revision` field doc.
    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn bump_revision(&self) {
        self.revision.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// See the `children_rewrites` field doc.
    pub(crate) fn children_rewrites(&self) -> u64 {
        self.children_rewrites.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn record_children_rewrite(&self) {
        self.children_rewrites.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Record a write to one of the maps [`Self::sync_discovery_tree`]
    /// mirrors. Every write a sync has to observe must be recorded here,
    /// including an in-place mutation of an entry that already exists:
    /// the sync visits recorded keys only, so an unrecorded write stays
    /// invisible to the discovery engine's view.
    pub(in super::super) fn record_package_write(&self, pkg_id: &str) {
        lock_recoverable(&self.sync_log).packages.push(pkg_id.to_string());
        self.note_finalization_candidate(pkg_id);
    }

    /// Queue `pkg_id` for the next finalization sweep. See
    /// [`Self::finalization_pending`].
    pub(super) fn note_finalization_candidate(&self, pkg_id: &str) {
        if self.finalized_package.is_some() {
            lock_recoverable(&self.finalization_pending).push(Arc::from(pkg_id));
        }
    }

    /// See [`Self::record_package_write`].
    pub(in super::super) fn record_children_by_id_write(&self, pkg_id: &str) {
        lock_recoverable(&self.sync_log).children_by_id.push(pkg_id.to_string());
    }

    /// See [`Self::record_package_write`].
    pub(in super::super) fn record_tree_node_write(&self, node_id: &NodeId) {
        lock_recoverable(&self.sync_log).dependencies_tree.push(node_id.clone());
    }

    /// See [`Self::record_package_write`].
    pub(in super::super) fn record_peer_dep_name(&self, name: &str) {
        lock_recoverable(&self.sync_log).peer_dep_names.push(name.to_string());
    }

    /// Fold the context's growth since the last sync into `tree`, the
    /// peer-hoist discovery engine's persistent view of the workspace.
    ///
    /// The shared maps grow monotonically during the hoist rounds, so
    /// the sync inserts entries `tree` doesn't have yet and lowers node
    /// depths that shrank. The exceptions are a children-owner change,
    /// which can rewrite an existing package's recorded child list or
    /// peer-dependency split: those invalidate walk results already
    /// derived from the old values, so the sync reports them as
    /// unmergeable (`false`) and the engine rebuilds its view from
    /// scratch. A replaced `children_by_id` `Arc` with equal contents
    /// (an ownership handover that re-recorded the same children) is
    /// re-pointed without invalidating.
    ///
    /// The sync visits the keys written since `cursor` rather than every
    /// entry of the shared maps, which is what keeps a hoist round
    /// proportional to what the round changed instead of to the size of
    /// the workspace. On `false` the cursor is left where it was: the
    /// caller discards the view and builds a fresh one with
    /// [`Self::rebuild_discovery_tree`].
    pub(crate) fn sync_discovery_tree(
        &self,
        tree: &mut ResolvedTree,
        cursor: &mut SyncCursor,
    ) -> bool {
        let next = {
            let log = lock_recoverable(&self.sync_log);
            SyncCursor {
                packages: log.packages.len(),
                children_by_id: log.children_by_id.len(),
                dependencies_tree: log.dependencies_tree.len(),
                peer_dep_names: log.peer_dep_names.len(),
            }
        };
        if !self.sync_children_by_id(tree, cursor.children_by_id, next.children_by_id) {
            return false;
        }
        if !self.sync_packages(tree, cursor.packages, next.packages) {
            return false;
        }
        self.sync_dependencies_tree(tree, cursor.dependencies_tree, next.dependencies_tree);
        let peer_dep_names =
            self.written_since(cursor.peer_dep_names, next.peer_dep_names, |log| {
                &log.peer_dep_names
            });
        tree.all_peer_dep_names.extend(peer_dep_names);
        *cursor = next;
        true
    }

    /// `false` when a package's recorded child edges disagree with the ones
    /// already synced, which invalidates the discovery tree.
    pub(super) fn sync_children_by_id(
        &self,
        tree: &mut ResolvedTree,
        from: usize,
        to: usize,
    ) -> bool {
        let written = self.written_since(from, to, |log| &log.children_by_id);
        let children_by_id = lock_recoverable(&self.children_by_id);
        for pkg_id in &written {
            let Some(spec) = children_by_id.get(pkg_id.as_str()).map(|recorded| &recorded.edges)
            else {
                continue;
            };
            if !merge_synced_child_spec(tree, pkg_id, spec) {
                return false;
            }
        }
        true
    }

    /// `false` when a package's peer dependencies were re-read differently
    /// than the synced copy records them.
    pub(super) fn sync_packages(&self, tree: &mut ResolvedTree, from: usize, to: usize) -> bool {
        use std::collections::hash_map::Entry;
        let written = self.written_since(from, to, |log| &log.packages);
        let packages = lock_recoverable(&self.packages);
        for pkg_id in &written {
            let Some(pkg) = packages.get(pkg_id.as_str()) else { continue };
            match tree.packages.entry(Arc::from(pkg_id.clone())) {
                Entry::Vacant(entry) => {
                    entry.insert(pkg.clone());
                }
                Entry::Occupied(entry) => {
                    if entry.get().peer_dependencies != pkg.peer_dependencies {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// An occurrence reached at a shallower depth takes that depth over.
    pub(super) fn sync_dependencies_tree(&self, tree: &mut ResolvedTree, from: usize, to: usize) {
        use std::collections::hash_map::Entry;
        let written = self.written_since(from, to, |log| &log.dependencies_tree);
        let dependencies_tree = lock_recoverable(&self.dependencies_tree);
        for node_id in &written {
            let Some(node) = dependencies_tree.get(node_id) else { continue };
            match tree.dependencies_tree.entry(node_id.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(node.clone());
                }
                Entry::Occupied(mut entry) => {
                    if entry.get().depth > node.depth {
                        entry.get_mut().depth = node.depth;
                    }
                }
            }
        }
    }

    /// Fill an empty `tree` from the shared maps, and set `cursor` to
    /// where the refilled view picks the write log up.
    ///
    /// Replaying the whole write log would reach the same view, but a
    /// scan copies each key once instead of once into the log snapshot
    /// and once into the view. The cursor is read *before* the scan, so
    /// a write that lands mid-scan is either picked up here and replayed
    /// harmlessly by the next sync, or missed here and applied by it.
    pub(crate) fn rebuild_discovery_tree(&self, tree: &mut ResolvedTree, cursor: &mut SyncCursor) {
        *cursor = {
            let log = lock_recoverable(&self.sync_log);
            SyncCursor {
                packages: log.packages.len(),
                children_by_id: log.children_by_id.len(),
                dependencies_tree: log.dependencies_tree.len(),
                peer_dep_names: log.peer_dep_names.len(),
            }
        };
        for (pkg_id, recorded) in lock_recoverable(&self.children_by_id).iter() {
            tree.children_by_id
                .entry(std::sync::Arc::<str>::clone(pkg_id))
                .or_insert_with(|| Arc::clone(&recorded.edges));
        }
        for (pkg_id, pkg) in lock_recoverable(&self.packages).iter() {
            tree.packages
                .entry(std::sync::Arc::<str>::clone(pkg_id))
                .or_insert_with(|| pkg.clone());
        }
        for (node_id, node) in lock_recoverable(&self.dependencies_tree).iter() {
            tree.dependencies_tree.entry(node_id.clone()).or_insert_with(|| node.clone());
        }
        tree.all_peer_dep_names.extend(lock_recoverable(&self.all_peer_dep_names).iter().cloned());
    }

    /// The keys written to one of [`SyncLog`]'s slots between two cursor
    /// positions, copied out so the sync can take the map's own lock
    /// without holding the log's. The range is what one hoist round
    /// wrote; a from-scratch view goes through
    /// [`Self::rebuild_discovery_tree`] instead of replaying the log.
    pub(super) fn written_since<Key: Clone>(
        &self,
        from: usize,
        to: usize,
        slot: impl Fn(&SyncLog) -> &Vec<Key>,
    ) -> Vec<Key> {
        if from >= to {
            return Vec::new();
        }
        slot(&lock_recoverable(&self.sync_log))[from..to].to_vec()
    }

    /// `NodeId → pkgIdWithPatchHash` for the given peer-provider nodes,
    /// keeping only nodes the shared context resolved eagerly (a node
    /// the peer walker realized lazily has no context entry and is
    /// dropped, matching the hoist loop's providers-that-existed-before-
    /// the-pass filter).
    pub(crate) fn provider_pkg_ids<'node_ids>(
        &self,
        node_ids: impl Iterator<Item = &'node_ids NodeId>,
    ) -> HashMap<NodeId, String> {
        let dependencies_tree = lock_recoverable(&self.dependencies_tree);
        let packages = lock_recoverable(&self.packages);
        node_ids
            .filter_map(|node_id| {
                let pkg_id = &dependencies_tree.get(node_id)?.resolved_package_id;
                packages.contains_key(&**pkg_id).then(|| (node_id.clone(), pkg_id.to_string()))
            })
            .collect()
    }
}
