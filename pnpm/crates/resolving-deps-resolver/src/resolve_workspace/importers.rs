//! Ordering the importers and running their initial waves, up to the
//! point where every importer has a hoist state and shares the root's
//! hoistable dependencies.

use super::{
    broken_optional,
    options::{PassSettings, WorkspaceImporter},
    time_based::TimeBasedCutoff,
};
use crate::{
    resolve_dependency_tree::WorkspaceTreeCtx,
    resolve_importer::{ImporterHoistState, ResolveImporterError, ResolveImporterOptions},
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_resolver_base::Resolver;
use std::{path::PathBuf, sync::Arc};

/// The importers in id order, each with its options.
pub(super) struct SortedImporters<'i, 'a> {
    pub(super) importers: Vec<&'i WorkspaceImporter<'a>>,
    pub(super) opts: Vec<ResolveImporterOptions>,
}

/// Build every importer's options up front so the `time-based` pre-pass
/// and the resolve loop see the same per-importer wiring.
/// `auto_install_peers` and `dedupe_peer_dependents` are workspace-wide
/// (one setting per install), so the workspace-level values override
/// whatever the per-importer callback set — the importer hoist loop and
/// the tree walk's shadow pruning must agree.
///
/// Sorted by importer id: children-owner claims are ranked by importer
/// position and the hoist rounds run sequentially in list order, so a
/// stable order makes ownership, the first-walk missing scope, and every
/// auto-install decision a function of the importer set rather than of
/// the caller's listing order (pnpm/pnpm#13846).
pub(super) fn sorted_importers<'i, 'a, BuildImporterOptions>(
    importers: &'i [WorkspaceImporter<'a>],
    mut per_importer_options: BuildImporterOptions,
    settings: &PassSettings,
) -> SortedImporters<'i, 'a>
where
    BuildImporterOptions: FnMut(&WorkspaceImporter<'a>) -> ResolveImporterOptions,
{
    let mut paired: Vec<(&WorkspaceImporter<'a>, ResolveImporterOptions)> = importers
        .iter()
        .map(|importer| {
            let mut opts = per_importer_options(importer);
            opts.peers.auto_install_peers = settings.peers.auto_install_peers;
            opts.peers.dedupe_peer_dependents = settings.peers.dedupe_peer_dependents;
            (importer, opts)
        })
        .collect();
    paired.sort_by(|(left, _), (right, _)| left.id.cmp(&right.id));
    let (importers, opts) = paired.into_iter().unzip();
    SortedImporters { importers, opts }
}

pub(super) struct InitializedImporters {
    pub(super) importer_ids: Vec<String>,
    pub(super) states: Vec<ImporterHoistState>,
    /// Each importer's project and modules dir, for its peer input.
    pub(super) input_dirs: Vec<(PathBuf, Option<PathBuf>)>,
    pub(super) optional_direct: Vec<broken_optional::OptionalDirect>,
}

/// Phase 1: every importer's initial wave resolves before any peer
/// hoist runs, then hoist rounds repeat across all importers until
/// none hoists — a workspace-wide barrier, so an optional-peer pick
/// sees every importer's resolved versions.
///
/// The initial waves run concurrently, like the TypeScript resolver's
/// importer fan-out: the shared context's children-owner claims are
/// rank-ordered (not arrival-ordered) and the peer-hoist pickers'
/// preferred-version candidates are derived from the settled
/// reachable tree (see `WorkspaceTreeCtx::run_preferred_versions`),
/// so the resolved graph is the same regardless of interleaving, and
/// a large workspace's walks overlap their resolver and hook waits
/// instead of paying them importer by importer.
pub(super) async fn init_importers<Chain>(
    resolver: &Chain,
    sorted: SortedImporters<'_, '_>,
    dependency_groups: &[DependencyGroup],
    cutoff: &TimeBasedCutoff,
    settings: &PassSettings,
    workspace: &Arc<WorkspaceTreeCtx>,
) -> Result<InitializedImporters, ResolveImporterError>
where
    Chain: Resolver + ?Sized,
{
    let mut input_dirs = Vec::with_capacity(sorted.importers.len());
    let optional_direct = sorted.importers
        .iter()
        .map(|importer| {
            importer.manifest
                .dependencies([DependencyGroup::Optional])
                .map(|(alias, specifier)| (alias.to_string(), specifier.to_string()))
                .collect()
        })
        .collect();
    let mut states = Vec::with_capacity(sorted.importers.len());
    for (importer_order, (importer, mut importer_opts)) in sorted.importers
        .iter()
        .zip(sorted.opts)
        .enumerate()
    {
        importer_opts.resolution.pick_lowest_direct = settings.version.pick_lowest_direct;
        importer_opts.resolution.subdep_published_by = cutoff.published_by;
        input_dirs.push((
            importer_opts.base_opts.project.project_dir.clone(),
            importer_opts.links.modules_dir.clone(),
        ));
        // Boxed to keep the enclosing install future small: inlining a
        // wave's frame into it trips the workspace's large-future lint.
        states.push(
            Box::pin(ImporterHoistState::init(
                resolver,
                &importer.id,
                importer_order,
                importer.manifest,
                dependency_groups.iter().copied(),
                importer_opts,
                Arc::clone(workspace),
            ))
            .await?,
        );
    }
    let importer_ids = sorted.importers
        .into_iter()
        .map(|importer| importer.id.clone())
        .collect();
    Ok(InitializedImporters { importer_ids, states, input_dirs, optional_direct })
}

/// Computed after the init barrier and shared unchanged: recomputing it
/// per round would let the root's own hoisted peers become candidates
/// for the importers hoisted after it.
pub(super) fn share_root_deps(
    states: &mut [ImporterHoistState],
) -> Result<(), ResolveImporterError> {
    let root_state = states
        .iter()
        .find(|state| state.importer_id() == pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY);
    let root_deps = Arc::new(
        root_state
            .map(ImporterHoistState::hoistable_root_deps)
            .transpose()?
            .unwrap_or_default(),
    );
    let root_dep_versions =
        Arc::new(root_state.map(ImporterHoistState::direct_dep_versions).unwrap_or_default());
    for state in states.iter_mut() {
        state.set_workspace_root_deps(Arc::clone(&root_deps), Arc::clone(&root_dep_versions));
    }
    Ok(())
}
