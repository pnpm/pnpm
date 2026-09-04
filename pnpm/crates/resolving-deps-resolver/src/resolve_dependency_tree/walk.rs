//! The fresh-resolve walk: seeding one edge's package
//! ([`fn@resolve_node_seed`]), seeding a settled occurrence's children
//! ([`fn@seed_node_children`]), and the per-wanted resolve cache both
//! go through. Seeding and settling are separate phases so a whole
//! depth level resolves before any of it is settled — which is what
//! makes children ownership, and with it the lockfile, independent of
//! the order concurrent subtrees finish in ([`fn@walk_from_seeds`]).

use async_recursion::async_recursion;
use futures_util::future;
use pipe_trait::Pipe;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::{LockfileResolution, PkgNameVerPeer, TarballRevision};
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
        ancestor_ids,
        depth,
        parent_optional,
        reuse,
        base_overlay.clone(),
        None,
        parent_pkg_aliases,
        false,
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

/// Resolve one `(alias, range)` edge and register the resolved package
/// in the dedup map if absent, run for a whole sibling level before any
/// child subtree starts.
///
/// `pick_overlay` carries the per-level preferred-version additions
/// (the parent level's resolved versions) consulted by the npm
/// resolver's version pick; it participates in the per-wanted dedup
/// cache key so the same range can legitimately pick different
/// versions under different levels, layering each level's resolved
/// versions onto the preferred-versions fold.
///
/// `ancestor_ids` is the chain of `pkgIdWithPatchHash` values from the
/// root importer down to the current node's parent. When the resolved
/// id appears in the chain, this call is a cycle re-entry: the edge is
/// dropped entirely (returns `Done(None)`) so the parent's `children`
/// map omits the cycled child. Without this, two nodes for the same id
/// race each other into `graph.insert`, and an empty-children entry for
/// the cycled occurrence can overwrite the real one.
///
/// `parent_dir` is the directory of the manifest that declares this
/// edge, when that manifest is a package resolved from a local
/// directory — see [`declaring_manifest_dir`].
///
/// `parent_pkg_aliases` is the scope this edge's level resolves in; it
/// decides which of the resolved package's `dependencies` its own
/// `peerDependencies` shadow (see [`peer_shadowed_dependencies`]).
#[expect(
    clippy::too_many_arguments,
    reason = "internal walker helper threading per-node context through the recursion"
)]
#[async_recursion]
pub(super) async fn resolve_node_seed<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: WantedDependency,
    ancestor_ids: &Arc<Vec<String>>,
    depth: i32,
    parent_optional: bool,
    reuse: ReuseSource,
    pick_overlay: Option<Arc<PreferredVersionsOverlay>>,
    parent_dir: Option<&Path>,
    parent_pkg_aliases: &Arc<ParentPkgAliases>,
    parent_is_workspace: bool,
) -> Result<NodeSeed, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let current_is_optional = wanted.optional.unwrap_or(false) || parent_optional;

    // The edge's recorded snapshot key in the prior lockfile, if any.
    // Feeds both subtree reuse (below) and — when the edge re-resolves
    // anyway — the `currentPkg` payload custom resolvers receive.
    let prior_key = reuse.prior_key(ctx, &wanted);

    // **Lockfile-resolution reuse.** When the prior lockfile already
    // resolved this edge (and the recorded version still satisfies the
    // manifest range, for a direct dep), synthesize the resolution from
    // the lockfile and walk its transitive subtree from the snapshot
    // graph instead of re-resolving from the registry.
    // `synthesize_reused_result` is conservative: any shape it can't
    // faithfully reproduce (non-registry resolutions, missing metadata)
    // yields `None` here and the node falls through to a fresh resolve.
    //
    // Stale-pin refresh: a node depending on a changed direct dep is
    // resolved fresh rather than reused, so its children walk against
    // their manifest ranges where `seed_node_children` can redirect a
    // stale pin onto the higher direct-dep version (reusing the subtree
    // would keep the pin, leaving the lockfile non-convergent).
    if ctx.workspace.reuse_lockfile_subtrees
        && reuse.allows_reuse()
        && !node_depends_on_changed_direct_dep(ctx, prior_key.as_ref())
        && let Some(reused) = try_reuse_node(ctx, &wanted, prior_key.as_ref(), depth)
    {
        return resolve_reused_node(
            ctx,
            resolver,
            wanted,
            ancestor_ids,
            depth,
            current_is_optional,
            reused,
            parent_pkg_aliases,
        )
        .await
        .map(NodeSeed::Done);
    }

    // Locked-version pin, the fresh-resolve counterpart of subtree
    // reuse: a transitive edge whose recorded version still satisfies
    // its manifest range (`prior_key` is satisfies-gated) resolves to
    // exactly that version even when its subtree cannot be reused
    // wholesale. Without it, a re-resolve picks open ranges (`*`)
    // against the whole preferred-versions pool and lands every such
    // edge on the highest locked version, churning the lockfile.
    // Mirrors the TypeScript resolver's `replaceVersionInBareSpecifier`
    // under `!update`: direct deps (depth 0) keep recomputing their
    // specifier, and an edge a `pacquet update` reaches keeps
    // re-picking. Only plain semver ranges pin; aliased (`npm:`),
    // named-registry, and exotic specifiers keep today's behavior.
    let mut wanted = wanted;
    let locked_version = prior_key.as_ref().and_then(|key| key.suffix.version_semver());
    if depth > 0
        && !update_unpins_edge(ctx.update_scope(), &wanted, locked_version, depth)
        && let Some(version) = locked_version
        && wanted
            .bare_specifier
            .as_deref()
            .is_some_and(|spec| spec.parse::<node_semver::Range>().is_ok())
    {
        wanted.bare_specifier = Some(version.to_string());
    }

    // Memoise the per-wanted resolve. The first caller for a given
    // `(alias, bare_specifier, optional, injected)` runs the resolver chain and
    // stores the `Arc<ResolveResult>` on `ctx.resolved_by_wanted`;
    // every later caller for the same wanted dep clones the `Arc` and
    // skips the chain entirely. Concurrent first-callers can both miss
    // the cache and run `resolver.resolve` in parallel — the resolver's
    // own per-cache-key semaphore (`pick_package::fetch_locker`)
    // already coalesces those into a single network fetch, so the
    // doubled work is bounded to in-memory packument lookups + semver
    // matching, and the second to finish loses the `insert` race
    // harmlessly (the entry holds an `Arc` to an equivalent
    // `ResolveResult`).
    // `resolutionMode` makes the version pick depend on whether this is
    // a direct (`depth == 0`) or transitive dep, so the cache key and
    // the resolver call both key off the depth-specific options.
    let opts = ctx.opts_for_depth(depth);
    // The prior lockfile entry rides along as `currentPkg`, handed to
    // the resolver. Only custom resolvers read it today; the clone of
    // the shared per-depth options is paid only when a prior entry
    // exists for a freshly resolving edge.
    let current_pkg = prior_key.as_ref().and_then(|key| {
        let lockfile = ctx.workspace.wanted_lockfile.as_ref()?;
        current_pkg_from_lockfile(lockfile, key, &ctx.workspace.registry_context)
    });
    if opts.update == UpdateBehavior::Patches
        && let Some(version) = current_pkg.as_ref().and_then(|current| current.version.as_deref())
        && let Some(specifier) = wanted.bare_specifier.as_deref()
    {
        wanted.bare_specifier = exact_registry_specifier_for_revision_refresh(
            specifier,
            version,
            prior_key
                .as_ref()
                .and_then(|key| key.suffix.registry_qualified().map(|(name, _)| name)),
        )
        .into();
    }
    let opts_with_current_pkg;
    let opts = match current_pkg {
        Some(current_pkg) => {
            opts_with_current_pkg =
                ResolveOptions { current_pkg: Some(current_pkg), ..opts.clone() };
            &opts_with_current_pkg
        }
        None => opts,
    };
    let opts_for_file_dep = opts_relative_to_declaring_manifest(opts, &wanted, parent_dir);
    let opts = opts_for_file_dep.as_ref();
    // Project-relative resolutions (`link:`/`file:`/`workspace:`) are
    // keyed by the consuming importer so one importer's relative path
    // is never reused by another. See [`WantedKey`]. The prior key
    // joins so two edges that share a specifier but recorded different
    // versions never share a `currentPkg`-dependent result.
    let project_scope = project_relative_cache_scope(&wanted, opts);
    // The overlay's view for this edge joins the cache key: the same
    // range can legitimately pick different versions under levels
    // that resolved different siblings. The view keeps each candidate
    // name (alias, `npm:` inner target, folded `jsr:` name) paired
    // with its versions — the picker consults the overlay per name,
    // so a flat union of versions could collide two overlays that
    // distribute the same versions across different names. Empty for
    // almost every edge, so the dedup keeps working where it matters.
    let overlay_versions: Vec<(String, Vec<String>)> = pick_overlay
        .as_ref()
        .map(|overlay| {
            let mut view: Vec<(String, Vec<String>)> =
                overlay_lookup_names(wanted.alias.as_deref(), wanted.bare_specifier.as_deref())
                    .into_iter()
                    .flatten()
                    .filter_map(|name| {
                        let mut versions: Vec<String> =
                            overlay.versions_for(&name).into_iter().map(str::to_string).collect();
                        if versions.is_empty() {
                            return None;
                        }
                        versions.sort_unstable();
                        versions.dedup();
                        Some((name.into_owned(), versions))
                    })
                    .collect();
            view.sort_unstable();
            view
        })
        .unwrap_or_default();
    let update_target = is_update_target(
        ctx.update_scope(),
        &wanted,
        prior_key.as_ref().and_then(|key| key.suffix.version_semver()),
        depth,
    );
    let cache_key = WantedKey::new((
        wanted.alias.clone(),
        wanted.bare_specifier.clone(),
        wanted.optional,
        wanted.injected,
        opts.pick_lowest_version,
        opts.published_by,
        project_scope,
        prior_key.clone(),
        overlay_versions,
        ctx.update_cache_scope(),
        update_target,
    ));
    let result =
        match resolve_wanted_cached(ctx, resolver, &wanted, opts, pick_overlay.as_ref(), cache_key)
            .await
        {
            Ok(result) => result,
            // A resolution failure on an optional edge drops the edge
            // instead of failing the install — unless the wanted lockfile
            // already holds a satisfying entry, where the silent skip would
            // erase the locked entries (see
            // `wanted_lockfile_contains_satisfying_entry`). Hook errors
            // keep aborting even for optional edges.
            Err(
                err @ (ResolveDependencyTreeError::Resolve(_)
                | ResolveDependencyTreeError::NoMatchingVersion(_)
                | ResolveDependencyTreeError::RegistryResponse(_)
                | ResolveDependencyTreeError::GitResolve(_)
                | ResolveDependencyTreeError::SpecNotSupported { .. }),
            ) if wanted.optional.unwrap_or(false) => {
                if wanted_lockfile_contains_satisfying_entry(
                    ctx.workspace.wanted_lockfile.as_deref(),
                    &wanted,
                ) {
                    return Err(ResolveDependencyTreeError::LockedOptionalResolutionFailure(
                        Box::new(err),
                    ));
                }
                if let Some(log) = ctx.workspace.skipped_optional_log.as_ref() {
                    log(SkippedOptionalDependency {
                        details: err.to_string(),
                        name: wanted.alias.clone(),
                        version: wanted
                            .alias
                            .is_some()
                            .then(|| wanted.bare_specifier.clone())
                            .flatten(),
                        bare_specifier: wanted.bare_specifier.clone().unwrap_or_default(),
                        parents: pkgs_info_from_ids(ctx, ancestor_ids),
                        prefix: opts.project_dir.display().to_string(),
                    });
                }
                return Ok(NodeSeed::Done(None));
            }
            Err(err) => return Err(err),
        };

    if let Some(violation) = result.policy_violation.clone() {
        lock_recoverable(&ctx.workspace.policy_violations).push(violation);
    }

    if ctx.base_opts.block_exotic_subdeps
        && depth > 0
        && !parent_is_workspace
        && is_exotic_resolved_via(&result.resolved_via)
    {
        return Err(ResolveDependencyTreeError::ExoticSubdep {
            specifier: wanted
                .alias
                .clone()
                .or_else(|| wanted.bare_specifier.clone())
                .unwrap_or_default(),
            resolved_via: result.resolved_via.clone(),
        });
    }

    let id = build_pkg_id_with_patch_hash(ctx, &result).await?;

    if result.name_ver.is_none()
        && wanted.bare_specifier.as_deref().is_some_and(|specifier| {
            specifier.starts_with("workspace:") && !specifier.starts_with("workspace:.")
        })
        && let Some(manifest) = result.manifest.as_deref()
        && let (Some(name), Some(version)) = (
            manifest.get("name").and_then(Value::as_str),
            manifest.get("version").and_then(Value::as_str),
        )
    {
        ctx.workspace.record_workspace_manifest_identity(&id, name, version);
    }

    // Cycle break — see the doc comment above. A direct self-edge and
    // the second lap of a longer cycle are dropped; the first re-entry
    // is kept so the cycle-closing edge reaches the lockfile snapshot.
    if ancestor_ids.last().is_some_and(|parent| {
        *parent == id || parent_ids_contain_sequence(ancestor_ids, parent, &id)
    }) {
        return Ok(NodeSeed::Done(None));
    }

    let alias = node_alias(&wanted, &result, &id);

    // Build (or look up) the ResolvedPackage envelope. The first
    // visitor populates it; later visitors AND-fold the `optional`
    // flag so a single non-optional path flips it back to `false`.
    // Child traversal is claimed first, and a later
    // deterministically-better occurrence replaces the shared
    // `children_by_id` entry — plus, since the two are two halves of
    // one manifest reading, the envelope's peer dependencies.
    // Leaves (no deps / optional deps / peers / peerDependenciesMeta)
    // reuse the package id as their `NodeId`, collapsing every parent
    // edge onto one tree node. Non-leaves still get a fresh per-
    // occurrence id so the peer resolver can attach different peer
    // suffixes per call site.
    // The leaf flag is computed before the dedup insert so it can be
    // persisted on [`ResolvedPackage::is_leaf`] for the lazy realisation
    // path to read back.
    // Workspace-link nodes get empty children (the linked project
    // resolves its own deps as a separate importer), `depth = -1` flags
    // the node for the peer-resolution short-circuit, and the
    // [`ResolvedPackage`] carries no peer dependencies (peer matching is
    // the linked importer's responsibility, not the parent's). The node
    // id is collapsed to a leaf so every reference to the same workspace
    // path shares one [`NodeId`].
    let is_link = id.starts_with("link:");
    let resolves_children_through_catalogs = resolves_children_through_catalogs(&result);
    let is_leaf = is_link || pkg_is_leaf(&result);
    let node_id = if is_leaf { NodeId::leaf(&id) } else { NodeId::next() };

    let peer_shadowed = peer_shadowed_dependencies(
        result.manifest.as_deref(),
        parent_pkg_aliases,
        ctx.workspace.auto_install_peers,
    );
    // The envelope's peer split follows the occurrence that owns the
    // package's children, which this level's settlement decides — see
    // [`fn@install_owner_peer_dependencies`]. Seeding only has to fill
    // a package nothing has resolved yet.
    let mut packages = lock_recoverable(&ctx.workspace.packages);
    let package_is_new = if let Some(existing) = packages.get_mut(id.as_str()) {
        if registry_revisions_conflict(&existing.result.resolution, &result.resolution) {
            let name_ver = result.name_ver.as_ref().expect("registry result has name and version");
            return Err(ResolveDependencyTreeError::RevisionConflict {
                name: name_ver.name.to_string(),
                version: name_ver.suffix.to_string(),
            });
        }
        existing.optional = existing.optional && current_is_optional;
        false
    } else {
        let peer_dependencies = if is_link {
            BTreeMap::new()
        } else {
            extract_peer_dependencies(
                &result,
                &peer_shadowed,
                catalogs_for_children(ctx, resolves_children_through_catalogs),
            )?
        };
        register_peer_dep_names(ctx, &peer_dependencies);
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
    };
    drop(packages);

    if package_is_new {
        emit_deprecation_if_needed(ctx, &result, &id, depth);
    }

    let next_ancestors: Vec<String> =
        ancestor_ids.iter().cloned().chain(std::iter::once(id.clone())).collect();
    let next_ancestors = Arc::new(next_ancestors);

    Ok(NodeSeed::Pending(Box::new(PendingNode {
        result,
        id,
        alias,
        node_id,
        is_link,
        resolves_children_through_catalogs,
        parent_ancestors: Arc::clone(ancestor_ids),
        next_ancestors,
        peer_shadowed,
        claim: None,
        depth,
        current_is_optional,
        prior_key,
    })))
}

