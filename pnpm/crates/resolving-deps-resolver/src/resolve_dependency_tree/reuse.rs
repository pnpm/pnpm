//! Reuse of the prior lockfile's resolutions: which edges may reuse at
//! all ([`ReuseSource`]), whether a whole subtree is reproducible from
//! the snapshot graph, and the snapshot-driven walk a reused node's
//! children take ([`fn@seed_reused_node`], [`fn@finish_reused_node`]).
//!
//! What a `pacquet update` reaches, and so excludes from reuse, is
//! [`super::update_scope`].

pub(crate) use direct_versions::{preseed_changed_direct_versions, record_changed_direct_deps};

pub(super) use direct_versions::{
    higher_direct_dep_version, node_depends_on_changed_direct_dep, record_direct_dep_versions,
};

pub(super) use snapshot_children::{
    PendingReusedNode, reuse_key_matches_peer_scope, reuse_matches_peer_scope,
};

mod direct_versions;
mod snapshot_children;
mod subtree;
use snapshot_children::{reused_children, reused_identity};
use subtree::subtree_fully_reusable;

use async_recursion::async_recursion;
use pnpm_lockfile::{
    BoundProviders, PkgName, PkgNameVerPeer, ProjectSnapshot, ResolvedDependencyMap,
    SnapshotDepRef, SnapshotEntry,
};
use pnpm_resolving_resolver_base::{Resolver, WantedDependency};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

use crate::{
    lockfile_reuse::{reusable_importer_dep, synthesize_reused_result},
    node_id::NodeId,
    parent_pkg_aliases::{ParentPkgAliases, declared_peer_dependencies},
    resolved_tree::{
        AncestorPkgIds, DirectDep, PeerDep, PeerSet, ResolvedPackage, ResolvedPackageInput,
        TreeChildren,
    },
};

use super::{
    DependencySpec, ResolveDependencyTreeError, UpdateReuseScope, lock_recoverable,
    manifest::{build_pkg_id_with_patch_hash, emit_deprecation_if_needed},
    tree_ctx::TreeCtx,
    walk::{
        ChildEdge, NodeSeed, closes_cycle, level_aliases, node_alias, node_id_for,
        resolve_node_seed, walk_from_seeds,
    },
    workspace_ctx::{
        ChildrenOwnerClaim, ChildrenRecording, DirectDepVersions, PackageRegistration,
        RecordedChildrenContext, claim_children_owner, insert_tree_node, is_current_children_owner,
        make_non_owner_nodes_lazy, record_children, register_peer_dep_names,
        register_shadowable_dep_names,
    },
};

/// How the current [`fn@resolve_node_seed`] call may reuse the prior
/// lockfile's resolution instead of re-resolving from the registry.
///
/// Threaded down the recursion to drive the `resolvedDependencies` /
/// `parentPkg.updated` reuse mechanism: an updated parent discards its
/// child-refs so the subtree re-resolves, while a non-updated parent
/// keeps them alive.
#[derive(Clone)]
pub(super) enum ReuseSource {
    /// A direct dependency of importer `importer_id`. Reuse matches the
    /// manifest specifier against the importer's recorded resolution via
    /// semver-satisfies ([`reusable_importer_dep`]).
    Importer { importer_id: String },
    /// A transitive dependency whose resolved snapshot key the parent's
    /// snapshot already pins. `Some` reuses that key directly (no semver
    /// check — the parent version pins it); `None` means an updated
    /// ancestor discarded its child-refs, forcing this subtree to
    /// re-resolve. The parent may itself be reused (snapshot walk) or
    /// freshly resolved onto its previously recorded version — both
    /// keep the prior child refs alive, mirroring pnpm's
    /// `parentPkg.updated ? undefined : resolvedDependencies`.
    Transitive { key: Option<PkgNameVerPeer> },
    /// Reuse disabled for this node (no prior lockfile).
    Off,
}

