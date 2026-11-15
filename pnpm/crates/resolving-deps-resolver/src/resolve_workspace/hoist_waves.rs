//! The workspace-wide hoist barrier: required rounds, then optional-peer
//! waves repeated across every importer until none hoists.

use crate::{
    ResolutionCancel,
    resolve_dependency_tree::{ResolveDependencyTreeError, WorkspaceTreeCtx},
    resolve_importer::{ImporterHoistState, OptionalPeerCandidates, ResolveImporterError},
    resolve_peers::PeerHoistDiscovery,
};
use pnpm_resolving_resolver_base::Resolver;
use std::{sync::Arc, task::Poll};

async fn yield_once() {
    let mut yielded = false;
    std::future::poll_fn(move |cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await;
}

fn check_cancelled(cancel: &ResolutionCancel) -> Result<(), ResolveImporterError> {
    if cancel.is_cancelled() {
        return Err(ResolveImporterError::Resolve(ResolveDependencyTreeError::ResolutionCancelled));
    }
    Ok(())
}

/// One discovery engine serves every hoist round of the workspace: its
/// persistent tree view + walker caches are what keep the barrier
/// linear in workspace size (each importer's pass short-circuits on the
/// subtree verdicts recorded by the passes before it). The engine's
/// tree view is released on return, before the merged-tree snapshot
/// clones the context again, so the two never coexist at peak.
pub(super) async fn run_hoist_rounds<Chain>(
    resolver: &Chain,
    states: &mut [ImporterHoistState],
    workspace: &WorkspaceTreeCtx,
    max_snapshots: Option<usize>,
    cancel: &ResolutionCancel,
) -> Result<(), ResolveImporterError>
where
    Chain: Resolver + ?Sized,
{
    let mut peer_discovery = PeerHoistDiscovery::with_limits(max_snapshots, cancel.clone());
    run_initial_required_rounds(resolver, states, workspace, &mut peer_discovery, cancel).await?;
    run_hoist_barrier(resolver, states, workspace, &mut peer_discovery, cancel).await
}

/// The first required round of every importer, prepared against one quiescent
/// snapshot of the owner-scope maps and then completed. The context is
/// quiescent between the prepare barrier and the completes, so the single
/// snapshot serves every importer.
async fn run_initial_required_rounds<Chain>(
    resolver: &Chain,
    states: &mut [ImporterHoistState],
    workspace: &WorkspaceTreeCtx,
    peer_discovery: &mut PeerHoistDiscovery,
    cancel: &ResolutionCancel,
) -> Result<(), ResolveImporterError>
where
    Chain: Resolver + ?Sized,
{
    let mut rounds = Vec::with_capacity(states.len());
    for (idx, state) in states.iter_mut().enumerate() {
        if idx > 0 && idx % 32 == 0 {
            yield_once().await;
        }
        check_cancelled(cancel)?;
        rounds.push(state.prepare_initial_required_round(peer_discovery));
        peer_discovery.check_snapshot_limit()?;
    }
    let first_importer_by_pkg = workspace.first_importer_by_pkg();
    let first_walk_missing_by_pkg = workspace.children.first_walk_missing_by_pkg();
    for (state, round) in states
        .iter()
        .zip(rounds.iter_mut().flatten())
    {
        state.apply_owner_missing_scope(round, &first_importer_by_pkg, &first_walk_missing_by_pkg);
    }
    let rewrites_before = workspace.tree.children_rewrites();
    for (idx, (state, round)) in states
        .iter_mut()
        .zip(rounds)
        .filter_map(|(state, round)| round.map(|round| (state, round)))
        .enumerate()
    {
        if idx > 0 && idx % 32 == 0 {
            yield_once().await;
        }
        check_cancelled(cancel)?;
        state.complete_initial_required_round(resolver, round, peer_discovery).await?;
        peer_discovery.check_snapshot_limit()?;
    }
    if workspace.tree.children_rewrites() != rewrites_before {
        settle_required_rounds(resolver, states, workspace, peer_discovery, cancel).await?;
    }
    Ok(())
}

/// Repeat optional-peer hoist rounds across every importer until none hoists,
/// re-running the required rounds after each wave. A workspace-wide barrier,
/// so an optional-peer pick sees every importer's resolved versions.
async fn run_hoist_barrier<Chain>(
    resolver: &Chain,
    states: &mut [ImporterHoistState],
    workspace: &WorkspaceTreeCtx,
    peer_discovery: &mut PeerHoistDiscovery,
    cancel: &ResolutionCancel,
) -> Result<(), ResolveImporterError>
where
    Chain: Resolver + ?Sized,
{
    loop {
        check_cancelled(cancel)?;
        if !hoist_optional_wave(resolver, states, OptionalPeerCandidates::Resolved, cancel).await?
            && !hoist_optional_wave(resolver, states, OptionalPeerCandidates::Preferred, cancel)
                .await?
        {
            return Ok(());
        }
        settle_required_rounds(resolver, states, workspace, peer_discovery, cancel).await?;
    }
}

async fn settle_required_rounds<Chain>(
    resolver: &Chain,
    states: &mut [ImporterHoistState],
    workspace: &WorkspaceTreeCtx,
    peer_discovery: &mut PeerHoistDiscovery,
    cancel: &ResolutionCancel,
) -> Result<(), ResolveImporterError>
where
    Chain: Resolver + ?Sized,
{
    loop {
        let rewrites_before = workspace.tree.children_rewrites();
        for (idx, state) in states.iter_mut().enumerate() {
            if idx > 0 && idx % 32 == 0 {
                yield_once().await;
            }
            check_cancelled(cancel)?;
            state.run_required_round(resolver, peer_discovery).await?;
            peer_discovery.check_snapshot_limit()?;
        }
        if workspace.tree.children_rewrites() == rewrites_before {
            return Ok(());
        }
    }
}

async fn hoist_optional_wave<Chain>(
    resolver: &Chain,
    states: &mut [ImporterHoistState],
    candidates: OptionalPeerCandidates,
    cancel: &ResolutionCancel,
) -> Result<bool, ResolveImporterError>
where
    Chain: Resolver + ?Sized,
{
    refresh_root_dep_versions(states);
    let mut any_hoisted = hoist_root_optional(resolver, states, candidates).await?;
    if any_hoisted {
        refresh_root_dep_versions(states);
    }
    for (idx, state) in states.iter_mut().enumerate() {
        if idx > 0 && idx % 32 == 0 {
            yield_once().await;
        }
        check_cancelled(cancel)?;
        if state.importer_id() != pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY {
            any_hoisted |= state.hoist_optional_round(resolver, candidates).await?;
        }
    }
    Ok(any_hoisted)
}

async fn hoist_root_optional<Chain>(
    resolver: &Chain,
    states: &mut [ImporterHoistState],
    candidates: OptionalPeerCandidates,
) -> Result<bool, ResolveImporterError>
where
    Chain: Resolver + ?Sized,
{
    match states
        .iter_mut()
        .find(|state| state.importer_id() == pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY)
    {
        Some(root) => root.hoist_optional_round(resolver, candidates).await,
        None => Ok(false),
    }
}

fn refresh_root_dep_versions(states: &mut [ImporterHoistState]) {
    let root_dep_versions = Arc::new(
        states
            .iter()
            .find(|state| state.importer_id() == pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY)
            .map(ImporterHoistState::direct_dep_versions)
            .unwrap_or_default(),
    );
    for state in states.iter_mut() {
        state.set_workspace_root_dep_versions(Arc::clone(&root_dep_versions));
    }
}