fn exact_registry_specifier_for_revision_refresh(
    specifier: &str,
    version: &str,
    named_registry: Option<&str>,
) -> String {
    if has_registry_revision_specifier(specifier) {
        return specifier.to_string();
    }
    if specifier.parse::<node_semver::Range>().is_ok() {
        return version.to_string();
    }
    let Some((protocol, body)) = specifier.split_once(':') else {
        return specifier.to_string();
    };
    if protocol != "npm" && protocol != "jsr" && named_registry != Some(protocol) {
        return specifier.to_string();
    }
    if body.parse::<node_semver::Range>().is_ok() {
        return format!("{protocol}:{version}");
    }
    let Some(delimiter) = body.rfind('@').filter(|index| *index > 0) else {
        return format!("{protocol}:{body}@{version}");
    };
    format!("{protocol}:{}@{version}", &body[..delimiter])
}

fn has_registry_revision_specifier(specifier: &str) -> bool {
    let selector_start =
        specifier.rfind([':', '@']).map_or(0, |delimiter| delimiter.saturating_add(1));
    let selector = &specifier[selector_start..];
    if node_semver::Version::parse(selector).is_err() {
        return false;
    }
    let Some((_, revision)) = selector.rsplit_once("+r") else { return false };
    !revision.is_empty() && revision.bytes().all(|byte| byte.is_ascii_digit())
}

