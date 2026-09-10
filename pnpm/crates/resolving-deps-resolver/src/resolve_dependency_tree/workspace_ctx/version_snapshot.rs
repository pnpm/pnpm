use super::{
    Arc, ChildrenOwnerEntry, DirectDep, FirstWalkMissing, HashMap, MissingNames, MutexGuard,
    OwnerMissingRecord, ResolvedTree, RunVersionsCache, WorkspaceTreeCtx, collect_newly_visited,
    fold_version, fold_visited_versions, lock_recoverable, take_locked, walk_reachable_children,
    walk_reachable_nodes,
};

impl WorkspaceTreeCtx {
    /// Snapshot the workspace context into a [`ResolvedTree`] without
    /// consuming `self`. `direct` carries the combined direct-dep
    /// envelopes the caller built up across importers; multi-importer
    /// orchestration usually leaves this empty and threads per-importer
    /// direct deps separately into [`fn@crate::resolve_peers_workspace`].
    pub fn snapshot(&self, direct: Vec<DirectDep>) -> ResolvedTree {
        ResolvedTree {
            direct,
            packages: lock_recoverable(&self.packages).clone(),
            dependencies_tree: lock_recoverable(&self.dependencies_tree).clone(),
            all_peer_dep_names: lock_recoverable(&self.all_peer_dep_names).clone(),
            policy_violations: lock_recoverable(&self.policy_violations).clone(),
            applied_patches: lock_recoverable(&self.applied_patches).clone(),
            children_by_id: lock_recoverable(&self.children_by_id)
                .iter()
                .map(|(pkg_id, recorded)| {
                    (std::sync::Arc::<str>::clone(pkg_id), Arc::clone(&recorded.edges))
                })
                .collect(),
        }
    }

    /// Snapshot only the part of the occurrence tree reachable from one
    /// importer's direct dependencies.
    ///
    /// A workspace resolve keeps every importer's occurrence nodes in
    /// this shared context, so a full [`Self::snapshot`] taken for one
    /// importer retains every other importer's nodes too. The
    /// reachable-only snapshot keeps per-importer consumers (the
    /// workspace-root hoistable-deps scan) proportional to that
    /// importer's own subtree.
    ///
    /// Realized edges provide the occurrence-node closure. Lazy edges are
    /// materialized by the peer walker from `children_by_id`, so their
    /// package closure is collected separately and included here too.
    #[must_use]
    pub fn snapshot_reachable_from(&self, direct: Vec<DirectDep>) -> ResolvedTree {
        let dependencies_tree = lock_recoverable(&self.dependencies_tree);
        let (reachable_node_ids, mut reachable_pkg_ids) =
            walk_reachable_nodes(&dependencies_tree, &direct);
        let reachable_dependencies_tree: HashMap<_, _> = reachable_node_ids
            .iter()
            .filter_map(|node_id| {
                dependencies_tree.get(node_id).cloned().map(|node| (node_id.clone(), node))
            })
            .collect();
        // Release before taking the next guard so this function never
        // holds two of the context's locks at once — `snapshot` takes
        // `packages` before `dependencies_tree`, so overlapping here
        // would create a reversed acquisition order.
        drop(dependencies_tree);
        let dependencies_tree = reachable_dependencies_tree;

        let all_children = lock_recoverable(&self.children_by_id);
        let children_by_id = walk_reachable_children(&all_children, &mut reachable_pkg_ids);
        drop(all_children);

        let packages = lock_recoverable(&self.packages);
        let packages = reachable_pkg_ids
            .into_iter()
            .filter_map(|pkg_id| packages.get(&*pkg_id).cloned().map(|pkg| (pkg_id, pkg)))
            .collect();

        ResolvedTree {
            direct,
            packages,
            dependencies_tree,
            all_peer_dep_names: lock_recoverable(&self.all_peer_dep_names).clone(),
            policy_violations: lock_recoverable(&self.policy_violations).clone(),
            applied_patches: lock_recoverable(&self.applied_patches).clone(),
            children_by_id,
        }
    }

