use super::{
    Arc, BTreeMap, DependencyOverrider, DirectDep, DirectSeeds, HashMap, HashSet, HoistSettings,
    ImporterHoistState, LockedPeers, MissingPeer, PreferredVersions, TreeCtx, WorkspaceRootDep,
};
use pnpm_resolving_resolver_base::DIRECT_DEP_SELECTOR_WEIGHT;

pub(super) struct ImporterHoistPolicy {
    pub(super) peers: crate::ImporterPeerOptions,
    pub(super) peers_suffix_max_length: usize,
}

impl ImporterHoistPolicy {
    pub(super) fn should_hoist_peers(&self) -> bool {
        self.peers.auto_install_peers || self.peers.dedupe_peer_dependents.dedupes()
    }
}

#[derive(Default)]
enum RequiredPeerPhase {
    #[default]
    CurrentProviders,
    LockedFallbacks,
}

#[derive(Default)]
pub(super) struct ImporterHoistProgress {
    required_peer_phase: RequiredPeerPhase,
    /// Whether the last required round converged with no missing
    /// required peers left. A converged importer's next required round
    /// is a no-op unless its inputs changed since: an optional hoist
    /// extended its direct set, or a workspace children-ownership
    /// rewrite restructured shared subtrees. A round that broke off
    /// with unhoistable misses is *not* converged — another importer's
    /// resolutions can extend the run-resolved preferred versions and
    /// make those misses hoistable, so it must re-discover every round.
    pub(super) discovery_converged: bool,
    /// the children-rewrite counter in [`crate::WorkspaceTreeCtx::tree`] at the moment
    /// [`Self::discovery_converged`] was set.
    pub(super) converged_children_rewrites: u64,
    /// How many entries of [`ImporterHoistDependencies::direct`] previous rounds' discovery
    /// walks covered. A later round walks only the direct deps added
    /// since — the earlier entries' subtrees are unchanged, so their
    /// (scope-filtered) missing reports are replayed from
    /// [`Self::merged_missing`] instead of re-walked.
    pub(super) walked_direct_len: usize,
    /// the children-rewrite counter in [`crate::WorkspaceTreeCtx::tree`] at the last
    /// discovery walk. A rewrite restructures shared subtrees, so the
    /// next walk covers the whole direct forest again.
    pub(super) walked_children_rewrites: u64,
    /// Scope-filtered missing-peer issues accumulated across this
    /// importer's discovery walks since its last full walk. Entries
    /// whose peer was hoisted stay behind and are filtered by
    /// [`fn@super::missing_peers::partition_missing_peers`]'s alias check.
    pub(super) merged_missing: HashMap<String, Vec<MissingPeer>>,
}

pub(super) struct ImporterHoistDependencies {
    pub(super) direct: Vec<DirectDep>,
    /// Empty until the caller assigns it; see
    /// [`crate::ImporterPeerOptions::resolve_peers_from_workspace_root`].
    pub(super) workspace_root_deps: Arc<Vec<WorkspaceRootDep>>,
    /// `alias → version` of the workspace root's direct dependencies,
    /// assigned together with `workspace_root_deps`.
    pub(super) workspace_root_dep_versions: Arc<HashMap<String, String>>,
    /// `alias → bare_specifier` as declared. Stands in wherever the
    /// resolver reports no normalized form — a plain `"19.2.0"` arrives
    /// as `None`.
    pub(super) wanted_specifier_by_alias: BTreeMap<String, String>,
    /// `NodeIds` appended to `direct` by
    /// [`ImporterHoistState::append_resolved_peer_providers`]. Threaded into
    /// [`crate::PeerResolutionScope::hoisted_peer_provider_node_ids`] so the
    /// peer walk resolves them at their tree position instead of the
    /// importer root.
    pub(super) hoisted_peer_provider_node_ids: HashSet<crate::NodeId>,
    pub(super) parent_pkg_aliases: HashSet<String>,
    pub(super) all_missing_optional_peers: BTreeMap<String, Vec<String>>,
    /// See [`crate::ImporterPeerInput::rejected_optional_peers`].
    pub(super) rejected_optional_peers: HashSet<String>,
}