fn registry_revisions_conflict(
    existing: &LockfileResolution,
    incoming: &LockfileResolution,
) -> bool {
    let revision = |resolution: &LockfileResolution| match resolution {
        LockfileResolution::Registry(registry) => registry.revision.map(TarballRevision::get),
        LockfileResolution::Tarball(tarball) => tarball.revision.map(TarballRevision::get),
        _ => None,
    };
    let existing_revision = revision(existing);
    let incoming_revision = revision(incoming);
    if existing_revision.is_none() && incoming_revision.is_none() {
        return false;
    }
    existing_revision != incoming_revision
        || existing.checkable_integrity() != incoming.checkable_integrity()
}

/// The name the edge installs under: the key its parent manifest
/// declares, else the name the resolver resolved it under, else the
/// resolved package's own name.
// The manifest key has to come first: an importer entry is only
// recorded for an alias the manifest declares, so an edge keyed by a
// resolver alias that differs from its manifest key is dropped from the
// lockfile importer.
pub(super) fn node_alias(
    wanted: &WantedDependency,
    result: &pnpm_resolving_resolver_base::ResolveResult,
    id: &str,
) -> String {
    wanted
        .alias
        .clone()
        .filter(|alias| !alias.is_empty())
        .or_else(|| result.alias.clone())
        .or_else(|| result.name_ver.as_ref().map(|name_ver| name_ver.name.to_string()))
        .unwrap_or_else(|| id.to_string())
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

/// Settle children ownership across every occurrence one level seeded.
///
/// The ownership rank is `(update policy, depth, importer order,
/// parent path)` and a level shares the first three, so the
/// best-placed occurrence of a package is the one whose parent path
/// sorts first. Only that one claims: the losers cannot win against a
/// standing owner either, since they do not even outrank their own
/// level's winner.
fn assign_level_owners<'seed>(
    ctx: &TreeCtx,
    seeds: impl Iterator<Item = &'seed mut NodeSeed>,
) -> Result<(), ResolveDependencyTreeError> {
    // Linked nodes resolve their dependencies as their own importer,
    // so they never own children here.
    let mut level: Vec<&mut Box<PendingNode>> = seeds
        .filter_map(|seed| match seed {
            NodeSeed::Pending(pending) if !pending.is_link => Some(pending),
            _ => None,
        })
        .collect();
    let winners: Vec<usize> = {
        let mut best: HashMap<&str, usize> = HashMap::default();
        for (index, pending) in level.iter().enumerate() {
            let best_so_far = *best.entry(pending.id.as_str()).or_insert(index);
            let standing = &level[best_so_far];
            // Depth joins the comparison even though one level shares
            // it, so this cannot drift from [`ChildrenOwner::wins_over`]
            // if a frontier ever carries more than one depth.
            if (standing.depth, &standing.parent_ancestors)
                > (pending.depth, &pending.parent_ancestors)
            {
                best.insert(pending.id.as_str(), index);
            }
        }
        let mut winners: Vec<usize> = best.into_values().collect();
        winners.sort_unstable();
        winners
    };
    for index in winners {
        let pending = &mut *level[index];
        let peer_shadowed = std::mem::take(&mut pending.peer_shadowed);
        let claim = claim_children_owner(
            ctx,
            &pending.id,
            pending.depth,
            &pending.parent_ancestors,
            peer_shadowed,
        );
        install_owner_peer_dependencies(ctx, pending, &claim)?;
        pending.claim = Some(claim);
    }
    Ok(())
}

/// Split the package's envelope into children and peers the way its
/// children owner reads its manifest. Which of a package's
/// dependencies its own `peerDependencies` shadow follows from the
/// scope the occurrence resolved in, so a package first seeded by one
/// occurrence and owned by another has to be re-split.
fn install_owner_peer_dependencies(
    ctx: &TreeCtx,
    pending: &PendingNode,
    claim: &ChildrenOwnerClaim,
) -> Result<(), ResolveDependencyTreeError> {
    if pending.is_link || !claim.owns_children {
        return Ok(());
    }
    let peer_dependencies = extract_peer_dependencies(
        &pending.result,
        &claim.peer_shadowed,
        catalogs_for_children(ctx, pending.resolves_children_through_catalogs),
    )?;
    let mut packages = lock_recoverable(&ctx.workspace.packages);
    let Some(existing) = packages.get_mut(pending.id.as_str()) else { return Ok(()) };
    if existing.peer_dependencies == peer_dependencies {
        return Ok(());
    }
    existing.peer_dependencies = peer_dependencies.clone();
    drop(packages);
    register_peer_dep_names(ctx, &peer_dependencies);
    ctx.workspace.record_package_write(&pending.id);
    Ok(())
}