    /// Record a walk's per-package missing-peer names. The owning
    /// importer's report is written once per ownership generation —
    /// its own later hoist waves never refresh it — and replaces any
    /// provisional report a non-owner's earlier walk left behind. See
    /// the `first_walk_missing_by_pkg` field doc.
    pub(crate) fn record_first_walk_missing(
        &self,
        importer_id: &str,
        missing_by_pkg: &HashMap<&str, MissingNames<'_>>,
    ) {
        // Lock order: `children_owner_by_id` before
        // `first_walk_missing_by_pkg`, the only place both are held.
        let owners = lock_recoverable(&self.children_owner_by_id);
        let mut record = lock_recoverable(&self.first_walk_missing_by_pkg);
        for (pkg_id, ChildrenOwnerEntry { owner, .. }) in owners.iter() {
            if owner.importer_id != importer_id {
                continue;
            }
            let recorded_by_current_owner = record
                .map()
                .get(&**pkg_id)
                .is_some_and(|entry| entry.recorded_by.as_ref() == Some(owner));
            if !recorded_by_current_owner {
                let names = missing_by_pkg
                    .get(&**pkg_id)
                    .map(|names| names.iter().map(str::to_owned).collect())
                    .unwrap_or_default();
                record.map_mut().insert(
                    std::sync::Arc::<str>::clone(pkg_id).to_string(),
                    OwnerMissingRecord { recorded_by: Some(owner.clone()), names },
                );
            }
        }
        for (pkg_id, names) in missing_by_pkg {
            if record.map().contains_key(*pkg_id) {
                continue;
            }
            if owners.get(*pkg_id).is_none_or(|entry| entry.owner.importer_id != importer_id) {
                record.map_mut().insert(
                    (*pkg_id).to_owned(),
                    OwnerMissingRecord {
                        recorded_by: None,
                        names: names.iter().map(str::to_owned).collect(),
                    },
                );
            }
        }
    }

    /// Snapshot of the per-package owner-context missing-peer names.
    /// See the `first_walk_missing_by_pkg` field doc.
    #[must_use]
    pub fn first_walk_missing_by_pkg(&self) -> Arc<FirstWalkMissing> {
        lock_recoverable(&self.first_walk_missing_by_pkg).snapshot(|record| {
            record.iter().map(|(pkg_id, entry)| (pkg_id.clone(), entry.names.clone())).collect()
        })
    }

