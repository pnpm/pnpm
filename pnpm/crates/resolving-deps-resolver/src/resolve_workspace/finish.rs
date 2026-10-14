//! Assembling [`super::ResolveWorkspaceResult`]: fold the importers into
//! peer inputs, reclaim the shared tree, drop the broken optionals and run
//! the workspace-wide peer pass.

use super::{
    ResolveWorkspaceResult, broken_optional, importers::InitializedImporters, options::PassSettings,
};
use crate::{
    resolve_dependency_tree::WorkspaceTreeCtx,
    resolve_importer::ResolveImporterError,
    resolve_peers::{
        ImporterPeerInput, ResolvePeersOptions, WorkspacePeerSettings, WorkspaceResolvePeersResult,
        resolve_peers_workspace,
    },
    resolved_tree::ResolvedTree,
};
use std::{collections::BTreeMap, sync::Arc};

/// Fold every importer's direct deps into the peer inputs, reclaim the
/// tree and run the workspace-wide peer pass.
pub(super) fn finish(
    settings: &PassSettings,
    workspace: Arc<WorkspaceTreeCtx>,
    mut initialized: InitializedImporters,
    time: BTreeMap<String, String>,
) -> Result<ResolveWorkspaceResult, ResolveImporterError> {
    let broken = workspace.take_broken_packages();
    let skipped_optional_log = workspace.skipped_optional_log();
    let optional_direct = std::mem::take(&mut initialized.optional_direct);
    let mut peer_inputs = importer_peer_inputs(initialized);
    // Reclaim the workspace ctx now that every importer's state has
    // dropped its `Arc<WorkspaceTreeCtx>`. The `try_unwrap` succeeds
    // when this is the sole remaining `Arc` reference (the common
    // case); the fallback snapshots out via the shared `Arc` for
    // parity.
    let mut merged_tree = match Arc::try_unwrap(workspace) {
        Ok(ws) => ws.into_resolved_tree(Vec::new()),
        Err(arc) => arc.snapshot(Vec::new()),
    };
    let left_out = broken_optional::drop_broken_optional_dependencies(
        &mut merged_tree,
        &mut peer_inputs.per_importer,
        &optional_direct,
        broken,
        &broken_optional::SkipReport {
            log: skipped_optional_log.as_ref(),
            lockfile_dir: &settings.peers.lockfile_dir,
        },
    )?;
    let time = broken_optional::forget_left_out(&mut merged_tree, &left_out, time);
    let peers = resolve_workspace_peers(settings, &mut merged_tree, peer_inputs);
    Ok(ResolveWorkspaceResult { merged_tree, peers, time })
}

struct PeerInputs {
    per_importer: Vec<ImporterPeerInput>,
    hoisted_provider_node_ids: std::collections::HashSet<crate::NodeId, rustc_hash::FxBuildHasher>,
}

fn importer_peer_inputs(initialized: InitializedImporters) -> PeerInputs {
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

fn resolve_workspace_peers(
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
