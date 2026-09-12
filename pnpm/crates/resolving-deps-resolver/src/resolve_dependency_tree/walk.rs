//! The fresh-resolve walk: seeding one edge's package
//! ([`fn@resolve_node_seed`]), seeding a settled occurrence's children
//! ([`fn@seed_node_children`]), and the per-wanted resolve cache both
//! go through. Seeding and settling are separate phases so a whole
//! depth level resolves before any of it is settled — which is what
//! makes children ownership, and with it the lockfile, independent of
//! the order concurrent subtrees finish in ([`fn@walk_from_seeds`]).

pub(super) use warm_children::warm_children_resolutions;

pub(crate) use child_seeds::parent_ids_contain_sequence;

pub(super) use level_walk::{level_aliases, level_versions};

pub(super) use locked_versions::node_alias;

pub(super) use edge_resolution::{closes_cycle, node_id_for, resolve_node_seed};

mod warm_children;

mod workspace_resolution;
use workspace_resolution::resolve_wanted_cached;

mod child_seeds;
use child_seeds::{
    catalogs_for_children, overlay_lookup_names, resolve_catalog_child_specs,
    resolves_children_through_catalogs, seed_node_children,
};

mod level_walk;
use level_walk::{assign_level_owners, pkgs_info_from_ids, seeded_dep, settle_level, settle_seeds};

mod locked_versions;
use locked_versions::{
    ensure_same_registry_revision, overlay_version_view, pin_locked_version, pin_patched_revision,
};

mod edge_resolution;

use async_recursion::async_recursion;
use futures_util::future;
use pipe_trait::Pipe;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::{LockfileResolution, PkgNameVerPeer, SnapshotEntry, TarballRevision};
use pnpm_resolving_resolver_base::{
    CurrentPkg, GitResolveError, NoMatchingVersionError, PreferredVersionsOverlay,
    RegistryResponseError, ResolveError, ResolveOptions, Resolver, UpdateBehavior,
    WantedDependency,
};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use serde_json::Value;
use std::{borrow::Cow, collections::BTreeMap, path::Path, sync::Arc};

use crate::{
    lockfile_reuse::{current_pkg_from_lockfile, prior_child_key},
    node_id::NodeId,
    parent_pkg_aliases::{ParentPkgAliases, peer_shadowed_dependencies},
    resolved_tree::{DirectDep, ResolvedPackage},
};

use super::{
    ResolveDependencyTreeError, SkippedOptionalDependency, SkippedOptionalDependencyParent,
    catalogs::resolve_catalog_specifier,
    lock_recoverable,
    manifest::{
        build_pkg_id_with_patch_hash, emit_deprecation_if_needed, extract_children,
        extract_peer_dependencies, is_exotic_resolved_via, pkg_is_leaf,
    },
    reuse::{
        ReuseSource, higher_direct_dep_version, is_update_target,
        node_depends_on_changed_direct_dep, real_package_name_of, resolve_reused_node,
        try_reuse_node, update_unpins_edge, wanted_lockfile_contains_satisfying_entry,
    },
    tree_ctx::{
        TreeCtx, declaring_manifest_dir, opts_relative_to_declaring_manifest,
        project_relative_cache_scope,
    },
    workspace_ctx::{
        ChildSpec, ChildrenOwnerClaim, RecordedChildrenContext, SharedWorkspaceWantedKey,
        WantedKey, WorkspaceFinalWantedKey, claim_children_owner, claim_children_warmup,
        insert_tree_node, is_current_children_owner, lazy_children, make_non_owner_nodes_lazy,
        record_children, recorded_children_match, register_peer_dep_names,
        remember_node_parent_ids,
    },
};

