use super::{
    Arc, BTreeMap, DirectDep, HashMap, HashSet, HoistMissingScope, HoistPeersOptions,
    ImporterHoistState, MissingPeerInfo, ParentPkgAliases, PeerDiscoveryResult, PeerHoistDiscovery,
    RequiredRound, ResolveImporterError, Resolver, WantedSpec, WorkspaceRootDep,
    apply_hoist_missing_scope, extend_tree, get_hoistable_optional_peers_with_locked_versions,
    hoist_peers, index_missing_names, partition_missing_peers,
};

impl ImporterHoistState {
    /// Resolve the importer's missing *required* peers to a fixpoint,
    /// rebuilding the missing-*optional* buckets the round's
    /// [`Self::hoist_optional_round`] consumes. No-op when nothing may
    /// be hoisted (see [`ImporterHoistState::hoist_peers`]); the final
    /// peer pass still reports every unmet peer as a warning.
    pub(crate) async fn run_required_round<Chain>(
        &mut self,
        resolver: &Chain,
        peer_discovery: &mut PeerHoistDiscovery,
    ) -> Result<(), ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        if !self.hoist_peers {
            return Ok(());
        }
        if self.discovery_converged
            && self.ctx.workspace().children_rewrites() == self.converged_children_rewrites
        {
            // See [`Self::discovery_converged`]: re-discovering an
            // unchanged importer reproduces the state its last round
            // converged to. Skipping keeps `all_missing_optional_peers`
            // intact for the next optional round.
            return Ok(());
        }
        self.begin_required_round();
        self.complete_required_round(resolver, None, peer_discovery).await
    }

    pub(crate) fn prepare_initial_required_round(
        &mut self,
        peer_discovery: &mut PeerHoistDiscovery,
    ) -> Option<RequiredRound> {
        if !self.hoist_peers {
            return None;
        }
        self.begin_required_round();
        Some(self.resolve_required_round(None, peer_discovery))
    }

    /// The caller snapshots the two context maps once per barrier and
    /// shares them across every importer's scope.
    pub(crate) fn apply_owner_missing_scope(
        &self,
        round: &mut RequiredRound,
        first_importer_by_pkg: &Arc<HashMap<String, String>>,
        first_walk_missing_by_pkg: &Arc<HashMap<String, HashSet<String>>>,
    ) {
        apply_hoist_missing_scope(
            &mut round.discovery,
            &HoistMissingScope {
                importer_id: self.importer_id.clone(),
                first_importer_by_pkg: Arc::clone(first_importer_by_pkg),
                first_walk_missing_by_pkg: Arc::clone(first_walk_missing_by_pkg),
                locked_peer_names: Arc::clone(&self.locked_peer_names),
            },
        );
    }

    pub(crate) async fn complete_initial_required_round<Chain>(
        &mut self,
        resolver: &Chain,
        round: RequiredRound,
        peer_discovery: &mut PeerHoistDiscovery,
    ) -> Result<(), ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        self.complete_required_round(resolver, Some(round), peer_discovery).await
    }

    pub(super) fn begin_required_round(&mut self) {
        // The hoist input must not see missing peers declared inside a
        // subtree owned by another importer's shared children context —
        // the owner walk's children report is reused there, so those
        // peers never reach a non-owner importer's hoist. The final peer
        // pass keeps the unscoped options so warnings stay complete.
        self.all_missing_optional_peers.clear();
    }

    pub(super) fn resolve_required_round(
        &mut self,
        hoist_missing_scope: Option<Arc<HoistMissingScope>>,
        peer_discovery: &mut PeerHoistDiscovery,
    ) -> RequiredRound {
        let children_rewrites = self.ctx.workspace().children_rewrites();
        let walk_was_full =
            self.walked_direct_len == 0 || children_rewrites != self.walked_children_rewrites;
        let walk_from = if walk_was_full { 0 } else { self.walked_direct_len };
        let discovery = {
            let mut opts = self.peers_opts();
            opts.hoist_missing_scope = hoist_missing_scope;
            peer_discovery.discover(
                self.ctx.workspace(),
                &self.direct,
                &self.direct[walk_from..],
                opts,
            )
        };
        self.walked_direct_len = self.direct.len();
        self.walked_children_rewrites = children_rewrites;
        let provider_pkg_ids = self
            .ctx
            .workspace()
            .provider_pkg_ids(discovery.resolved_peer_providers_by_alias.values());
        self.ctx.workspace().record_first_walk_missing(
            &self.importer_id,
            &index_missing_names(&discovery.missing_summaries),
        );
        RequiredRound { provider_pkg_ids, discovery, walk_was_full }
    }

    pub(super) async fn complete_required_round<Chain>(
        &mut self,
        resolver: &Chain,
        mut first_round: Option<RequiredRound>,
        peer_discovery: &mut PeerHoistDiscovery,
    ) -> Result<(), ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        loop {
            let round = match first_round.take() {
                Some(round) => round,
                None => self.next_required_round(peer_discovery),
            };
            self.merge_missing_issues(&round.discovery, round.walk_was_full);
            let (missing_required, fresh_optional) = partition_missing_peers(
                &self.merged_missing,
                &self.parent_pkg_aliases,
                self.auto_install_peers_from_highest_match,
            );
            self.append_resolved_peer_providers(
                &round.discovery.resolved_peer_providers_by_alias,
                &round.provider_pkg_ids,
                &missing_required,
            );
            self.merge_fresh_optional_peers(fresh_optional);

            if missing_required.is_empty() {
                self.discovery_converged = true;
                self.converged_children_rewrites = self.ctx.workspace().children_rewrites();
                break;
            }
            if !self.hoist_missing_required(resolver, &missing_required).await? {
                break;
            }
        }
        Ok(())
    }

    /// Hoist the missing required peers to the importer level and resolve
    /// them as direct deps. `false` when nothing could be hoisted.
    ///
    /// Both hoists bias toward the run-resolved preferred versions: the
    /// seed buckets for the missing names merged with every version
    /// resolved into the settled tree so far.
    pub(super) async fn hoist_missing_required<Chain>(
        &mut self,
        resolver: &Chain,
        missing_required: &BTreeMap<String, MissingPeerInfo>,
    ) -> Result<bool, ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        let missing_as_pairs: Vec<(String, MissingPeerInfo)> =
            missing_required.iter().map(|(n, info)| (n.clone(), info.clone())).collect();
        let hoist_preferred = self.ctx.preferred_versions_for_names(
            &self.preferred_versions_seed,
            missing_as_pairs.iter().map(|(name, _)| name.as_str()),
        );
        let hoisted = hoist_peers(
            &HoistPeersOptions {
                auto_install_peers: self.auto_install_peers,
                all_preferred_versions: &hoist_preferred,
                workspace_root_deps: self.hoist_root_deps(),
                override_bare_specifier: self.override_bare_specifier.as_deref(),
                project_dir: &self.project_dir,
            },
            &missing_as_pairs,
        );
        if hoisted.is_empty() {
            return Ok(false);
        }

        for name in hoisted.keys() {
            self.parent_pkg_aliases.insert(name.clone());
        }

        // Hoisted required peers are installed at the importer
        // level as non-optional direct deps — they exist precisely
        // to satisfy a missing required peer, so flipping their
        // own `optional` flag to `true` would defeat the
        // auto-install. Hoisted peers don't carry
        // `dependenciesMeta` from any manifest, so `injected`
        // defaults to `false`: the hoist path constructs a fresh
        // wanted dependency without threading the per-dep meta.
        let new_wanted: Vec<WantedSpec> =
            hoisted.into_iter().map(|(name, range)| (name, range, false, false)).collect();
        let new_direct = extend_tree(
            &self.ctx,
            resolver,
            new_wanted,
            &self.importer_id,
            &ParentPkgAliases::root(self.parent_pkg_aliases.clone()),
        )
        .await?;
        self.direct.extend(new_direct);
        Ok(true)
    }

    /// The workspace root's own dependencies, when peers resolve from there.
    /// They bound what a hoist may install.
    pub(super) fn hoist_root_deps(&self) -> &[WorkspaceRootDep] {
        if self.resolve_peers_from_workspace_root { &self.workspace_root_deps } else { &[] }
    }

    pub(super) fn next_required_round(
        &mut self,
        peer_discovery: &mut PeerHoistDiscovery,
    ) -> RequiredRound {
        self.resolve_required_round(
            Some(Arc::new(HoistMissingScope {
                importer_id: self.importer_id.clone(),
                first_importer_by_pkg: self.ctx.workspace().first_importer_by_pkg(),
                first_walk_missing_by_pkg: self.ctx.workspace().first_walk_missing_by_pkg(),
                locked_peer_names: Arc::clone(&self.locked_peer_names),
            })),
            peer_discovery,
        )
    }

    /// A full walk reports every missing peer again, so its issues replace the
    /// accumulated ones instead of adding to them.
    pub(super) fn merge_missing_issues(
        &mut self,
        discovery: &PeerDiscoveryResult,
        walk_was_full: bool,
    ) {
        if walk_was_full {
            self.merged_missing.clear();
        }
        for (peer_name, issues) in &discovery.peer_dependency_issues.missing {
            self.merged_missing
                .entry(peer_name.clone())
                .or_default()
                .extend(issues.iter().cloned());
        }
    }

    pub(super) fn merge_fresh_optional_peers(
        &mut self,
        fresh_optional: BTreeMap<String, Vec<String>>,
    ) {
        for (name, ranges) in fresh_optional {
            let bucket = self.all_missing_optional_peers.entry(name).or_default();
            for range in ranges {
                if !bucket.iter().any(|existing| existing == &range) {
                    bucket.push(range);
                }
            }
        }
    }

    pub(super) fn append_resolved_peer_providers(
        &mut self,
        providers: &BTreeMap<String, crate::NodeId>,
        provider_pkg_ids: &HashMap<crate::NodeId, String>,
        missing_required: &BTreeMap<String, MissingPeerInfo>,
    ) {
        if !self.auto_install_peers {
            return;
        }
        for (alias, node_id) in providers {
            if self.parent_pkg_aliases.contains(alias) || missing_required.contains_key(alias) {
                continue;
            }
            let Some(pkg_id) = provider_pkg_ids.get(node_id) else {
                continue;
            };
            self.direct.push(DirectDep {
                alias: alias.clone(),
                node_id: node_id.clone(),
                id: pkg_id.clone(),
            });
            self.hoisted_peer_provider_node_ids.insert(node_id.clone());
            self.parent_pkg_aliases.insert(alias.clone());
        }
    }

    /// Hoist this round's missing optional peers; `true` when any were
    /// installed (the workspace runs another round). No-op when nothing
    /// may be hoisted (see [`ImporterHoistState::hoist_peers`]).
    pub(crate) async fn hoist_optional_round<Chain>(
        &mut self,
        resolver: &Chain,
    ) -> Result<bool, ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        if !self.hoist_peers || self.all_missing_optional_peers.is_empty() {
            return Ok(false);
        }
        let hoist_preferred = self.ctx.preferred_versions_for_names(
            &self.preferred_versions_seed,
            self.all_missing_optional_peers.keys().map(String::as_str),
        );
        let hoisted_optional = get_hoistable_optional_peers_with_locked_versions(
            &self.all_missing_optional_peers,
            &hoist_preferred,
            self.hoist_root_deps(),
            &self.locked_peer_versions,
        );
        if hoisted_optional.is_empty() {
            return Ok(false);
        }
        for name in hoisted_optional.keys() {
            self.parent_pkg_aliases.insert(name.clone());
        }
        // Optional peers picked up via `getHoistableOptionalPeers` are
        // also installed at the importer level — the picker already
        // confirmed a preferred version is in scope. Treating them as
        // non-optional matches the required-peer arm above; `injected`
        // also defaults to `false` for the same reason.
        let new_wanted: Vec<WantedSpec> =
            hoisted_optional.into_iter().map(|(name, range)| (name, range, false, false)).collect();
        let new_direct = extend_tree(
            &self.ctx,
            resolver,
            new_wanted,
            &self.importer_id,
            &ParentPkgAliases::root(self.parent_pkg_aliases.clone()),
        )
        .await?;
        self.direct.extend(new_direct);
        // The direct set changed; the next required round must
        // re-discover so the hoisted names leave the missing buckets.
        self.discovery_converged = false;
        Ok(true)
    }
}