/// Give every occurrence of a settled level its tree node, and collect
/// the ones that still have to walk children of their own.
///
/// An occurrence walks only when it owns its package's children and
/// nothing has recorded them under its context; every other one reads
/// them from the owner's recording, under its own `parent_ids` cycle
/// break.
fn settle_seeds(
    ctx: &TreeCtx,
    seeds: Vec<NodeSeed>,
    children_overlay: Option<&Arc<PreferredVersionsOverlay>>,
    children_pkg_aliases: &Arc<ParentPkgAliases>,
) -> Vec<FrontierNode> {
    let mut frontier = Vec::new();
    for seed in seeds {
        let NodeSeed::Pending(mut pending) = seed else { continue };
        let claim = pending.claim.take();
        // Linked nodes don't walk their manifest's deps — see the
        // `is_link` comment block in [`fn@resolve_node_seed`]. They get
        // an empty `Realized` map: a linked node has no children of its
        // own here.
        if pending.is_link {
            insert_walked_node(
                ctx,
                &pending,
                crate::resolved_tree::TreeChildren::Realized(std::sync::Arc::new(BTreeMap::new())),
            );
            continue;
        }
        let Some(claim) = claim.filter(|claim| claim.owns_children) else {
            let children = lazy_children(&pending.parent_ancestors);
            insert_walked_node(ctx, &pending, children);
            continue;
        };
        if !pending.resolves_children_through_catalogs
            && recorded_children_match(ctx, &pending.id, &children_context(ctx, &pending, &claim))
        {
            let children = lazy_children(&pending.parent_ancestors);
            insert_walked_node(ctx, &pending, children);
            continue;
        }
        frontier.push(FrontierNode {
            pending,
            claim,
            children_overlay: children_overlay.cloned(),
            children_pkg_aliases: Arc::clone(children_pkg_aliases),
        });
    }
    frontier
}

/// Record what each occurrence of a seeded level resolved for its
/// children, and settle that level's own seeds into the next frontier.
fn settle_level(
    ctx: &TreeCtx,
    mut seeded: Vec<SeededNode>,
) -> Result<Vec<FrontierNode>, ResolveDependencyTreeError> {
    assign_level_owners(ctx, seeded.iter_mut().flat_map(|node| node.seeds.iter_mut()))?;
    let mut frontier = Vec::new();
    for node in seeded {
        let SeededNode {
            node: FrontierNode { pending, claim, .. },
            child_specs,
            seeds,
            grandchild_overlay,
            grandchild_pkg_aliases,
        } = node;
        let (children, others_stale) =
            record_walked_children(ctx, &pending, &claim, &child_specs, &seeds);
        insert_walked_node(ctx, &pending, children);
        if (others_stale || !claim.children_context_unchanged)
            && is_current_children_owner(ctx, &pending.id, &claim.owner)
        {
            make_non_owner_nodes_lazy(ctx, &pending.id, &pending.node_id);
        }
        frontier.extend(settle_seeds(
            ctx,
            seeds,
            grandchild_overlay.as_ref(),
            &grandchild_pkg_aliases,
        ));
    }
    super::finalized::announce_finalized_packages(ctx);
    Ok(frontier)
}

/// Publish the child edges one occurrence resolved, and report the
/// children to hang on its own tree node.
///
/// `children_by_id` records the resolved child pkg ids (not `NodeIds`)
/// plus the `optional` flag so lazy realisation can thread
/// `current_is_optional` correctly; the occurrence's own node keeps the
/// realized `(alias → NodeId)` map.
fn record_walked_children(
    ctx: &TreeCtx,
    pending: &PendingNode,
    claim: &ChildrenOwnerClaim,
    child_specs: &[ChildSpec],
    seeds: &[NodeSeed],
) -> (crate::resolved_tree::TreeChildren, bool) {
    if !is_current_children_owner(ctx, &pending.id, &claim.owner) {
        return (lazy_children(&pending.parent_ancestors), false);
    }
    let optional_by_alias: HashMap<&str, bool> =
        child_specs.iter().map(|(name, _, optional, _)| (name.as_str(), *optional)).collect();
    let mut realized: BTreeMap<String, NodeId> = BTreeMap::new();
    let mut by_id: Vec<crate::resolved_tree::ChildEdge> = Vec::new();
    for dep in seeds.iter().filter_map(seeded_dep) {
        let optional = optional_by_alias.get(dep.alias.as_str()).copied().unwrap_or(false);
        by_id.push(crate::resolved_tree::ChildEdge {
            alias: dep.alias.clone(),
            pkg_id: Arc::from(dep.id),
            optional,
        });
        realized.insert(dep.alias, dep.node_id);
    }
    record_children(ctx, &pending.id, &claim.owner, by_id, children_context(ctx, pending, claim))
        .into_children(realized, &pending.parent_ancestors)
}

/// The edge one seed contributes to its parent's children. `None` for
/// a seed the walk dropped — a cycle re-entry or a skipped optional.
fn seeded_dep(seed: &NodeSeed) -> Option<DirectDep> {
    match seed {
        NodeSeed::Done(dep) => dep.clone(),
        NodeSeed::Pending(pending) => Some(DirectDep {
            alias: pending.alias.clone(),
            node_id: pending.node_id.clone(),
            id: pending.id.clone(),
        }),
    }
}

/// What a walk resolved its children under, which a later occurrence
/// compares its own against before reading them.
fn children_context(
    ctx: &TreeCtx,
    pending: &PendingNode,
    claim: &ChildrenOwnerClaim,
) -> RecordedChildrenContext {
    RecordedChildrenContext {
        peer_shadowed: Arc::clone(&claim.peer_shadowed),
        prior_key: pending.prior_key.clone(),
        update_active: !matches!(ctx.update_reuse_scope(), super::UpdateReuseScope::All),
    }
}

/// Record an occurrence in the shared tree.
///
/// Repeat-visit leaves collapse onto one tree node; keep the
/// shallowest depth seen so downstream consumers that read
/// `tree_node.depth` (the peer pass folds it onto the graph node's
/// `depth`) take the minimum across visits. Per-occurrence counter ids
/// are unique by construction, so that only ever fires for leaves.
/// Linked nodes carry `depth = -1` so the peer-resolution pass
/// short-circuits them.
fn insert_walked_node(
    ctx: &TreeCtx,
    pending: &PendingNode,
    children: crate::resolved_tree::TreeChildren,
) {
    let depth = if pending.is_link { -1 } else { pending.depth };
    remember_node_parent_ids(ctx, &pending.node_id, Arc::clone(&pending.parent_ancestors));
    insert_tree_node(ctx, pending.node_id.clone(), &pending.id, children, depth);
}

