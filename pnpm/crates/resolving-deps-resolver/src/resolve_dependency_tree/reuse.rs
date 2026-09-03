//! Reuse of the prior lockfile's resolutions: which edges may reuse at
//! all ([`ReuseSource`], the `pacquet update` [`UpdateScope`]), whether
//! a whole subtree is reproducible from the snapshot graph, and the
//! snapshot-driven walk a reused node's children take
//! ([`fn@resolve_reused_node`]).

use async_recursion::async_recursion;
use futures_util::future;
use pipe_trait::Pipe;
use pnpm_lockfile::{
    PkgName, PkgNameVerPeer, ProjectSnapshot, ResolvedDependencyMap, SnapshotDepRef, SnapshotEntry,
};
use pnpm_resolving_resolver_base::{Resolver, WantedDependency};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

use crate::{
    lockfile_reuse::{reusable_importer_dep, synthesize_reused_result},
    node_id::NodeId,
    parent_pkg_aliases::ParentPkgAliases,
    resolved_tree::{DirectDep, PeerDep, ResolvedPackage},
};

use super::{
    ResolveDependencyTreeError, UpdateDepth, UpdateReuseScope, WantedSpec, lock_recoverable,
    manifest::{
        build_pkg_id_with_patch_hash, emit_deprecation_if_needed, extract_peer_dependencies,
    },
    tree_ctx::TreeCtx,
    walk::{node_alias, parent_ids_contain_sequence, resolve_node},
    workspace_ctx::{
        DirectDepVersions, RecordedChildrenContext, claim_children_owner, insert_tree_node,
        is_current_children_owner, lazy_children, make_non_owner_nodes_lazy, record_children,
        remember_node_parent_ids,
    },
};

/// The `pacquet update` scope a node is judged against: which names the
/// update targets, and how deep it reaches.
#[derive(Debug, Clone, Copy)]
pub(super) struct UpdateScope<'a> {
    pub(super) reuse: &'a UpdateReuseScope,
    pub(super) max_depth: UpdateDepth,
}

/// How the current [`fn@resolve_node`] call may reuse the prior
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
        let lockfile = ctx.workspace.wanted_lockfile.as_ref()?;
        match self {
            ReuseSource::Importer { importer_id } => reusable_importer_dep(
                lockfile,
                importer_id,
                wanted.alias.as_deref()?,
                wanted.bare_specifier.as_deref()?,
            ),
            ReuseSource::Transitive { key } => key.clone(),
            ReuseSource::Off => None,
        }
    }

    /// Whether this edge may reuse the prior lockfile's subtree.
    pub(super) fn allows_reuse(&self) -> bool {
        matches!(self, ReuseSource::Importer { .. } | ReuseSource::Transitive { .. })
    }
}

/// Record the importer direct deps whose manifest specifier differs from
/// the prior lockfile's recorded specifier (a new dep counts as changed).
/// See [`WorkspaceTreeCtx::changed_direct_deps`].
///
/// Called for the importer's *manifest* wave only, never for
/// auto-installed peers: hoisted peers have no importer-snapshot entry
/// to compare against, so recording them would mark them "changed" on
/// every install and permanently decline subtree reuse for anything
/// depending on them.
///
/// [`WorkspaceTreeCtx::changed_direct_deps`]: super::WorkspaceTreeCtx::changed_direct_deps
pub(crate) fn record_changed_direct_deps(
    ctx: &TreeCtx,
    importer_id: &str,
    wanted: &[WantedSpec],
) -> HashSet<PkgName> {
    let lockfile = ctx.workspace.wanted_lockfile.as_deref();
    let prior = lockfile.and_then(|lockfile| lockfile.importers.get(importer_id));
    let mut changed = lock_recoverable(&ctx.workspace.changed_direct_deps);
    let bucket = changed.entry(importer_id.to_string()).or_default();
    for (alias, spec, _optional, _injected) in wanted {
        let unchanged = prior
            .and_then(|importer| importer_dep_specifier(importer, alias))
            .is_some_and(|recorded| {
                recorded == spec || catalog_specifier_unchanged(lockfile, recorded, alias, spec)
            });
        if !unchanged && let Ok(name) = alias.parse::<PkgName>() {
            bucket.insert(name);
        }
    }
    bucket.clone()
}

