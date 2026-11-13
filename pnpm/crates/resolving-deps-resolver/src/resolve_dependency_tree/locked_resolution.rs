use super::{TreeCtx, lock_recoverable};
use crate::{NodeId, resolved_tree::LockedResolution};
use pnpm_deps_path::DepPath;
use pnpm_lockfile::{BoundProviders, PkgNameVerPeer};
use std::collections::BTreeMap;

pub(super) fn from_prior(ctx: &TreeCtx, key: &PkgNameVerPeer) -> Option<Box<LockedResolution>> {
    let lockfile = ctx.workspace.reuse.lockfile.as_ref()?;
    let snapshot = lockfile.snapshots.as_ref()?.get(key)?;
    let metadata = lockfile.packages
        .as_ref()?
        .get(&key.without_peer())?;
    let bound = metadata.peer_dependencies
        .as_ref()
        .filter(|peers| !peers.is_empty())
        .map(|_| bound_providers(ctx, lockfile, key));
    let peers: BTreeMap<_, _> = metadata.peer_dependencies
        .iter()
        .flatten()
        .filter_map(|(name, _)| {
            let alias = name.parse().ok()?;
            let reference = snapshot.dependencies
                .as_ref()
                .and_then(|deps| deps.get(&alias))
                .or_else(|| snapshot.optional_dependencies.as_ref()?.get(&alias))?;
            let provider = reference.resolve(&alias)?;
            bound
                .as_ref()?
                .binds(&provider)
                .then(|| (name.clone(), DepPath::from(provider.to_string())))
        })
        .collect();
    Some(Box::new(LockedResolution {
        previous_dep_path: Some(DepPath::from(key.to_string())),
        locked_peer_context: (!peers.is_empty()).then_some(peers),
        dependency_names_whose_current_provider_must_win: None,
    }))
}

pub(super) fn record(ctx: &TreeCtx, node_id: &NodeId, locked: Option<Box<LockedResolution>>) {
    ctx.workspace.record_locked_resolution(node_id, locked);
}

fn bound_providers(
    ctx: &TreeCtx,
    lockfile: &pnpm_lockfile::Lockfile,
    key: &PkgNameVerPeer,
) -> BoundProviders {
    let bound = BoundProviders::from_key(key);
    if !bound.is_hashed() {
        return bound;
    }
    crate::lockfile_reuse::cached_bound_providers(
        lockfile,
        key,
        &mut lock_recoverable(&ctx.workspace.cache.lockfile_reuse.bound_providers),
    )
}