    /// Record importer-level direct-dependency package ids as roots for
    /// [`Self::run_preferred_versions`]. Called by [`fn@extend_tree`]
    /// after each importer-level wave resolves.
    ///
    /// [`fn@extend_tree`]: super::super::extend_tree
    pub(in super::super) fn record_preferred_version_roots<'id>(
        &self,
        pkg_ids: impl Iterator<Item = &'id str>,
    ) {
        let mut roots = lock_recoverable(&self.preferred_version_roots);
        for pkg_id in pkg_ids {
            if !roots.contains(pkg_id) {
                roots.insert(pkg_id.to_string());
            }
        }
    }

    /// The `name → version` entries of every package reachable from any
    /// importer's recorded direct dependencies, shaped as the plain
    /// [`pnpm_resolving_resolver_base::PreferredVersions`] entries the
    /// peer-hoist pickers bias toward.
    ///
    /// Derived from the settled tree — the recorded roots plus the
    /// children edges the deterministic children owners recorded — not
    /// from resolution arrival order. Concurrent importer waves can
    /// transiently walk (and resolve versions inside) a subtree whose
    /// children ownership a better-placed occurrence later takes over;
    /// whether such a walk happens at all depends on thread
    /// interleaving, so an arrival-ordered fold gives the pickers
    /// run-to-run varying candidates and reshuffles peer bindings on
    /// every install. The reachable closure is interleaving-independent
    /// because both the roots and the surviving children records are.
    ///
    /// The closure is cached and extended incrementally: shared-map
    /// growth (`revision`) only ever hangs new subtrees under new
    /// roots, so previously visited packages keep their entries; a
    /// children-ownership rewrite (`children_rewrites`) can restructure
    /// existing subtrees, so it rebuilds the closure from scratch.
    pub(in super::super) fn run_preferred_versions(&self) -> MutexGuard<'_, RunVersionsCache> {
        let revision = self.revision();
        let children_rewrites = self.children_rewrites();
        let mut cache = lock_recoverable(&self.run_versions_cache);
        if cache.revision == revision && cache.children_rewrites == children_rewrites {
            return cache;
        }
        if cache.children_rewrites != children_rewrites {
            cache.visited.clear();
            cache.awaiting_identity.clear();
            cache.versions.clear();
        }
        let mut queue: Vec<String> = lock_recoverable(&self.preferred_version_roots)
            .iter()
            .filter(|pkg_id| !cache.visited.contains(*pkg_id))
            .cloned()
            .collect();
        // Lock discipline: `run_versions_cache` is locked only by this
        // function, so holding it across the refresh cannot form an
        // acquisition cycle; the context's shared maps (roots,
        // `children_by_id`, `packages`, identities) are each taken on
        // their own, never two at a time. Contention is also not a
        // concern: refreshes run at the quiescent points between hoist
        // waves, not while walks hold the shared maps hot.
        let newly_visited = {
            let children_by_id = lock_recoverable(&self.children_by_id);
            collect_newly_visited(&children_by_id, &mut queue, &mut cache.visited)
        };
        {
            let packages = lock_recoverable(&self.packages);
            fold_visited_versions(&packages, newly_visited, &mut cache);
        }
        self.fold_settled_identities(&mut cache);
        cache.revision = revision;
        cache.children_rewrites = children_rewrites;
        cache
    }

    /// Fold the versions of the workspace packages whose manifest
    /// identity has settled since the cache last looked.
    pub(super) fn fold_settled_identities(&self, cache: &mut RunVersionsCache) {
        let identities = lock_recoverable(&self.workspace_manifest_identities);
        let RunVersionsCache { awaiting_identity, versions, .. } = cache;
        awaiting_identity.retain(|pkg_id| match identities.get(pkg_id) {
            Some((name, version)) => {
                fold_version(versions, name.clone(), version.clone());
                false
            }
            None => true,
        });
    }

    /// Record the manifest identity of a `name_ver`-less package wanted
    /// through a non-path `workspace:` specifier, for
    /// [`Self::run_preferred_versions`] to fold once the package is
    /// reachable.
    pub(in super::super) fn record_workspace_manifest_identity(
        &self,
        pkg_id: &str,
        name: &str,
        version: &str,
    ) {
        lock_recoverable(&self.workspace_manifest_identities)
            .entry(pkg_id.to_string())
            .or_insert_with(|| (name.to_string(), version.to_string()));
    }

    /// Take ownership of `self` and emit the final [`ResolvedTree`].
    /// Pacquet's single-importer path consumes the context via
    /// [`crate::TreeCtx::into_resolved_tree`], which routes through here once
    /// the last `Arc<WorkspaceTreeCtx>` reference is the [`crate::TreeCtx`]'s
    /// own.
    pub fn into_resolved_tree(mut self, direct: Vec<DirectDep>) -> ResolvedTree {
        let tree = ResolvedTree {
            direct,
            packages: take_locked(&mut self.packages),
            dependencies_tree: take_locked(&mut self.dependencies_tree),
            all_peer_dep_names: take_locked(&mut self.all_peer_dep_names),
            policy_violations: take_locked(&mut self.policy_violations),
            applied_patches: take_locked(&mut self.applied_patches),
            children_by_id: take_locked(&mut self.children_by_id)
                .into_iter()
                .map(|(pkg_id, recorded)| (pkg_id, recorded.edges))
                .collect(),
        };
        // The per-edge dedup caches hold an entry per resolved wanted
        // dependency; freeing a workspace-scale map costs long enough
        // to show up in the install's tail, and nothing reads them
        // again, so a background thread takes the drop off the
        // critical path.
        let dedup_caches = (
            take_locked(&mut self.resolved_by_wanted),
            take_locked(&mut self.resolved_workspace_by_wanted),
            take_locked(&mut self.resolved_workspace_final_by_wanted),
            take_locked(&mut self.children_specs_by_id),
        );
        pnpm_fs::background_drop(dedup_caches);
        tree
    }
}