/// Whether a `catalog:`-recorded direct dep still resolves to the same
/// underlying range. The wanted specs reaching
/// [`fn@record_changed_direct_deps`] have their `catalog:` protocol
/// already replaced by the catalog's range
/// ([`fn@resolve_catalog_specifiers`]), while the importer snapshot
/// records the literal `catalog:` / `catalog:<name>` form — comparing
/// those directly would mark every catalog-managed dep as changed on
/// every install, and the changed-direct-dep reuse gate would then
/// re-resolve (and drift) every subtree depending on one. The edge is
/// unchanged when the lockfile's `catalogs:` snapshot recorded the same
/// range for this alias.
///
/// [`fn@resolve_catalog_specifiers`]: super::resolve_catalog_specifiers
fn catalog_specifier_unchanged(
    lockfile: Option<&pnpm_lockfile::Lockfile>,
    recorded: &str,
    alias: &str,
    resolved_spec: &str,
) -> bool {
    let Some(catalog_name) = recorded.strip_prefix("catalog:") else {
        return false;
    };
    let catalog_name = if catalog_name.is_empty() { "default" } else { catalog_name };
    lockfile
        .and_then(|lockfile| lockfile.catalogs.as_ref())
        .and_then(|catalogs| catalogs.get(catalog_name))
        .and_then(|catalog| catalog.get(alias))
        .is_some_and(|entry| entry.specifier == resolved_spec)
}

/// The recorded specifier for direct-dep `alias` across the importer's
/// prod / dev / optional dependency maps in the prior lockfile.
fn importer_dep_specifier<'a>(importer: &'a ProjectSnapshot, alias: &str) -> Option<&'a str> {
    let name: PkgName = alias.parse().ok()?;
    let lookup = |map: Option<&'a ResolvedDependencyMap>| map.and_then(|deps| deps.get(&name));
    lookup(importer.dependencies.as_ref())
        .or_else(|| lookup(importer.optional_dependencies.as_ref()))
        .or_else(|| lookup(importer.dev_dependencies.as_ref()))
        .map(|dep| dep.specifier.as_str())
}

/// Store the importer's resolved (parsed) direct-dep versions for the
/// per-edge stale-pin refresh. See [`WorkspaceTreeCtx::direct_dep_versions`].
///
/// [`WorkspaceTreeCtx::direct_dep_versions`]: super::WorkspaceTreeCtx::direct_dep_versions
pub(super) fn record_direct_dep_versions(
    ctx: &TreeCtx,
    importer_id: &str,
    level: &BTreeMap<String, Vec<String>>,
) {
    let mut versions = lock_recoverable(&ctx.workspace.direct_dep_versions);
    let by_name = Arc::make_mut(versions.entry(importer_id.to_string()).or_default());
    for (name, level_versions) in level {
        let bucket = by_name.entry(name.clone()).or_default();
        for version in level_versions {
            let Ok(parsed) = version.parse::<node_semver::Version>() else { continue };
            if !bucket.contains(&parsed) {
                bucket.push(parsed);
            }
        }
    }
}

/// True when `snapshot` depends on one of this importer's changed direct
/// deps (see [`WorkspaceTreeCtx::changed_direct_deps`]).
///
/// [`WorkspaceTreeCtx::changed_direct_deps`]: super::WorkspaceTreeCtx::changed_direct_deps
fn reused_parent_has_changed_direct_child(ctx: &TreeCtx, snapshot: &SnapshotEntry) -> bool {
    // Copy the (small) changed set out and drop the lock before scanning.
    let importer_changed = {
        let changed = lock_recoverable(&ctx.workspace.changed_direct_deps);
        match changed.get(&ctx.importer_id) {
            Some(set) if !set.is_empty() => set.clone(),
            _ => return false,
        }
    };
    let depends_on = |map: Option<&std::collections::HashMap<PkgName, SnapshotDepRef>>| {
        map.is_some_and(|deps| deps.keys().any(|name| importer_changed.contains(name)))
    };
    depends_on(snapshot.dependencies.as_ref())
        || depends_on(snapshot.optional_dependencies.as_ref())
}

/// Reuse-decline gate: whether `prior_key`'s prior snapshot depends on a
/// changed direct dep.
pub(super) fn node_depends_on_changed_direct_dep(
    ctx: &TreeCtx,
    prior_key: Option<&PkgNameVerPeer>,
) -> bool {
    prior_key
        .and_then(|key| ctx.workspace.wanted_lockfile.as_ref()?.snapshots.as_ref()?.get(key))
        .is_some_and(|snapshot| reused_parent_has_changed_direct_child(ctx, snapshot))
}

