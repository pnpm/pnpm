use super::{ImporterPeerInput, InitializedImporters, PassSettings, PeerInputs, ResolvedTree};
use crate::resolve_peers::{
    ResolvePeersOptions, WorkspacePeerSettings, WorkspaceResolvePeersResult,
    resolve_peers_workspace,
};

pub(super) fn importer_peer_inputs(initialized: InitializedImporters) -> PeerInputs {
    let mut per_importer = Vec::with_capacity(initialized.importer_ids.len());
    let mut hoisted_provider_node_ids = std::collections::HashSet::default();
    for ((id, state), (project_dir, modules_dir)) in initialized.importer_ids
        .into_iter()
        .zip(initialized.states)
        .zip(initialized.input_dirs)
    {
        let (direct, importer_provider_node_ids) = state.into_direct();
        hoisted_provider_node_ids.extend(importer_provider_node_ids);
        per_importer.push(ImporterPeerInput { id, direct, root_dir: project_dir, modules_dir });
    }
    PeerInputs { per_importer, hoisted_provider_node_ids }
}

pub(super) fn resolve_workspace_peers(
    settings: &PassSettings,
    tree: &mut ResolvedTree,
    inputs: PeerInputs,
) -> WorkspaceResolvePeersResult {
    resolve_peers_workspace(
        tree,
        &inputs.per_importer,
        &settings.peers.lockfile_dir,
        WorkspacePeerSettings {
            dedupe_injected_deps: settings.peers.dedupe_injected_deps,
            dedupe_peer_dependents: settings.peers.dedupe_peer_dependents,
            resolve_peers_from_workspace_root: settings.peers.resolve_peers_from_workspace_root,
        },
        ResolvePeersOptions {
            peers_suffix_max_length: settings.peers.peers_suffix_max_length,
            dedupe_peers: settings.peers.dedupe_peers,
            project_dir: None,
            links: crate::PeerLinkOptions {
                exclude_links_from_lockfile: settings.peers.exclude_links_from_lockfile,
                lockfile_dir: Some(settings.peers.lockfile_dir.clone()),
                // Per-importer; resolve_peers_workspace swaps the
                // ImporterPeerInput's modules_dir into walker.opts before each
                // importer's walk.
                modules_dir: None,
            },
            scope: crate::PeerResolutionScope {
                hoist_missing_scope: None,
                hoisted_peer_provider_node_ids: inputs.hoisted_provider_node_ids,
                ..Default::default()
            },
        },
    )
}