pub(super) struct ImporterHoistSelection {
    /// The lockfile + manifest preferred-versions seed. The hoist
    /// pickers merge it per lookup with the workspace-wide
    /// run-resolved versions — see
    /// [`TreeCtx::preferred_versions_for_names`] — instead of
    /// maintaining a per-importer copy of the whole run history.
    pub(super) preferred_versions: Arc<PreferredVersions>,
    pub(super) locked_versions: Arc<HashMap<String, HashSet<String>>>,
    pub(super) override_bare_specifier: Option<Arc<DependencyOverrider>>,
}

impl super::ImporterHoistState {
    /// Enable lockfile-only required providers after current-graph hoists settle.
    pub(crate) fn enable_locked_peer_fallbacks(&mut self) -> bool {
        if matches!(self.progress.required_peer_phase, RequiredPeerPhase::LockedFallbacks)
            || !self.policy.should_hoist_peers()
            || self.policy.peers.auto_install_peers
            || self.ctx
                .workspace()
                .wanted_lockfile()
                .is_none()
            || (self.progress.discovery_converged
                && self.dependencies.all_missing_optional_peers.is_empty())
        {
            return false;
        }
        self.progress.required_peer_phase = RequiredPeerPhase::LockedFallbacks;
        true
    }

    /// With auto-install disabled, retain manifest preferences, current providers,
    /// then admit this importer's recorded pins once current-graph hoists settle.
    pub(super) fn required_peer_preferred_versions<'name>(
        &self,
        names: impl Iterator<Item = &'name str>,
        resolved: &HashMap<String, HashSet<String>>,
    ) -> PreferredVersions {
        let mut preferred =
            self.ctx.preferred_versions_for_names(&self.selection.preferred_versions, names);
        if self.policy.peers.auto_install_peers {
            return preferred;
        }
        let Some(lockfile) = self.ctx.workspace().wanted_lockfile() else {
            return preferred;
        };
        let locked = match self.progress.required_peer_phase {
            RequiredPeerPhase::CurrentProviders => None,
            RequiredPeerPhase::LockedFallbacks => lockfile.importers
                .contains_key(&self.importer_id)
                .then_some(&self.selection.locked_versions),
        };
        preferred.retain(|name, versions| {
            versions.retain(|version, entry| {
                entry.weight() >= DIRECT_DEP_SELECTOR_WEIGHT
                    || resolved
                        .get(name)
                        .is_some_and(|versions| versions.contains(version))
                    || locked
                        .and_then(|locked| locked.get(name))
                        .is_some_and(|versions| versions.contains(version))
            });
            !versions.is_empty()
        });
        preferred
    }

    pub(super) fn assemble(
        importer_id: &str,
        ctx: TreeCtx,
        direct: Vec<DirectDep>,
        seeds: DirectSeeds,
        locked: LockedPeers,
        settings: HoistSettings,
    ) -> Self {
        ImporterHoistState {
            importer_id: importer_id.to_string(),
            ctx,
            project_dir: settings.project_dir,
            policy: ImporterHoistPolicy {
                peers: settings.peers,
                peers_suffix_max_length: settings.peers_suffix_max_length,
            },
            links: settings.links,
            progress: ImporterHoistProgress::default(),
            dependencies: ImporterHoistDependencies {
                direct,
                workspace_root_deps: Arc::default(),
                workspace_root_dep_versions: Arc::default(),
                wanted_specifier_by_alias: seeds.wanted_specifier_by_alias,
                hoisted_peer_provider_node_ids: HashSet::default(),
                parent_pkg_aliases: seeds.parent_pkg_aliases,
                all_missing_optional_peers: BTreeMap::new(),
                rejected_optional_peers: HashSet::default(),
            },
            selection: ImporterHoistSelection {
                preferred_versions: settings.all_preferred_versions,
                locked_versions: locked.versions,
                override_bare_specifier: settings.override_bare_specifier,
            },
        }
    }
}