/// The highest resolved direct-dependency version of `name` strictly
/// above `pinned` that still satisfies `range`, or `None`. Anchored to
/// direct deps (the deterministic, resolved-before-the-walk signal).
/// `direct_versions` is the importer's snapshot, taken once per walked
/// occurrence as it seeds its children.
pub(super) fn higher_direct_dep_version(
    direct_versions: Option<&DirectDepVersions>,
    name: &str,
    pinned: &node_semver::Version,
    range: &node_semver::Range,
) -> Option<node_semver::Version> {
    direct_versions?
        .get(name)?
        .iter()
        .filter(|&version| version > pinned && range.satisfies(version))
        .max()
        .cloned()
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
    let lockfile = ctx.workspace.wanted_lockfile.as_ref()?;
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

/// `true` when a node named `name`, locked at `version`, is a `pacquet
/// update` target at `depth`, and so excluded from reuse. A `None` version
/// is judged by name alone -- see [`crate::UpdateTargets::covers`]. Past the
/// `--depth` ceiling the update no longer reaches, so every node keeps its
/// locked resolution.
fn update_excludes(
    scope: UpdateScope<'_>,
    name: &str,
    version: Option<&node_semver::Version>,
    depth: i32,
) -> bool {
    if !scope.max_depth.reaches(depth) {
        return false;
    }
    match scope.reuse {
        UpdateReuseScope::All => false,
        // `None` is handled earlier in `try_reuse_node`; treat it the
        // same here for completeness.
        UpdateReuseScope::None => true,
        UpdateReuseScope::Except(targets) => targets.covers(name, version),
    }
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
    let Some(alias) = wanted.alias.as_deref().filter(|alias| !alias.is_empty()) else {
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
    packages.keys().any(|key| {
        key.name == pkg_name
            && key.suffix.version_semver().is_some_and(|version| range.satisfies(version))
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
        let alias_keeps_name = alias
            .is_some_and(|alias| !alias.is_empty() && rest.parse::<node_semver::Range>().is_ok());
        if !alias_keeps_name {
            let last_at =
                rest.bytes().enumerate().rev().find_map(|(i, b)| (b == b'@').then_some(i));
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

/// Whether the running `pacquet update` reaches this edge, so its
/// locked-version pin must not survive — the update exists to move it.
/// Unlike [`fn@is_update_target`], an update-everything scope
/// ([`UpdateReuseScope::None`]) unpins every edge the depth ceiling
/// reaches.
pub(super) fn update_unpins_edge(
    scope: UpdateScope<'_>,
    wanted: &WantedDependency,
    locked_version: Option<&node_semver::Version>,
    depth: i32,
) -> bool {
    if !scope.max_depth.reaches(depth) {
        return false;
    }
    match scope.reuse {
        UpdateReuseScope::All => false,
        UpdateReuseScope::None => true,
        UpdateReuseScope::Except(_) => {
            real_package_name_of(wanted.alias.as_deref(), wanted.bare_specifier.as_deref())
                .is_some_and(|name| update_excludes(scope, name.as_ref(), locked_version, depth))
        }
    }
}

/// Whether `wanted` is one of the packages the user asked to update,
/// given the install's [`UpdateReuseScope`]. Feeds the per-resolve
/// `ResolveOptions::update_requested` flag, which gates the npm
/// picker's held-back-update warning.
#[inline]
pub(super) fn is_update_target(
    scope: UpdateScope<'_>,
    wanted: &WantedDependency,
    locked_version: Option<&node_semver::Version>,
    depth: i32,
) -> bool {
    if !scope.max_depth.reaches(depth) {
        return false;
    }
    match scope.reuse {
        UpdateReuseScope::All | UpdateReuseScope::None => false,
        UpdateReuseScope::Except(_) => {
            real_package_name_of(wanted.alias.as_deref(), wanted.bare_specifier.as_deref())
                .is_some_and(|name| update_excludes(scope, name.as_ref(), locked_version, depth))
        }
    }
}

/// `true` when `key` and its entire transitive subtree can be
/// synthesized from `lockfile` (every node a plain-semver registry
/// package present in `packages:`, every snapshot child non-`link:`).
/// Memoised on [`WorkspaceTreeCtx::subtree_reusable`] so each package is
/// checked once.
///
/// A snapshot cycle is treated as **non**-reusable at the back-edge: the
/// key is provisionally inserted as `false` before recursing, so a node
/// reached through a still-in-progress ancestor resolves to `false` and
/// any subtree containing a dependency cycle conservatively re-resolves.
/// This avoids the unsound alternative — a provisional `true` could cache
/// a cycle member as reusable based on an ancestor that later finalizes
/// `false` (e.g. an update-excluded target reachable only through the
/// cycle), wrongly reusing it. SCC-aware reuse of acyclic-equivalent
/// cycles is possible but not worth the complexity for an uncommon case.
///
/// [`WorkspaceTreeCtx::subtree_reusable`]: super::WorkspaceTreeCtx::subtree_reusable
fn subtree_fully_reusable(
    ctx: &TreeCtx,
    lockfile: &pnpm_lockfile::Lockfile,
    key: &PkgNameVerPeer,
    depth: i32,
) -> bool {
    let scope = ctx.update_scope();
    let memo_key = (ctx.update_cache_scope(), key.clone(), scope.max_depth.memo_bucket(depth));
    if let Some(&cached) = lock_recoverable(&ctx.workspace.subtree_reusable).get(&memo_key) {
        return cached;
    }
    // Provisionally mark non-reusable so a cycle back to `key` resolves to
    // `false` (re-resolve) instead of recursing forever — see the doc above
    // for why `false` rather than `true`.
    lock_recoverable(&ctx.workspace.subtree_reusable).insert(memo_key.clone(), false);
    // A `pacquet update` target anywhere in the subtree forces the whole
    // subtree to re-resolve so the bump's new transitive deps are picked
    // up — update names match at every depth the update reaches.
    let name = key.name.to_string();
    let reusable = !update_excludes(scope, &name, key.suffix.version_semver(), depth)
        && synthesize_reused_result(lockfile, key, &name).is_some()
        && subtree_children_reusable(ctx, lockfile, key, depth);
    lock_recoverable(&ctx.workspace.subtree_reusable).insert(memo_key, reusable);
    reusable
}

/// Recurse [`fn@subtree_fully_reusable`] across `key`'s snapshot
/// children. A `link:` child (no snapshot key) makes the subtree
/// non-reusable: the linked importer resolves its own deps, which this
/// reuse path doesn't model.
fn subtree_children_reusable(
    ctx: &TreeCtx,
    lockfile: &pnpm_lockfile::Lockfile,
    key: &PkgNameVerPeer,
    depth: i32,
) -> bool {
    let Some(snapshot) = lockfile.snapshots.as_ref().and_then(|snaps| snaps.get(key)) else {
        // No snapshot entry → the lockfile doesn't record this node's
        // children, so the reuse walk can't reproduce its subtree.
        // Force a fresh resolve rather than risk silently dropping
        // transitive deps. A genuine leaf has an empty-but-*present*
        // snapshot entry (`{}`); a missing one means an inconsistent
        // lockfile, which `try_reuse_node`'s contract sends to a fresh
        // resolve.
        return false;
    };
    let dep_maps = [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()];
    for dep_map in dep_maps.into_iter().flatten() {
        for (child_name, dep_ref) in dep_map {
            let Some(child_key) = dep_ref.resolve(child_name) else {
                return false;
            };
            if !subtree_fully_reusable(ctx, lockfile, &child_key, depth + 1) {
                return false;
            }
        }
    }
    true
}

/// Register a node whose resolution was reused from the prior lockfile,
/// then walk its transitive children from the snapshot graph instead of
/// re-resolving them. Mirrors the post-resolve half of
/// [`fn@resolve_node`], specialized for a node whose subtree
/// [`fn@try_reuse_node`] already confirmed reusable.
#[async_recursion]
#[expect(
    clippy::too_many_arguments,
    reason = "internal walker helper threading per-node context through the recursion"
)]
pub(super) async fn resolve_reused_node<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: WantedDependency,
    ancestor_ids: &Arc<Vec<String>>,
    depth: i32,
    current_is_optional: bool,
    reused: ReusedNode,
    parent_pkg_aliases: &Arc<ParentPkgAliases>,
) -> Result<Option<DirectDep>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let ReusedNode { key, result } = reused;
    let result = Arc::new(result);

    // The synthesized result must stay out of `resolved_by_wanted`: its
    // manifest deliberately omits `dependencies` (a reused node's
    // children come from the snapshot graph), so if the fresh-resolve
    // path ever read it back — e.g. an identical edge denied reuse by
    // the changed-direct-dep gate or by `subtree_fully_reusable`'s
    // provisional-`false` cycle guard — `extract_children` /
    // `pkg_is_leaf` would misread the package as dependency-less and
    // record it as a leaf, emptying its lockfile snapshot.

    let id = build_pkg_id_with_patch_hash(ctx, &result).await?;

    // Cycle break — same as the fresh path.
    if ancestor_ids.last().is_some_and(|parent| {
        *parent == id || parent_ids_contain_sequence(ancestor_ids, parent, &id)
    }) {
        return Ok(None);
    }

    let alias = node_alias(&wanted, &result, &id);

    // Leaf classification reads the snapshot graph (the source of truth
    // for a reused node's children), not the synthesized manifest (whose
    // `dependencies` are deliberately omitted). A node with no recorded
    // children and no peers is a leaf, matching `pkg_is_leaf`.
    let snapshot = ctx
        .workspace
        .wanted_lockfile
        .as_ref()
        .and_then(|lockfile| lockfile.snapshots.as_ref())
        .and_then(|snaps| snaps.get(&key));
    // A reused node's children come from the snapshot rather than from
    // a manifest, so no `dependencies` entry of its synthesized
    // manifest can be peer-shadowed.
    let peer_dependencies = extract_peer_dependencies(&result, &HashSet::default(), None)?;
    let child_refs = snapshot_child_refs(snapshot, &peer_dependencies);
    let is_leaf = child_refs.is_empty() && peer_dependencies.is_empty();
    let node_id = if is_leaf { NodeId::leaf(&id) } else { NodeId::next() };

    let package_is_new = {
        let mut packages = lock_recoverable(&ctx.workspace.packages);
        if let Some(existing) = packages.get_mut(id.as_str()) {
            existing.optional = existing.optional && current_is_optional;
            false
        } else {
            {
                let mut all_peers = lock_recoverable(&ctx.workspace.all_peer_dep_names);
                for name in peer_dependencies.keys() {
                    if all_peers.insert(name.clone()) {
                        ctx.workspace.record_peer_dep_name(name);
                    }
                }
            }
            ctx.workspace.record_package_write(&id);
            let shared_id: Arc<str> = Arc::from(id.as_str());
            packages.insert(
                Arc::<str>::clone(&shared_id),
                ResolvedPackage {
                    id: shared_id,
                    result: Arc::clone(&result),
                    peer_dependencies,
                    optional: current_is_optional,
                    is_leaf,
                },
            );
            true
        }
    };

    if package_is_new {
        emit_deprecation_if_needed(ctx, &result, &id, depth);
    }

    let next_ancestors: Vec<String> =
        ancestor_ids.iter().cloned().chain(std::iter::once(id.clone())).collect();
    let next_ancestors = Arc::new(next_ancestors);
    let children_owner = claim_children_owner(ctx, &id, depth, ancestor_ids, HashSet::default());

    let (children, others_stale) = if children_owner.owns_children {
        let child_results = child_refs
            .iter()
            .map(|(child_alias, child_key)| {
                let child_wanted = WantedDependency {
                    alias: Some(child_alias.clone()),
                    // The snapshot pins the exact version; carry it as
                    // the bare specifier so the per-wanted dedup cache
                    // key is stable and a fresh fallback (if reuse were
                    // ever disabled) would still target the right pin.
                    bare_specifier: Some(child_key.suffix.without_peer().to_string()),
                    ..WantedDependency::default()
                };
                let next_ancestors = Arc::clone(&next_ancestors);
                let child_key = child_key.clone();
                async move {
                    resolve_node(
                        ctx,
                        resolver,
                        child_wanted,
                        &next_ancestors,
                        depth + 1,
                        current_is_optional,
                        ReuseSource::Transitive { key: Some(child_key) },
                        parent_pkg_aliases,
                    )
                    .await
                }
            })
            .pipe(future::try_join_all)
            .await?;
        if is_current_children_owner(ctx, &id, &children_owner.owner) {
            let mut realized: BTreeMap<String, NodeId> = BTreeMap::new();
            let mut by_id: Vec<crate::resolved_tree::ChildEdge> = Vec::new();
            let optional_by_alias: HashMap<&str, bool> = child_refs
                .iter()
                .map(|(alias, _)| (alias.as_str(), is_optional_child(snapshot, alias)))
                .collect();
            for dep in child_results.into_iter().flatten() {
                let optional = optional_by_alias.get(dep.alias.as_str()).copied().unwrap_or(false);
                by_id.push(crate::resolved_tree::ChildEdge {
                    alias: dep.alias.clone(),
                    pkg_id: Arc::from(dep.id),
                    optional,
                });
                realized.insert(dep.alias, dep.node_id);
            }
            let recording = record_children(
                ctx,
                &id,
                &children_owner.owner,
                by_id,
                RecordedChildrenContext {
                    peer_shadowed: Arc::clone(&children_owner.peer_shadowed),
                    prior_key: Some(key.clone()),
                    update_active: !matches!(ctx.update_reuse_scope(), UpdateReuseScope::All),
                },
            );
            recording.into_children(realized, ancestor_ids)
        } else {
            (lazy_children(ancestor_ids), false)
        }
    } else {
        (lazy_children(ancestor_ids), false)
    };

    remember_node_parent_ids(ctx, &node_id, Arc::clone(ancestor_ids));
    insert_tree_node(ctx, node_id.clone(), &id, children, depth);
    if children_owner.owns_children
        && (others_stale || !children_owner.children_context_unchanged)
        && is_current_children_owner(ctx, &id, &children_owner.owner)
    {
        make_non_owner_nodes_lazy(ctx, &id, &node_id);
    }

    Ok(Some(DirectDep { alias, node_id, id }))
}

/// `(install_alias, resolved_snapshot_key)` for every non-`link:` child
/// recorded on `snapshot`'s `dependencies` + `optionalDependencies`,
/// excluding resolved peers. Sorted by alias so the per-occurrence walk
/// order is deterministic.
///
/// A snapshot's `dependencies` map lists not only the package's real
/// dependencies but also every *resolved peer* — the node's own peers
/// (`peer_dependencies`) and the peers its descendants required and
/// resolved through this node (`transitivePeerDependencies`) — each
/// pinned to the version it matched in the recorded context. Those are
/// not real children: a fresh resolve walks only the package's manifest
/// `dependencies` and re-derives peers separately against the parent
/// context. Reuse must walk the manifest's deps too — so peer-named
/// entries are dropped here, and the snapshot's `dependencies` is used
/// only as the locked-ref lookup, not as the child set. Treating a
/// resolved peer as a regular child makes the peer pass satisfy the peer
/// from the node's own subtree instead of propagating it up, collapsing
/// the peer-context suffix.
fn snapshot_child_refs(
    snapshot: Option<&SnapshotEntry>,
    peer_dependencies: &BTreeMap<String, PeerDep>,
) -> Vec<(String, PkgNameVerPeer)> {
    let Some(snapshot) = snapshot else { return Vec::new() };
    let transitive_peers: HashSet<&str> =
        snapshot.transitive_peer_dependencies.iter().flatten().map(String::as_str).collect();
    let mut out: Vec<(String, PkgNameVerPeer)> = Vec::new();
    for dep_map in [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
        .into_iter()
        .flatten()
    {
        for (alias, dep_ref) in dep_map {
            let alias_str = match &alias.scope {
                Some(scope) => Cow::Owned(format!("@{scope}/{}", alias.bare)),
                None => Cow::Borrowed(alias.bare.as_str()),
            };
            if peer_dependencies.contains_key(alias_str.as_ref())
                || transitive_peers.contains(alias_str.as_ref())
            {
                continue;
            }
            if let Some(key) = dep_ref.resolve(alias) {
                out.push((alias_str.into_owned(), key));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// `true` when `alias` is recorded under `snapshot.optionalDependencies`
/// (as opposed to `dependencies`). Threads the right `optional` flag onto
/// the reused child's [`crate::resolved_tree::ChildEdge`].
fn is_optional_child(snapshot: Option<&SnapshotEntry>, alias: &str) -> bool {
    let Some(snapshot) = snapshot else { return false };
    let Ok(name) = alias.parse::<pnpm_lockfile::PkgName>() else { return false };
    snapshot.optional_dependencies.as_ref().is_some_and(|deps| deps.contains_key(&name))
}

#[cfg(test)]
mod tests;