impl ReuseSource {
    /// The prior lockfile snapshot key recorded for this edge, if any —
    /// the basis of both subtree reuse and the `currentPkg` payload.
    /// The `Importer` arm applies the semver-satisfies gate
    /// ([`reusable_importer_dep`]): a lockfile reference is only reused
    /// when the recorded version still satisfies the wanted spec.
    pub(super) fn prior_key(
        &self,
        ctx: &TreeCtx,
        wanted: &WantedDependency,
    ) -> Option<PkgNameVerPeer> {
        let lockfile = ctx.workspace.reuse.lockfile.as_ref()?;
        match self {
            ReuseSource::Importer { importer_id } => {
                let alias = wanted.alias.as_deref()?;
                if ctx.workspace.is_stale_peer_pin(importer_id, alias) {
                    return None;
                }
                reusable_importer_dep(
                    lockfile,
                    importer_id,
                    alias,
                    wanted.bare_specifier.as_deref()?,
                )
            }
            ReuseSource::Transitive { key } => key.clone(),
            ReuseSource::Off => None,
        }
    }

    /// Whether this edge may reuse the prior lockfile's subtree.
    pub(super) fn allows_reuse(&self) -> bool {
        matches!(self, ReuseSource::Importer { .. } | ReuseSource::Transitive { .. })
    }
}

/// One reusable node: its prior-lockfile snapshot key plus the
/// `ResolveResult` synthesized from the lockfile metadata.
pub(super) struct ReusedNode {
    key: PkgNameVerPeer,
    result: pnpm_resolving_resolver_base::ResolveResult,
}

/// Decide whether the current edge can reuse the prior lockfile's
/// resolution. `prior_key` is the edge's recorded snapshot key (see
/// [`ReuseSource::prior_key`]). Returns the synthesized node when the
/// edge's whole transitive subtree is reusable; `None` (fresh resolve)
/// otherwise.
///
/// Conservative on every axis: no prior lockfile, no recorded key, a
/// `link:` / non-registry shape anywhere in the subtree, or a missing
/// snapshot entry all yield `None`. See [`fn@subtree_fully_reusable`]
/// for the recursive subtree check.
pub(super) fn try_reuse_node(
    ctx: &TreeCtx,
    wanted: &WantedDependency,
    prior_key: Option<&PkgNameVerPeer>,
    depth: i32,
) -> Option<ReusedNode> {
    let lockfile = ctx.workspace.reuse.lockfile.as_ref()?;
    let scope = ctx.update_scope();
    if matches!(scope.reuse, UpdateReuseScope::None) && scope.max_depth.reaches(depth) {
        return None;
    }
    let alias = wanted.alias.as_deref()?;
    let key = prior_key?;
    if !subtree_fully_reusable(ctx, lockfile, key, depth) {
        return None;
    }
    let result = synthesize_reused_result(lockfile, key, alias)?;
    Some(ReusedNode { key: key.clone(), result })
}

/// Whether the wanted lockfile already holds a package entry that
/// satisfies the wanted dependency. An optional dependency that fails
/// to resolve is normally skipped, but when a locked resolution exists
/// the failure is environmental (e.g. a registry mirror that hasn't
/// synced the release yet) rather than a genuinely uninstallable
/// package. Silently skipping in that case would erase the locked
/// entries, making the lockfile differ across machines from identical
/// inputs and leaving frozen installs on other hosts with nothing to
/// link (<https://github.com/pnpm/pnpm/issues/12853>).
///
/// Only plain semver specifiers are checked; exotic specifiers (git,
/// catalogs, tags, URLs) keep the skip-on-failure behavior.
///
/// The check is deliberately by package name and range rather than by
/// the current edge's locked snapshot ref: a satisfying entry locked
/// via any edge means the registry served this package in-range
/// before, so failing loudly instead of skipping is the right outcome
/// even when the failing edge itself was never locked. This matches
/// the TypeScript resolver's `wantedLockfileContainsSatisfyingEntry`.
pub(super) fn wanted_lockfile_contains_satisfying_entry(
    lockfile: Option<&pnpm_lockfile::Lockfile>,
    wanted: &WantedDependency,
) -> bool {
    let Some(packages) = lockfile.and_then(|lockfile| lockfile.packages.as_ref()) else {
        return false;
    };
    let Some(alias) = wanted.alias
        .as_deref()
        .filter(|alias| !alias.is_empty())
    else {
        return false;
    };
    let (pkg_name, range) =
        unwrap_package_name(alias, wanted.bare_specifier.as_deref().unwrap_or_default());
    let Ok(range) = range.parse::<node_semver::Range>() else {
        return false;
    };
    let Ok(pkg_name) = PkgName::parse(pkg_name) else {
        return false;
    };
    packages
        .keys()
        .any(|key| {
            key.name == pkg_name
                && key.suffix
                    .version_semver()
                    .is_some_and(|version| range.satisfies(version))
        })
}