/// Resolve one `(alias, range)` edge end-to-end with no
/// preferred-versions overlay: [`fn@resolve_node_seed`] then
/// [`fn@walk_from_seeds`]. Used where per-level preference folding
/// does not apply — the lockfile-reuse subtree walk, whose versions
/// are exact pins.
///
/// The node's children resolve in the same `parent_pkg_aliases` scope
/// as the node itself, without this level's own aliases folded in: a
/// reused subtree takes its children from the lockfile snapshot rather
/// than from a manifest, so no shadowed dependency can be dropped
/// inside it, and the narrower scope only ever means one omission
/// fewer where an edge falls back to a fresh resolve.
#[expect(
    clippy::too_many_arguments,
    reason = "internal walker helper threading per-node context through the recursion"
)]
#[async_recursion]
pub(super) async fn resolve_node<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: WantedDependency,
    ancestor_ids: &Arc<Vec<String>>,
    depth: i32,
    parent_optional: bool,
    reuse: ReuseSource,
    parent_pkg_aliases: &Arc<ParentPkgAliases>,
) -> Result<Option<DirectDep>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let base_overlay = ctx.base_opts.preferred_versions_overlay.clone();
    let seed = resolve_node_seed(
        ctx,
        resolver,
        wanted,
        ChildEdge {
            ancestor_ids,
            depth,
            parent_optional,
            reuse,
            pick_overlay: base_overlay.clone(),
            parent_dir: None,
            parent_pkg_aliases,
            parent_is_workspace: false,
        },
    )
    .await?;
    let direct =
        walk_from_seeds(ctx, resolver, vec![seed], base_overlay, Arc::clone(parent_pkg_aliases))
            .await?;
    Ok(direct.into_iter().next())
}

/// Outcome of [`fn@resolve_node_seed`]: either the edge completed
/// without a children walk (lockfile reuse, cycle break), or the
/// package resolved and its children are still pending — the level
/// settles which occurrence of it walks them, and only then does that
/// one seed its children, so their resolution sees the whole level's
/// versions in its preferred-versions overlay.
pub(super) enum NodeSeed {
    Done(Option<DirectDep>),
    Pending(Box<PendingNode>),
}

/// A resolved-but-not-settled node: everything the level settlement
/// needs to decide whether this occurrence walks the package's
/// children, and [`fn@seed_node_children`] needs to seed them.
pub(super) struct PendingNode {
    result: Arc<pnpm_resolving_resolver_base::ResolveResult>,
    id: String,
    alias: String,
    node_id: NodeId,
    is_link: bool,
    resolves_children_through_catalogs: bool,
    parent_ancestors: Arc<Vec<String>>,
    next_ancestors: Arc<Vec<String>>,
    /// The dependency names this occurrence's own `peerDependencies`
    /// shadow. Ownership of the package's children is settled across
    /// the whole level once it has seeded (see
    /// [`fn@assign_level_owners`]), and the winner's set is the one
    /// that filters the children and splits the envelope's peers.
    ///
    /// Readable up to that settlement only: settling moves the winner's
    /// set onto its claim and leaves this one empty. The speculative
    /// child prewarm reads it while the level is still seeding.
    peer_shadowed: HashSet<String>,
    /// The claim [`fn@assign_level_owners`] settled for this
    /// occurrence, once its level has seeded in full.
    claim: Option<ChildrenOwnerClaim>,
    depth: i32,
    current_is_optional: bool,
    /// The edge's recorded snapshot key in the prior lockfile, if
    /// any — threads each child's prior ref through the walk phase
    /// via `ReuseSource::Transitive`.
    pub(super) prior_key: Option<PkgNameVerPeer>,
}

/// What a fresh resolve knows about a package the first time it reaches it.
#[derive(Clone, Copy)]
struct SeededPackage<'a> {
    id: &'a str,
    result: &'a Arc<pnpm_resolving_resolver_base::ResolveResult>,
    peer_shadowed: &'a HashSet<String>,
    resolves_children_through_catalogs: bool,
    current_is_optional: bool,
    is_link: bool,
    is_leaf: bool,
}

