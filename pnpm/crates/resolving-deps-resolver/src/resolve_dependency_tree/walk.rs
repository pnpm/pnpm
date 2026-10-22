//! The fresh-resolve walk: seeding one edge's package
//! ([`fn@resolve_node_seed`]), seeding a settled occurrence's children
//! ([`fn@seed_node_children`]), and the per-wanted resolve cache both
//! go through. Seeding and settling are separate phases so a whole
//! depth level resolves before any of it is settled — which is what
//! makes children ownership, and with it the lockfile, independent of
//! the order concurrent subtrees finish in ([`fn@walk_from_seeds`]).

pub(super) use warm_children::warm_children_resolutions;

pub(super) use crate::resolve_dependency_tree::workspace_ctx::{
    cached_workspace_final, completed_resolved_wanted, wanted_key_admission,
};

pub(crate) use child_seeds::parent_ids_contain_sequence;

pub(super) use level_walk::{level_aliases, level_versions};

pub(super) use locked_versions::node_alias;

pub(super) use edge_resolution::{
    closes_cycle, node_id_for, resolve_node_seed, try_resolve_edge_cached,
};

mod warm_children;

mod workspace_resolution;
use workspace_resolution::resolve_wanted_cached;

mod child_seeds;
use child_seeds::{
    ChildSeedScope, catalogs_for_children, child_wanted, landed_on_prior_entry,
    overlay_lookup_names, resolves_children_through_catalogs, seed_node_children, walk_child_specs,
};

mod level_walk;
use level_walk::{assign_level_owners, pkgs_info_from_ids, seeded_dep, settle_level, settle_seeds};

mod locked_versions;
use locked_versions::{
    ensure_same_registry_revision, keeps_locked_version, overlay_version_view, pin_locked_version,
    pin_patched_revision,
};

mod edge_resolution;

mod failed_edge;

mod package_root_link;
use package_root_link::{
    link_file_deps_inside_package, package_root_link_result, wanted_package_root_link,
};

use async_recursion::async_recursion;
use futures_util::future;
use pipe_trait::Pipe;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::{LockfileResolution, PkgNameVerPeer, SnapshotEntry, TarballRevision};
use pnpm_resolving_npm_resolver::PickPackageError;
use pnpm_resolving_resolver_base::{
    CurrentPkg, GitResolveError, NoMatchingVersionError, PreferredVersionsOverlay,
    RegistryResponseError, ResolveError, ResolveOptions, Resolver, UnsupportedProtocolError,
    UpdateBehavior, WantedDependency,
};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use serde_json::Value;
use std::{borrow::Cow, collections::BTreeMap, path::Path, sync::Arc};

use crate::{
    lockfile_reuse::{current_pkg_from_lockfile, prior_child_key},
    node_id::NodeId,
    parent_pkg_aliases::ParentPkgAliases,
    peer_provider::{dependencies_named_as_peers, peer_shadowed_dependencies},
    resolved_tree::{
        AncestorPkgIds, DirectDep, ResolvedPackage, ResolvedPackageInput, TreeChildren,
    },
};

use super::{
    DependencySpec, ResolveDependencyTreeError, SkippedOptionalDependency,
    SkippedOptionalDependencyParent,
    catalogs::catalog_anchor,
    lock_recoverable,
    manifest::{
        build_pkg_id_with_patch_hash, emit_deprecation_if_needed, extract_children,
        extract_peer_dependencies, is_exotic_resolved_via, pkg_is_leaf,
    },
    read_recoverable,
    reuse::{
        PendingReusedNode, ReuseSource, finish_reused_node, higher_direct_dep_version,
        node_depends_on_changed_direct_dep, real_package_name_of, reuse_matches_peer_scope,
        seed_reused_node, try_reuse_node, wanted_lockfile_contains_satisfying_entry,
    },
    tree_ctx::{
        TreeCtx, declaring_manifest_dir, opts_relative_to_declaring_manifest,
        project_relative_cache_scope,
    },
    update_scope::{is_update_target, update_unpins_edge},
    workspace_ctx::{
        ChildrenOwnerClaim, ChildrenRecording, PackageRegistration, RecordedChildrenContext,
        SharedWorkspaceWantedKey, WantedKey, WorkspaceFinalWantedKey, children_warmup_claimed,
        claim_children_owner, claim_children_warmup, claim_recorded_subtree_owners,
        insert_tree_node, is_current_children_owner, make_non_owner_nodes_lazy, record_children,
        recorded_children_match, recorded_prior_key, register_peer_dep_names,
        register_shadowable_dep_names,
    },
    write_recoverable,
};

