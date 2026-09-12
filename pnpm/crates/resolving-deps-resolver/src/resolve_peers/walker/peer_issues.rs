use super::{
    Arc, ComparablePeerRange, DepPath, HashMap, MissingPeer, MissingPeerInfo, NodeId, ParentChain,
    ParentPkgInfo, ParentRefs, PeerDep, PeerDependencyIssue, PeerId, ResolvedPackage, SharedChain,
    Walker, link_node_id_as_dep_path, link_path_to_peer_version, peer_id_pair,
};

impl Walker<'_> {}

impl Walker<'_> {
    /// `true` when a missing-peer issue for `peer_name` under the
    /// given ancestor chain must not be emitted for the hoist input.
    /// See [`ResolvePeersOptions::hoist_missing_scope`](crate::ResolvePeersOptions::hoist_missing_scope).
    pub(in super::super) fn missing_issue_suppressed(
        &self,
        ancestor_pkg_ids: &SharedChain<String>,
        peer_name: &str,
    ) -> bool {
        let Some(scope) = self.opts.hoist_missing_scope.as_ref() else { return false };
        scope.suppresses_iter(ancestor_pkg_ids.iter(), peer_name)
    }

    pub(in super::super) fn record_missing_issue(
        &mut self,
        peer_name: &str,
        issue: MissingPeer,
        ancestor_pkg_ids: &SharedChain<String>,
    ) {
        if self.in_canonical_drain {
            return;
        }
        self.issues.missing.entry(peer_name.to_string()).or_default().push(issue);
        if self.discovery {
            self.missing_ancestor_pkg_ids
                .entry(peer_name.to_string())
                .or_default()
                .push(ancestor_pkg_ids.clone());
        }
    }

    pub(in super::super) fn issue_parents(&self, chain: &SharedChain<String>) -> ParentChain {
        if self.discovery { ParentChain::default() } else { ParentChain(chain.clone()) }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "internal walker helper threading per-node context, mirrors the resolve_node parameter set"
    )]
    pub(super) fn resolve_one_peer(
        &mut self,
        peer_name: &str,
        peer_dep: &PeerDep,
        parent_refs: &ParentRefs,
        chain: &SharedChain<String>,
        ancestor_pkg_ids: &SharedChain<String>,
        resolved: &mut HashMap<String, NodeId>,
        missing: &mut HashMap<String, MissingPeerInfo>,
    ) {
        let raw_range = peer_dep.version.as_str();
        // The stored range keeps the original scheme (only `workspace:` is
        // stripped) so it still selects the package to auto-install for a
        // missing peer, e.g. `work:5.x.x` fetches from the `work` registry.
        let range_for_match = raw_range.strip_prefix("workspace:").unwrap_or(raw_range);
        // The satisfaction check needs a comparable semver range, so
        // named-registry/`npm:` bodies are extracted and opaque specs become `*`.
        let comparable_range = self.comparable_peer_range(raw_range);
        let optional = peer_dep.optional;

        match parent_refs.get(peer_name) {
            None => {
                missing.insert(
                    peer_name.to_string(),
                    MissingPeerInfo { range: range_for_match.to_string(), optional },
                );
                self.record_missing_peer_if_needed(
                    peer_name,
                    peer_dep,
                    chain,
                    ancestor_pkg_ids,
                    &comparable_range,
                );
            }
            Some(parent) => {
                if !comparable_range.satisfies(&parent.version) && !self.in_canonical_drain {
                    let parents = self.issue_parents(chain);
                    self.issues.bad.entry(peer_name.to_string()).or_default().push(
                        PeerDependencyIssue {
                            wanted_range: comparable_range.text.clone(),
                            found_version: parent.version.clone(),
                            optional,
                            parents,
                            resolved_from: ParentChain::default(),
                        },
                    );
                }
                if let Some(parent_node_id) = parent.node_id.as_ref() {
                    resolved.insert(peer_name.to_string(), parent_node_id.clone());
                }
            }
        }
    }

    pub(super) fn record_missing_peer_if_needed(
        &mut self,
        peer_name: &str,
        peer_dep: &PeerDep,
        chain: &SharedChain<String>,
        ancestor_pkg_ids: &SharedChain<String>,
        comparable_range: &ComparablePeerRange,
    ) {
        let raw_range = peer_dep.version.as_str();
        let range_for_match = raw_range.strip_prefix("workspace:").unwrap_or(raw_range);
        let optional = peer_dep.optional;
        if !self.missing_issue_suppressed(ancestor_pkg_ids, peer_name) {
            self.record_missing_issue(
                peer_name,
                MissingPeer {
                    wanted_range: comparable_range.text.clone(),
                    raw_range: range_for_match.to_string(),
                    optional,
                    parents: self.issue_parents(chain),
                },
                ancestor_pkg_ids,
            );
        }
    }

    /// Build the [`PeerId`] contribution for one resolved peer.
    ///
    /// Precedence:
    ///
    /// 1. **`link:<rel>` `NodeIds`** — emit
    ///    `PeerId::Pair { name: peer_alias, version: link_path_to_peer_version(rel) }`
    ///    so the peer-suffix segment reads as `name@encoded_path`
    ///    instead of carrying the raw link target. This branch fires
    ///    for both workspace-link parents and the
    ///    `excludeLinksFromLockfile` remap that points the parent at
    ///    `link:node_modules/<alias>`.
    /// 2. **`dedupe_peers` enabled** — emit `name@version` from the
    ///    resolved package so recursive peer suffixes collapse like
    ///    `(foo@1.0.0(bar@2.0.0))` → `(foo@1.0.0)`.
    /// 3. **The peer's `DepPath`** once it has been walked —
    ///    `node_dep_paths` lookup, emitted as [`PeerId::DepPath`].
    /// 4. **Cycle fallback** — `name@version` from the resolved package,
    ///    emitted as [`PeerId::Pair`].
    pub(super) fn build_peer_id(&self, peer_alias: &str, peer_node_id: &NodeId) -> PeerId {
        if let NodeId::Leaf(id) = peer_node_id
            && let Some(rel) = id.strip_prefix("link:")
        {
            return PeerId::Pair {
                name: peer_alias.to_string(),
                version: link_path_to_peer_version(rel),
            };
        }
        if self.opts.dedupe_peers
            && let Some(tree_node) = self.tree.dependencies_tree.get(peer_node_id)
            && let Some(pkg) = self.tree.packages.get(&tree_node.resolved_package_id)
        {
            return peer_id_pair(&pkg.result);
        }
        if let Some(dep_path) = self.node_dep_paths.get(peer_node_id) {
            return PeerId::DepPath(dep_path.clone());
        }
        let tree_node = &self.tree.dependencies_tree[peer_node_id];
        let pkg = &self.tree.packages[&tree_node.resolved_package_id];
        peer_id_pair(&pkg.result)
    }

    /// Resolve `node_id` to the depPath computed during the main walk.
    /// Peerless nodes are already final at this point; nodes with peers
    /// may still be missing a pending peer provider's own final suffix.
    pub(in super::super) fn provisional_dep_path_of(&self, node_id: &NodeId) -> DepPath {
        if let Some(dep_path) = self.node_dep_paths.get(node_id) {
            return dep_path.clone();
        }
        if let Some(dep_path) = link_node_id_as_dep_path(node_id) {
            return dep_path;
        }
        let pkg_id = &self.tree.dependencies_tree[node_id].resolved_package_id;
        DepPath::from(std::sync::Arc::<str>::clone(&self.tree.packages[pkg_id].id))
    }

    pub(in super::super) fn remember_parent_context_if_peer_provider(
        &mut self,
        alias: &str,
        node_id: &NodeId,
        parent_context: &Arc<HashMap<String, ParentPkgInfo>>,
    ) {
        let Some(tree_node) = self.tree.dependencies_tree.get(node_id) else { return };
        let Some(pkg) = self.tree.packages.get(&tree_node.resolved_package_id) else { return };
        if self.is_peer_relevant(alias, pkg) {
            self.parent_pkgs_of_node.insert(node_id.clone(), Arc::clone(parent_context));
        }
    }

    pub(super) fn owned_package(&mut self, pkg_id: &str) -> Arc<ResolvedPackage> {
        if let Some(pkg) = self.packages_by_id.get(pkg_id) {
            return Arc::clone(pkg);
        }
        let pkg = Arc::new(self.tree.packages[pkg_id].clone());
        self.packages_by_id.insert(pkg_id.to_string(), Arc::clone(&pkg));
        pkg
    }

    pub(in super::super) fn remember_resolved_node(
        &mut self,
        node_id: &NodeId,
        dep_path: &DepPath,
    ) {
        let retain = !self.discovery
            || self.parent_pkgs_of_node.contains_key(node_id)
            || self.opts.hoisted_peer_provider_node_ids.contains(node_id);
        if !retain {
            return;
        }
        self.node_dep_paths.insert(node_id.clone(), dep_path.clone());
        self.visited_this_call.insert(node_id.clone());
    }
}