/// Seed every child edge of one occurrence — its manifest's
/// dependencies less the names its own `peerDependencies` shadow, with
/// a workspace project's catalog specifiers resolved — and derive the
/// scope its grandchildren will resolve in.
///
/// The whole level seeds before any of it settles, so this resolves
/// packages and takes no ownership: [`fn@assign_level_owners`] does
/// that once every occurrence of the level is in.
#[async_recursion]
async fn seed_node_children<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    node: FrontierNode,
) -> Result<SeededNode, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let FrontierNode { pending, claim, children_overlay, children_pkg_aliases } = node;
    // Look up cached children specs first; only read the manifest on a
    // miss. The cache value is held by `Arc` so revisits clone the
    // refcount instead of the inner `Vec<ChildSpec>`, and
    // it is cached unfiltered because which of the specs the package's
    // own `peerDependencies` shadow is a property of the owner
    // occurrence, not of the manifest.
    let cached =
        lock_recoverable(&ctx.workspace.children_specs_by_id).get(pending.id.as_str()).cloned();
    let child_specs = if let Some(specs) = cached {
        specs
    } else {
        let specs = Arc::new(extract_children(&pending.result)?);
        lock_recoverable(&ctx.workspace.children_specs_by_id)
            .entry(Arc::from(pending.id.as_str()))
            .or_insert_with(|| Arc::clone(&specs));
        specs
    };
    let peer_shadowed = &claim.peer_shadowed;
    let child_specs = if peer_shadowed.is_empty() {
        child_specs
    } else {
        child_specs
            .iter()
            .filter(|(name, _, optional, _)| *optional || !peer_shadowed.contains(name))
            .cloned()
            .collect::<Vec<ChildSpec>>()
            .pipe(Arc::new)
    };
    let child_specs = if let Some(catalogs) =
        catalogs_for_children(ctx, pending.resolves_children_through_catalogs)
    {
        child_specs
            .iter()
            .cloned()
            .collect::<Vec<ChildSpec>>()
            .pipe(|specs| resolve_catalog_child_specs(specs, catalogs))?
            .pipe(Arc::new)
    } else {
        child_specs
    };
    // An *updated* parent (one that landed on a different version than
    // the lockfile recorded, or a new dep) discards its
    // `resolvedDependencies` child refs, forcing its subtree to
    // re-resolve. A parent that freshly resolved but landed back on its
    // previously recorded version keeps the prior child refs alive
    // (pnpm's non-`parentPkg.updated` arm), and each child edge
    // re-enters the reuse gate with its recorded key — so a
    // still-satisfied subtree is reused rather than re-resolved.
    // Re-resolving those children would re-pick open ranges (`*`) at
    // their newest versions and churn the lockfile.
    let prior_children_snapshot = pending
        .prior_key
        .as_ref()
        .filter(|key| landed_on_prior_entry(key, &pending.id))
        .and_then(|key| ctx.workspace.wanted_lockfile.as_ref()?.snapshots.as_ref()?.get(key));
    // Snapshot this importer's direct-dep versions once for the whole
    // child fanout instead of locking per edge.
    let direct_versions =
        lock_recoverable(&ctx.workspace.direct_dep_versions).get(&ctx.importer_id).map(Arc::clone);
    let declaring_dir = declaring_manifest_dir(ctx, &pending.result);
    let parent_is_workspace = pending.result.resolved_via == "workspace";
    let child_depth = pending.depth + 1;
    let child_optional_parent = pending.current_is_optional;
    let next_ancestors = Arc::clone(&pending.next_ancestors);
    let seeds = child_specs
        .iter()
        .map(|(child_name, child_range, child_optional, child_injected)| {
            let mut child_wanted = WantedDependency {
                alias: Some(child_name.clone()),
                bare_specifier: Some(child_range.clone()),
                optional: Some(*child_optional),
                injected: child_injected.then_some(true),
                ..WantedDependency::default()
            };
            let mut child_prior = prior_children_snapshot
                .and_then(|snapshot| prior_child_key(snapshot, child_name, child_range));
            // Stale-pin refresh: force the edge onto a higher in-range
            // direct-dep version instead of reusing the pin, so the
            // pinned version is never resolved or fetched.
            let forced_version = child_prior
                .as_ref()
                .and_then(|key| key.suffix.version_semver().cloned())
                .zip(child_range.parse::<node_semver::Range>().ok())
                .and_then(|(pinned, range)| {
                    higher_direct_dep_version(
                        direct_versions.as_deref(),
                        child_name,
                        &pinned,
                        &range,
                    )
                });
            if let Some(higher) = forced_version {
                child_wanted.bare_specifier = Some(higher.to_string());
                child_prior = None;
            }
            let next_ancestors = Arc::clone(&next_ancestors);
            let pick_overlay = children_overlay.clone();
            let declaring_dir = declaring_dir.clone();
            let parent_pkg_aliases = &children_pkg_aliases;
            async move {
                let seed = resolve_node_seed(
                    ctx,
                    resolver,
                    child_wanted,
                    &next_ancestors,
                    child_depth,
                    child_optional_parent,
                    ReuseSource::Transitive { key: child_prior },
                    pick_overlay,
                    declaring_dir.as_deref(),
                    parent_pkg_aliases,
                    parent_is_workspace,
                )
                .await?;
                warm_children_resolutions(ctx, resolver, &seed).await;
                Ok::<NodeSeed, ResolveDependencyTreeError>(seed)
            }
        })
        .pipe(future::try_join_all)
        .await?;
    let grandchild_overlay =
        PreferredVersionsOverlay::layer(children_overlay.clone(), level_versions(ctx, &seeds));
    let grandchild_pkg_aliases = children_pkg_aliases.extend(level_aliases(&seeds));
    Ok(SeededNode {
        node: FrontierNode { pending, claim, children_overlay, children_pkg_aliases },
        child_specs,
        seeds,
        grandchild_overlay,
        grandchild_pkg_aliases,
    })
}

/// Whether the `parent → child` edge closes a dependency cycle's
/// *second* lap. The first re-entry of a cycle is kept (so the
/// cycle-closing dependency edge appears in the tree and the lockfile
/// snapshot); only the repeat of the full `parent … child` sequence is
/// dropped.
pub(crate) fn parent_ids_contain_sequence(
    pkg_ids: &[String],
    pkg_id1: &str,
    pkg_id2: &str,
) -> bool {
    let Some(pkg1_index) = pkg_ids.iter().position(|id| id == pkg_id1) else {
        return false;
    };
    if pkg1_index == pkg_ids.len() - 1 {
        return false;
    }
    let Some(pkg2_index) = pkg_ids.iter().rposition(|id| id == pkg_id2) else {
        return false;
    };
    pkg1_index < pkg2_index && pkg2_index != pkg_ids.len() - 1
}

/// Whether a freshly resolved node landed back on its previously
/// recorded lockfile entry — the non-`updated` arm, which keeps the
/// prior child refs alive.
fn landed_on_prior_entry(prior_key: &PkgNameVerPeer, resolved_pkg_id: &str) -> bool {
    prior_key.without_peer().to_string() == pnpm_deps_path::remove_suffix(resolved_pkg_id)
}

/// The package names the npm picker may consult the preferred-versions
/// overlay under for one wanted edge: the alias itself, plus the real
/// package name from [`real_package_name_of`] when it differs (the
/// inner target of an `npm:` alias or the folded `@jsr/...` name of a
/// `jsr:` specifier) — mirroring the name derivation in the npm
/// resolver's `parse_bare_specifier`, which keys its overlay merge by
/// the resolved `spec.name` rather than the outer alias.
///
/// Borrowed and slot-shaped rather than a `Vec<String>`: every edge of
/// every walked package asks for these, and the overwhelming majority
/// resolve to one borrowed alias.
fn overlay_lookup_names<'edge>(
    alias: Option<&'edge str>,
    bare_specifier: Option<&'edge str>,
) -> [Option<Cow<'edge, str>>; 2] {
    let alias = alias.filter(|alias| !alias.is_empty());
    let real_name = real_package_name_of(alias, bare_specifier)
        .filter(|real_name| alias.is_none_or(|alias| alias != real_name.as_ref()));
    [alias.map(Cow::Borrowed), real_name]
}