/// The parent-side context one child edge resolves in.
pub(super) struct ChildEdge<'e> {
    pub(super) ancestor_ids: &'e Arc<Vec<String>>,
    pub(super) depth: i32,
    pub(super) parent_optional: bool,
    pub(super) reuse: ReuseSource,
    pub(super) pick_overlay: Option<Arc<PreferredVersionsOverlay>>,
    pub(super) parent_dir: Option<&'e Path>,
    pub(super) parent_pkg_aliases: &'e Arc<ParentPkgAliases>,
    pub(super) parent_is_workspace: bool,
}

/// One occurrence whose children the walk still has to seed, with the
/// scope they resolve in: `children_overlay` is the preferred-versions
/// overlay covering the occurrence's own level (the caller folds every
/// sibling of that level into it), and `children_pkg_aliases` the
/// alias scope that level installs.
struct FrontierNode {
    pending: Box<PendingNode>,
    claim: ChildrenOwnerClaim,
    children_overlay: Option<Arc<PreferredVersionsOverlay>>,
    children_pkg_aliases: Arc<ParentPkgAliases>,
}

/// A frontier node that has seeded its children — what settling the
/// level needs to record its edges and to hand the children that own
/// their own packages on to the next level.
struct SeededNode {
    node: FrontierNode,
    child_specs: Arc<Vec<ChildSpec>>,
    seeds: Vec<NodeSeed>,
    grandchild_overlay: Option<Arc<PreferredVersionsOverlay>>,
    grandchild_pkg_aliases: Arc<ParentPkgAliases>,
}

/// Walk `seeds` and everything below them, one depth level at a time,
/// and report the seeds' own edges back to the caller.
///
/// A level is seeded in full before any of it is settled, so every
/// occurrence of a package at that depth is in hand when the package's
/// children ownership is decided ([`fn@assign_level_owners`]).
/// Ownership then never passes to an occurrence seeded later, and the
/// children each package records stop depending on which subtree
/// happened to reach it first — the arrival-order dependence behind
/// <https://github.com/pnpm/pnpm/issues/13685>. It is also why no
/// occurrence ever re-walks a package another one recorded, which the
/// exponential blowup of <https://github.com/pnpm/pnpm/issues/13574>
/// made the alternative to.
#[async_recursion]
pub(super) async fn walk_from_seeds<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    mut seeds: Vec<NodeSeed>,
    children_overlay: Option<Arc<PreferredVersionsOverlay>>,
    children_pkg_aliases: Arc<ParentPkgAliases>,
) -> Result<Vec<DirectDep>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    assign_level_owners(ctx, seeds.iter_mut())?;
    let direct: Vec<DirectDep> = seeds.iter().filter_map(seeded_dep).collect();
    let mut frontier = settle_seeds(ctx, seeds, children_overlay.as_ref(), &children_pkg_aliases);
    let mut level = 0usize;
    while !frontier.is_empty() {
        let level_started = std::time::Instant::now();
        let frontier_len = frontier.len();
        let seeded = frontier
            .into_iter()
            .map(|node| seed_node_children(ctx, resolver, node))
            .pipe(future::try_join_all)
            .await?;
        frontier = settle_level(ctx, seeded)?;
        level += 1;
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "resolve_level",
            level,
            parents = frontier_len,
            next = frontier.len(),
            elapsed_ms = level_started.elapsed().as_millis() as u64,
            "phase complete",
        );
    }
    Ok(direct)
}

/// Render `{alias}@{bare}` (either half dropped when absent) for the
/// no-resolver error message.
fn render_specifier(wanted: &WantedDependency) -> String {
    let alias = wanted.alias.as_deref().unwrap_or("");
    let bare = wanted.bare_specifier.as_deref().unwrap_or("");
    match (alias.is_empty(), bare.is_empty()) {
        (true, true) => String::new(),
        (true, false) => bare.to_string(),
        (false, true) => alias.to_string(),
        (false, false) => format!("{alias}@{bare}"),
    }
}

#[cfg(test)]
mod tests;