/// Walk the snapshot children of any reused seeds in `seeds` once the
/// level's `children_pkg_aliases` scope has been settled.
pub(super) async fn finish_reused_seeds<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    seeds: &mut [NodeSeed],
    children_pkg_aliases: &Arc<ParentPkgAliases>,
) -> Result<(), ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let reused: Vec<PendingReusedNode> = seeds
        .iter_mut()
        .filter_map(|seed| match seed {
            NodeSeed::Reused(_) => {
                let NodeSeed::Reused(reused) = std::mem::replace(seed, NodeSeed::Done(None)) else {
                    unreachable!()
                };
                *seed = NodeSeed::Done(Some(reused.dep.clone()));
                Some(*reused)
            }
            _ => None,
        })
        .collect();
    if reused.is_empty() {
        return Ok(());
    }
    reused
        .into_iter()
        .map(|node| finish_reused_node(ctx, resolver, node, children_pkg_aliases))
        .pipe(future::try_join_all)
        .await?;
    Ok(())
}

/// Outcome of [`fn@resolve_node_seed`]: either the edge completed
/// without a children walk (cycle break, skipped optional, or finished
/// lockfile reuse), the package was reused from the lockfile and its
/// snapshot children are waiting on the level's settled alias scope,
/// or the package resolved fresh and its children are still pending —
/// the level settles which occurrence of it walks them, and only then
/// does that one seed its children, so their resolution sees the whole
/// level's versions in its preferred-versions overlay.
pub(super) enum NodeSeed {
    Done(Option<DirectDep>),
    Reused(Box<PendingReusedNode>),
    Pending(Box<PendingNode>),
}

/// How the walk treats one package occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NodeKind {
    /// A workspace link. The linked project resolves its own
    /// dependencies as a separate importer, so this walk does not
    /// descend into it.
    Link,
    /// A package with no dependencies, optional dependencies, peers, or
    /// `peerDependenciesMeta`.
    Leaf,
    /// A package whose children are walked.
    Branch,
}

impl NodeKind {
    pub(super) fn of(result: &pnpm_resolving_resolver_base::ResolveResult, id: &str) -> Self {
        if id.starts_with("link:") {
            NodeKind::Link
        } else if pkg_is_leaf(result) {
            NodeKind::Leaf
        } else {
            NodeKind::Branch
        }
    }

    /// Links and leaves share one tree node per package: see [`fn@node_id_for`].
    pub(super) fn is_leaf(self) -> bool {
        match self {
            NodeKind::Link | NodeKind::Leaf => true,
            NodeKind::Branch => false,
        }
    }
}

/// A resolved-but-not-settled node: everything the level settlement
/// needs to decide whether this occurrence walks the package's
/// children, and [`fn@seed_node_children`] needs to seed them.
pub(super) struct PendingNode {
    result: Arc<pnpm_resolving_resolver_base::ResolveResult>,
    kind: NodeKind,
    resolves_children_through_catalogs: bool,
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
    /// The edge's recorded snapshot key in the prior lockfile, if
    /// any — threads each child's prior ref through the walk phase
    /// via `ReuseSource::Transitive`.
    pub(super) prior_key: Option<PkgNameVerPeer>,
    identity: PendingNodeIdentity,
    ancestry: PendingNodeAncestry,
}

pub(super) struct PendingNodeIdentity {
    id: Arc<str>,
    alias: String,
    node_id: NodeId,
}

pub(super) struct PendingNodeAncestry {
    parent_ancestors: AncestorPkgIds,
    next_ancestors: AncestorPkgIds,
    depth: i32,
    current_is_optional: bool,
}

/// What a fresh resolve knows about a package the first time it reaches it.
#[derive(Clone, Copy)]
struct SeededPackage<'a> {
    id: &'a str,
    result: &'a Arc<pnpm_resolving_resolver_base::ResolveResult>,
    peer_shadowed: &'a HashSet<String>,
    resolves_children_through_catalogs: bool,
    current_is_optional: bool,
    kind: NodeKind,
}

/// The parent-side context one child edge resolves in.
pub(super) struct ChildEdge<'e> {
    pub(super) ancestor_ids: &'e AncestorPkgIds,
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
    child_specs: Arc<Vec<DependencySpec>>,
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
    finish_reused_seeds(ctx, resolver, &mut seeds, &children_pkg_aliases).await?;
    assign_level_owners(ctx, seeds.iter_mut())?;
    let direct: Vec<DirectDep> = seeds
        .iter()
        .filter_map(seeded_dep)
        .collect();
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