/// Convert a workspace directory resolution into the representation shared by
/// every importer. A `link:` target is stored relative to the lockfile root;
/// an injected `file:` target already has that representation.
fn canonical_workspace_resolution(
    result: &pnpm_resolving_resolver_base::ResolveResult,
    project_dir: &Path,
    lockfile_dir: &Path,
) -> Option<pnpm_resolving_resolver_base::ResolveResult> {
    if result.resolved_via != "workspace" {
        return None;
    }
    let pnpm_lockfile::LockfileResolution::Directory(directory_resolution) = &result.resolution
    else {
        return None;
    };
    if result.id.as_str().strip_prefix("file:") == Some(&directory_resolution.directory) {
        return Some(result.clone());
    }
    if result.id.as_str().strip_prefix("link:") != Some(&directory_resolution.directory) {
        return None;
    }

    let target = Path::new(&directory_resolution.directory);
    let absolute_target = if target.is_absolute() {
        pnpm_fs::lexical_normalize(target)
    } else {
        pnpm_fs::lexical_normalize(&project_dir.join(target))
    };
    let canonical_target = pathdiff::diff_paths(&absolute_target, lockfile_dir)
        .unwrap_or(absolute_target)
        .display()
        .to_string()
        .replace('\\', "/");
    let mut canonical = result.clone();
    canonical.id =
        pnpm_resolving_resolver_base::PkgResolutionId::from(format!("link:{canonical_target}"));
    let pnpm_lockfile::LockfileResolution::Directory(canonical_directory) =
        &mut canonical.resolution
    else {
        unreachable!("the cloned workspace resolution remains a directory")
    };
    canonical_directory.directory = canonical_target;
    Some(canonical)
}

/// Render a canonical workspace resolution for one consuming importer before
/// its manifest hooks run.
fn render_workspace_resolution(
    canonical: &pnpm_resolving_resolver_base::ResolveResult,
    anchor: &crate::link_target::ImporterAnchor,
    project_dir: &Path,
    lockfile_dir: &Path,
) -> pnpm_resolving_resolver_base::ResolveResult {
    let mut rendered = canonical.clone();
    let pnpm_lockfile::LockfileResolution::Directory(directory_resolution) =
        &mut rendered.resolution
    else {
        unreachable!("the shared workspace cache contains only directory resolutions")
    };
    if canonical.id.as_str().starts_with("file:") {
        return rendered;
    }

    let target = directory_resolution.directory.as_str();
    let consumer_target = anchor.target_relative_to_importer(target).unwrap_or_else(|| {
        let target = Path::new(target);
        let absolute_target = if target.is_absolute() {
            pnpm_fs::lexical_normalize(target)
        } else {
            pnpm_fs::lexical_normalize(&lockfile_dir.join(target))
        };
        let project_dir = pnpm_fs::lexical_normalize(project_dir);
        pathdiff::diff_paths(&absolute_target, project_dir)
            .unwrap_or(absolute_target)
            .display()
            .to_string()
            .replace('\\', "/")
    });
    rendered.id =
        pnpm_resolving_resolver_base::PkgResolutionId::from(format!("link:{consumer_target}"));
    directory_resolution.directory = consumer_target;
    rendered
}

/// Remove the consumer directory from a named `workspace:` request while
/// retaining every other input the explicit workspace resolver reads.
fn shared_workspace_key(
    ctx: &TreeCtx,
    cache_key: &WantedKey,
    wanted: &WantedDependency,
    opts: &ResolveOptions,
) -> Option<SharedWorkspaceWantedKey> {
    let bare_specifier = wanted.bare_specifier.as_deref()?;
    if !bare_specifier.starts_with("workspace:") || bare_specifier.starts_with("workspace:.") {
        return None;
    }
    #[cfg(debug_assertions)]
    {
        let importer_key_covers_edge = ctx.workspace_resolution_options_key.matches_options(opts);
        debug_assert!(
            importer_key_covers_edge,
            "the importer-wide workspace-resolution key must describe every edge's options",
        );
    }
    // The consumer scope is exactly what the shared key's hash and
    // equality drop. Every `workspace:` selector carries one, so its
    // absence means this edge is not the shape assumed here.
    cache_key.fields().6.as_ref()?;
    Some(SharedWorkspaceWantedKey::new(
        cache_key.clone(),
        wanted.prev_specifier.clone(),
        &ctx.workspace_resolution_options_key,
    ))
}