/// Normalize an `npm:` alias specifier into the real package name and
/// the wanted range (`("is-positive", "^1.0.0")` for the edge
/// `my-alias@npm:is-positive@^1.0.0`; the range is `"*"` for the
/// spec-less `npm:is-positive` form). A specifier without the `npm:`
/// prefix is returned as-is: the alias is the real package name.
///
/// Mirrors the TypeScript resolver's `unwrapPackageName`; unlike
/// [`fn@real_package_name_of`] it also yields the range and does not
/// special-case the `npm:<range>` form, so the locked-entry check
/// matches its TypeScript counterpart byte for byte.
pub(crate) fn unwrap_package_name<'a>(
    alias: &'a str,
    bare_specifier: &'a str,
) -> (&'a str, &'a str) {
    let Some(rest) = bare_specifier.strip_prefix("npm:") else {
        return (alias, bare_specifier);
    };
    match rest.rfind('@') {
        None | Some(0) => (rest, "*"),
        Some(index) => (&rest[..index], &rest[index + 1..]),
    }
}

/// Resolve the *real* package name an `(alias, bare_specifier)` edge
/// targets — the name update targeting matches against, not the local
/// install alias, which an `npm:` alias or a `jsr:` specifier can
/// differ from. The picker and the lockfile snapshots key on this name.
/// `walk::overlay_lookup_names` builds its candidate set from it.
///
/// `None` when no name can be recovered; the caller reads that as "not
/// a targeted update", since update targets are keyed by package name.
pub fn real_package_name_of<'edge>(
    alias: Option<&'edge str>,
    bare_specifier: Option<&'edge str>,
) -> Option<Cow<'edge, str>> {
    let bare = bare_specifier?;
    if let Some(rest) = bare.strip_prefix("npm:") {
        let alias_keeps_name = alias.is_some_and(|alias| {
            !alias.is_empty() && rest.parse::<node_semver::Range>().is_ok()
        });
        if !alias_keeps_name {
            let last_at = rest
                .bytes()
                .enumerate()
                .rev()
                .find_map(|(i, b)| (b == b'@').then_some(i));
            let name = match last_at {
                Some(idx) if idx >= 1 => &rest[..idx],
                _ => rest,
            };
            return (!name.is_empty()).then_some(Cow::Borrowed(name));
        }
    }
    if bare.starts_with("jsr:") {
        let spec =
            pnpm_resolving_jsr_specifier_parser::parse_jsr_specifier(bare, alias).ok().flatten()?;
        return Some(Cow::Owned(spec.npm_pkg_name));
    }
    alias.map(Cow::Borrowed)
}

