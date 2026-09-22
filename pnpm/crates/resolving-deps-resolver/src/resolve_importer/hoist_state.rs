use super::{
    Arc,
    BTreeMap,
    DependencyOverrider,
    DirectDep,
    DirectSeeds,
    HashMap,
    HashSet,
    HoistSettings,
    ImporterHoistState,
    LockedPeers,
    MissingPeer,
    PreferredVersions,
    TreeCtx,
    WorkspaceRootDep,
};

pub(super) struct ImporterHoistPolicy {
    pub(super) peers: crate::ImporterPeerOptions,
    pub(super) peers_suffix_max_length: usize,
}

impl ImporterHoistPolicy {
    pub(super) fn should_hoist_peers(&self) -> bool {
        self.peers.auto_install_peers || self.peers.dedupe_peer_dependents
    }
}

#[derive(Default)]
pub(super) struct ImporterHoistProgress {
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
}

pub(super) struct ImporterHoistSelection {
    /// The lockfile + manifest preferred-versions seed. The hoist
    /// pickers merge it per lookup with the workspace-wide
    /// run-resolved versions — see
    /// [`TreeCtx::preferred_versions_for_names`] — instead of
    /// maintaining a per-importer copy of the whole run history.
    pub(super) preferred_versions: Arc<PreferredVersions>,
    pub(super) locked_names: Arc<HashSet<String>>,
    pub(super) locked_versions: Arc<HashMap<String, HashSet<String>>>,
    pub(super) override_bare_specifier: Option<Arc<DependencyOverrider>>,
}

impl super::ImporterHoistState {
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
                wanted_specifier_by_alias: seeds.wanted_specifier_by_alias,
                hoisted_peer_provider_node_ids: HashSet::default(),
                parent_pkg_aliases: seeds.parent_pkg_aliases,
                all_missing_optional_peers: BTreeMap::new(),
            },
            selection: ImporterHoistSelection {
                preferred_versions: settings.all_preferred_versions,
                locked_names: locked.names,
                locked_versions: locked.versions,
                override_bare_specifier: settings.override_bare_specifier,
            },
        }
    }
}