/// Look the wanted edge up in the per-wanted dedup cache or run the resolver
/// chain and the manifest-hook pipeline, caching the `Arc<ResolveResult>`
/// under `cache_key`. Eligible named workspace selectors add two more layers:
/// a canonical resolver result, shared by every importer, and a hook-processed
/// result keyed by the `link:` this importer renders, shared only with the
/// importers that render the same one. A second importer therefore always
/// skips the resolver chain, and skips the hooks as well when its rendered
/// link matches. Concurrent first-callers can both miss and resolve in parallel —
/// the resolver's own per-cache-key fetch locker coalesces the network work,
/// and the second `or_insert` loses the race harmlessly.
async fn resolve_wanted_cached<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: &WantedDependency,
    opts: &ResolveOptions,
    pick_overlay: Option<&Arc<PreferredVersionsOverlay>>,
    cache_key: WantedKey,
) -> Result<Arc<pnpm_resolving_resolver_base::ResolveResult>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let cached =
        lock_recoverable(&ctx.workspace.resolved_by_wanted).get(&cache_key).map(Arc::clone);
    if let Some(result) = cached {
        return Ok(result);
    }
    // Combine two per-package opts adjustments into one clone. The
    // `update_requested` flag is scoped per wanted-dependency — true only
    // when the package's real name (parsed from `bare_specifier` for
    // npm-aliases, folded from the jsr specifier for jsr deps) is in the
    // update target list — so the picker's held-back-update warning fires
    // only for the packages the user actually asked to update.
    let needs_overlay = !cache_key.fields().8.is_empty();
    let update_target = cache_key.fields().10;
    let needs_update = update_target != opts.update_requested;
    let owned_opts;
    let opts = if needs_overlay || needs_update {
        let mut owned = opts.clone();
        if needs_overlay {
            owned.preferred_versions_overlay = pick_overlay.map(Arc::clone);
        }
        if needs_update {
            owned.update_requested = update_target;
        }
        owned_opts = owned;
        &owned_opts
    } else {
        opts
    };
    let shared_workspace_key = ctx
        .workspace
        .share_workspace_resolutions
        .then(|| shared_workspace_key(ctx, &cache_key, wanted, opts))
        .flatten();
    let cached_workspace = shared_workspace_key.as_ref().and_then(|key| {
        lock_recoverable(&ctx.workspace.resolved_workspace_by_wanted).get(key).map(Arc::clone)
    });
    let mut canonical_workspace = cached_workspace.clone();
    let mut result = if let Some(canonical) = cached_workspace {
        // A `workspace:` edge never runs under the per-edge project-dir
        // override (that fires for `file:` specifiers only), so the
        // importer-wide anchor is exactly this edge's anchor.
        #[cfg(debug_assertions)]
        {
            let anchor_inputs_describe_this_edge = opts.project_dir == ctx.base_opts.project_dir
                && opts.lockfile_dir == ctx.base_opts.lockfile_dir;
            debug_assert!(
                anchor_inputs_describe_this_edge,
                "the importer-wide link anchor must describe every workspace edge",
            );
        }
        render_workspace_resolution(
            &canonical,
            &ctx.base_link_anchor,
            &opts.project_dir,
            &opts.lockfile_dir,
        )
    } else {
        let result = resolver.resolve(wanted, opts).await.map_err(map_resolve_error)?;
        let Some(result) = result else {
            return Err(ResolveDependencyTreeError::SpecNotSupported {
                specifier: render_specifier(wanted),
            });
        };
        if let Some(shared_workspace_key) = shared_workspace_key.as_ref()
            && let Some(canonical) =
                canonical_workspace_resolution(&result, &opts.project_dir, &opts.lockfile_dir)
        {
            let canonical = Arc::new(canonical);
            canonical_workspace = Some(Arc::clone(
                lock_recoverable(&ctx.workspace.resolved_workspace_by_wanted)
                    .entry(shared_workspace_key.clone())
                    .or_insert(canonical),
            ));
        }
        result
    };
    let workspace_final_key = match (shared_workspace_key, canonical_workspace.as_deref()) {
        (Some(shared_wanted), Some(canonical)) => {
            Some(WorkspaceFinalWantedKey::new(shared_wanted, &canonical.id, &result.id))
        }
        _ => None,
    };
    if let Some(key) = workspace_final_key.as_ref()
        && let Some(cached) = lock_recoverable(&ctx.workspace.resolved_workspace_final_by_wanted)
            .get(key)
            .map(Arc::clone)
    {
        // Both return paths record the project-scoped entry, so the lookup at
        // the top of this function stays authoritative: a repeat of this edge
        // costs one lookup rather than a shared-key rebuild and a re-render.
        lock_recoverable(&ctx.workspace.resolved_by_wanted)
            .entry(cache_key)
            .or_insert_with(|| Arc::clone(&cached));
        return Ok(cached);
    }
    if result.manifest.is_none() {
        result.manifest = Some(Arc::new(fallback_manifest(wanted, opts.current_pkg.as_ref())));
    }
    // Apply the configured `readPackageHook` (today:
    // `packageExtensions`) to the manifest fragment before
    // anything downstream sees it. The hook clones the inner `Value`
    // only when it modifies it, so unrelated manifests keep sharing the
    // resolver's cached `Arc`.
    if let Some(hook) = ctx.workspace.manifest_hook.as_ref()
        && let Some(manifest) = result.manifest.take()
    {
        result.manifest = Some(hook(manifest));
    }

    if let Some(pnpmfile_hook) = ctx.workspace.pnpmfile_hook.as_ref()
        && let Some(manifest) = result.manifest.take()
    {
        let log = ctx.workspace.read_package_log.clone().unwrap_or_else(|| Arc::new(|_| {}));
        // Directory resolutions carry their directory so the hook can tell a
        // workspace project's dependency instance apart from a registry
        // manifest — see `HookContext::dir`.
        let dir = match &result.resolution {
            pnpm_lockfile::LockfileResolution::Directory(directory_resolution) => {
                Some(directory_resolution.directory.clone())
            }
            _ => None,
        };
        let hook_ctx = pnpm_hooks::HookContext { log, dir };

        let updated = pnpmfile_hook
            .read_package((*manifest).clone(), hook_ctx)
            .await
            .map_err(ResolveDependencyTreeError::PnpmfileHook)?;
        result.manifest = Some(updated);
    }

    // Overrides run last so a pnpmfile hook that replaced the manifest
    // cannot erase them — see `WorkspaceTreeCtx::overrides_hook`.
    if let Some(hook) = ctx.workspace.overrides_hook.as_ref()
        && let Some(manifest) = result.manifest.take()
    {
        result.manifest = Some(hook(manifest));
    }

    // Wrap in `Arc` once so the cache, the per-id
    // `ResolvedPackage` envelope, and the later peer-resolved
    // graph node share one heap-allocated `ResolveResult`
    // instead of cloning every `String` field per occurrence.
    let result = Arc::new(result);
    if let Some(key) = workspace_final_key {
        lock_recoverable(&ctx.workspace.resolved_workspace_final_by_wanted)
            .entry(key)
            .or_insert_with(|| Arc::clone(&result));
    }
    lock_recoverable(&ctx.workspace.resolved_by_wanted)
        .entry(cache_key)
        .or_insert_with(|| Arc::clone(&result));
    Ok(result)
}

/// Stand in for the manifest a resolver didn't supply (pnpm's
/// `getManifestFromResponse`).
///
/// Every consumer downstream of the resolver chain reads the package's
/// identity off its manifest — most of all
/// [`build_pkg_id_with_patch_hash`], which has no name to prefix the dep
/// path with when there is none, leaving a bare `file:<path>` / URL that
/// keys no `packages:` row. A package with no `package.json` of its own
/// still has to install, so it borrows an identity; `0.0.0` is the
/// version pnpm writes into `packages:` for such a package.
fn fallback_manifest(
    wanted: &WantedDependency,
    current_pkg: Option<&CurrentPkg>,
) -> pnpm_resolving_resolver_base::DependencyManifest {
    if let Some(current) = current_pkg
        && let Some(name) = current.name.as_deref().filter(|name| !name.is_empty())
        && let Some(version) = current.version.as_deref().filter(|version| !version.is_empty())
    {
        return serde_json::json!({ "name": name, "version": version });
    }
    let name = match wanted.alias.as_deref().filter(|alias| !alias.is_empty()) {
        Some(alias) => alias,
        // A specifier's last path segment is the closest thing to a name
        // an unaliased dep carries: `file:./no-manifest-1.0.0.tgz` and
        // `https://host/no-manifest-1.0.0.tgz` both name the archive.
        None => wanted
            .bare_specifier
            .as_deref()
            .unwrap_or_default()
            .rsplit('/')
            .next()
            .unwrap_or_default(),
    };
    serde_json::json!({ "name": name, "version": "0.0.0" })
}

/// Wrap a resolver-chain failure, keeping the pnpm error code of the ones
/// that carry one. The chain hands back a type-erased
/// [`ResolveError`], which drops the `miette::Diagnostic` facet, so the codes
/// that are part of pnpm's public contract are recovered by downcast; every
/// other failure keeps the generic envelope.
fn map_resolve_error(err: ResolveError) -> ResolveDependencyTreeError {
    let err = match err.downcast::<NoMatchingVersionError>() {
        Ok(no_matching_version) => {
            return ResolveDependencyTreeError::NoMatchingVersion(*no_matching_version);
        }
        Err(err) => err,
    };
    let err = match err.downcast::<RegistryResponseError>() {
        Ok(response) => return ResolveDependencyTreeError::RegistryResponse(*response),
        Err(err) => err,
    };
    match err.downcast::<GitResolveError>() {
        Ok(git) => ResolveDependencyTreeError::GitResolve(*git),
        Err(err) => ResolveDependencyTreeError::Resolve(err.to_string()),
    }
}

/// Speculatively warm a freshly-seeded node's whole subtree so its
/// packuments download while the level barriers wait for their
/// slowest members. Results are discarded — the real picks run in the
/// walk phase with the level's preferred-versions overlay and hit the
/// warm metadata caches — and errors are swallowed: a speculative
/// fetch must never fail the install (the real resolve will surface
/// it). Recovers the cross-level pipelining the postponed-resolution
/// barrier otherwise serializes; pure overlap, no behavioral effect.
pub(super) async fn warm_children_resolutions<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    seed: &NodeSeed,
) where
    Chain: Resolver + ?Sized,
{
    // A configured pnpmfile hook is externally observable per call
    // (`readPackage` IPC, `context.log`, custom resolvers), so
    // speculative resolutions must not fire it; the pure in-memory
    // manifest hook (packageExtensions / overrides) is idempotent and
    // cache-deduped, indistinguishable from a first-caller win in the
    // pre-existing concurrent-miss race.
    if ctx.workspace.pnpmfile_hook.is_some() {
        return;
    }
    let NodeSeed::Pending(pending) = seed else { return };
    if pending.is_link || !claim_children_warmup(ctx, &pending.id) {
        return;
    }
    warm_result_children(
        ctx,
        resolver,
        &pending.result,
        &pending.peer_shadowed,
        pending.resolves_children_through_catalogs,
        pending.depth,
    )
    .await;
}