/// Register a node whose resolution was reused from the prior lockfile,
/// leaving its snapshot children to [`fn@finish_reused_node`] once the
/// current level's [`ParentPkgAliases`] scope has been settled.
pub(super) async fn seed_reused_node(
    ctx: &TreeCtx,
    wanted: WantedDependency,
    edge: &ChildEdge<'_>,
    current_is_optional: bool,
    reused: ReusedNode,
) -> Result<NodeSeed, ResolveDependencyTreeError> {
    // The synthesized result must stay out of `resolved_by_wanted`: its
    // manifest deliberately omits `dependencies` (a reused node's
    // children come from the snapshot graph), so if the fresh-resolve
    // path ever read it back — e.g. an identical edge denied reuse by
    // the changed-direct-dep gate or by `subtree_fully_reusable`'s
    // provisional-`false` cycle guard — `extract_children` /
    // `pkg_is_leaf` would misread the package as dependency-less and
    // record it as a leaf, emptying its lockfile snapshot.
    let result = Arc::new(reused.result);
    let id = build_pkg_id_with_patch_hash(ctx, &result).await?;
    if closes_cycle(edge.ancestor_ids, &id) {
        return Ok(NodeSeed::Done(None));
    }
    let alias = node_alias(&wanted, &result, &id);
    let identity = reused_identity(ctx, &id, &result, &reused.key, edge.parent_pkg_aliases);
    let registration = register_reused_package(
        ctx,
        id,
        &result,
        identity.peer_set.clone(),
        current_is_optional,
        identity.is_leaf,
    );
    let id = match registration {
        PackageRegistration::Created(id) => {
            emit_deprecation_if_needed(ctx, &result, &id, edge.depth);
            id
        }
        PackageRegistration::Existing(id) => id,
    };
    let ancestry = snapshot_children::ReusedNodeAncestry::new(edge, &id, current_is_optional);
    Ok(NodeSeed::Reused(Box::new(PendingReusedNode {
        dep: DirectDep { alias, node_id: identity.node_id, id },
        key: reused.key,
        child_refs: identity.child_refs,
        peer_set: identity.peer_set,
        peer_shadowed: identity.peer_shadowed,
        ancestry,
    })))
}

/// Walk the reused node's children from the snapshot graph under the
/// settled `children_pkg_aliases` scope, insert the node, and demote the
/// other occurrences to lazy when this one owns the children.
#[async_recursion]
pub(super) async fn finish_reused_node<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    mut reused: PendingReusedNode,
    children_pkg_aliases: &Arc<ParentPkgAliases>,
) -> Result<(), ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let peer_shadowed = std::mem::take(&mut reused.peer_shadowed);
    let peer_set = std::mem::take(&mut reused.peer_set);
    let children_owner = claim_children_owner(
        ctx,
        &reused.dep.id,
        reused.ancestry.depth,
        &reused.ancestry.parent_ancestors,
        peer_shadowed,
    );
    if children_owner.owns_children {
        install_reused_owner_peer_set(ctx, &reused.dep.id, &peer_set);
    }
    let (children, _recording) =
        reused_children(ctx, resolver, &children_owner, &reused, children_pkg_aliases).await?;
    let depth = reused.ancestry.depth;
    let node_id = reused.dep.node_id;
    let id = reused.dep.id;
    insert_tree_node(ctx, node_id.clone(), &id, children, depth);
    if is_current_children_owner(ctx, &id, &children_owner.owner) {
        make_non_owner_nodes_lazy(ctx, &id, &node_id);
    }
    Ok(())
}

fn install_reused_owner_peer_set(ctx: &TreeCtx, id: &Arc<str>, peer_set: &PeerSet) {
    let mut packages = lock_recoverable(&ctx.workspace.tree.packages);
    let Some(existing) = packages.get_mut(id) else { return };
    if &existing.peer_set == peer_set {
        return;
    }
    existing.peer_set = peer_set.clone();
    drop(packages);
    register_peer_dep_names(ctx, peer_set.taken());
    ctx.workspace.record_package_write(id);
}

/// Insert a reused package into the workspace's package table, answering
/// with the table's `Arc` of the id as a created or an existing entry.
fn register_reused_package(
    ctx: &TreeCtx,
    id: String,
    result: &Arc<pnpm_resolving_resolver_base::ResolveResult>,
    peer_set: PeerSet,
    current_is_optional: bool,
    is_leaf: bool,
) -> PackageRegistration {
    let mut packages = lock_recoverable(&ctx.workspace.tree.packages);
    if let Some(existing) = packages.get_mut(id.as_str()) {
        existing.optional = existing.optional && current_is_optional;
        return PackageRegistration::Existing(Arc::clone(&existing.id));
    }
    register_peer_dep_names(ctx, peer_set.taken());
    ctx.workspace.record_package_write(&id);
    let shared_id: Arc<str> = Arc::from(id);
    packages.insert(
        Arc::<str>::clone(&shared_id),
        ResolvedPackage::new(ResolvedPackageInput {
            id: Arc::<str>::clone(&shared_id),
            result: Arc::clone(result),
            peer_set,
            optional: current_is_optional,
            is_leaf,
        }),
    );
    PackageRegistration::Created(shared_id)
}

#[cfg(test)]
mod tests;