/// Warm the resolutions of `result`'s whole subtree. Speculative only:
/// nothing is recorded in the tree, every package is visited at most
/// once across the walk, and a child whose own peers shadow it is
/// skipped the way the real seed path skips it.
#[async_recursion]
async fn warm_result_children<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    result: &pnpm_resolving_resolver_base::ResolveResult,
    peer_shadowed: &HashSet<String>,
    through_catalogs: bool,
    depth: i32,
) where
    Chain: Resolver + ?Sized,
{
    let Ok(specs) = extract_children(result) else { return };
    let specs = if peer_shadowed.is_empty() {
        specs
    } else {
        specs
            .into_iter()
            .filter(|(name, _, optional, _)| *optional || !peer_shadowed.contains(name))
            .collect()
    };
    let specs = if let Some(catalogs) = catalogs_for_children(ctx, through_catalogs) {
        let Ok(specs) = resolve_catalog_child_specs(specs, catalogs) else { return };
        specs
    } else {
        specs
    };
    let opts = ctx.opts_for_depth(depth + 1);
    let declaring_dir = declaring_manifest_dir(ctx, result);
    specs
        .iter()
        .map(|(name, range, optional, injected)| {
            let wanted = WantedDependency {
                alias: Some(name.clone()),
                bare_specifier: Some(range.clone()),
                optional: Some(*optional),
                injected: injected.then_some(true),
                ..WantedDependency::default()
            };
            let opts = opts_relative_to_declaring_manifest(opts, &wanted, declaring_dir.as_deref());
            async move {
                let opts = opts.as_ref();
                // Warm through the same per-wanted dedup cache, under
                // the empty-overlay-view key: when the real pick's
                // view is empty too (the overwhelmingly common case)
                // it reuses this entry outright; otherwise it misses
                // into its own bucket and re-picks from the warm
                // metadata caches.
                let project_scope = project_relative_cache_scope(&wanted, opts);
                let cache_key = WantedKey::new((
                    wanted.alias.clone(),
                    wanted.bare_specifier.clone(),
                    wanted.optional,
                    wanted.injected,
                    opts.pick_lowest_version,
                    opts.published_by,
                    project_scope,
                    // No prior-lockfile key: a warm entry must only be
                    // reused by edges that carry no currentPkg either.
                    None,
                    Vec::new(),
                    ctx.update_cache_scope(),
                    is_update_target(ctx.update_scope(), &wanted, None, depth + 1),
                ));
                let Ok(child) =
                    resolve_wanted_cached(ctx, resolver, &wanted, opts, None, cache_key).await
                else {
                    return;
                };
                // Claimed by the resolver's raw id: the patch-qualified
                // id is only built on the real walk, whose bookkeeping
                // decides which patches count as applied.
                let child_id = child.id.as_str();
                if child_id.starts_with("link:") || !claim_children_warmup(ctx, child_id) {
                    return;
                }
                // The parent alias scope is not tracked speculatively;
                // with `autoInstallPeers` off nothing is shadowed and
                // the real walk drops the edge instead.
                let child_peer_shadowed = peer_shadowed_dependencies(
                    child.manifest.as_deref(),
                    &ParentPkgAliases::root(HashSet::default()),
                    ctx.workspace.auto_install_peers,
                );
                warm_result_children(
                    ctx,
                    resolver,
                    &child,
                    &child_peer_shadowed,
                    resolves_children_through_catalogs(&child),
                    depth + 1,
                )
                .await;
            }
        })
        .pipe(future::join_all)
        .await;
}

/// Whether this package's child specifiers pass through the importer's
/// catalogs, which makes them a property of the resolving importer
/// rather than of the package id — the one input
/// [`fn@recorded_children_match`] cannot compare, since the recorded
/// context does not carry the catalogs the recording importer used.
fn resolves_children_through_catalogs(
    result: &pnpm_resolving_resolver_base::ResolveResult,
) -> bool {
    result.resolved_via == "workspace" && result.id.as_str().starts_with("file:")
}

fn catalogs_for_children(
    ctx: &TreeCtx,
    resolves_children_through_catalogs: bool,
) -> Option<&Catalogs> {
    (resolves_children_through_catalogs && !ctx.catalogs.is_empty()).then_some(&ctx.catalogs)
}

fn resolve_catalog_child_specs(
    child_specs: Vec<ChildSpec>,
    catalogs: &Catalogs,
) -> Result<Vec<ChildSpec>, ResolveDependencyTreeError> {
    child_specs
        .into_iter()
        .map(|(name, range, optional, injected)| {
            resolve_catalog_specifier(name, range, catalogs)
                .map(|(name, range)| (name, range, optional, injected))
        })
        .collect()
}

/// The install aliases one resolved level contributes to its
/// children's [`ParentPkgAliases`] scope. An edge the walk dropped (a
/// cycle re-entry, a skipped optional) contributes nothing, matching
/// pnpm's fold over the level's resolved addresses.
pub(super) fn level_aliases(seeds: &[NodeSeed]) -> HashSet<String> {
    seeds
        .iter()
        .filter_map(|seed| match seed {
            NodeSeed::Pending(pending) => Some(pending.alias.clone()),
            NodeSeed::Done(Some(dep)) => Some(dep.alias.clone()),
            NodeSeed::Done(None) => None,
        })
        .collect()
}

/// The `(name → versions)` additions one resolved level contributes
/// to its children's preferred-versions overlay. Linked nodes carry no
/// `name_ver` and contribute nothing — they're skipped in the fold.
pub(super) fn level_versions(ctx: &TreeCtx, seeds: &[NodeSeed]) -> BTreeMap<String, Vec<String>> {
    let packages = lock_recoverable(&ctx.workspace.packages);
    let mut level: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for seed in seeds {
        let name_ver = match seed {
            NodeSeed::Pending(pending) => pending.result.name_ver.as_ref(),
            NodeSeed::Done(Some(dep)) => {
                packages.get(dep.id.as_str()).and_then(|pkg| pkg.result.name_ver.as_ref())
            }
            NodeSeed::Done(None) => None,
        };
        let Some(name_ver) = name_ver else { continue };
        let versions = level.entry(name_ver.name.to_string()).or_default();
        let version = name_ver.suffix.to_string();
        if !versions.contains(&version) {
            versions.push(version);
        }
    }
    level
}

/// Map an ancestor-id chain to the `parents` payload of a
/// skipped-optional-dependency notification, resolving each
/// `pkgIdWithPatchHash` through the shared packages map (the
/// counterpart of pnpm's `getPkgsInfoFromIds`).
fn pkgs_info_from_ids(
    ctx: &TreeCtx,
    ancestor_ids: &[String],
) -> Vec<SkippedOptionalDependencyParent> {
    let packages = lock_recoverable(&ctx.workspace.packages);
    ancestor_ids
        .iter()
        .map(|id| {
            let name_ver = packages.get(id.as_str()).and_then(|pkg| pkg.result.name_ver.as_ref());
            SkippedOptionalDependencyParent {
                id: id.clone(),
                name: name_ver.map(|name_ver| name_ver.name.to_string()).unwrap_or_default(),
                version: name_ver.map(|name_ver| name_ver.suffix.to_string()).unwrap_or_default(),
            }
        })
        .collect()
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
