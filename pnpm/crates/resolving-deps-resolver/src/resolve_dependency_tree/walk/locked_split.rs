use super::{
    Arc, HashSet, NodeSeed, ParentPkgAliases, PendingNode, Resolver, TreeCtx, WantedDependency,
    WantedKey, dependencies_named_as_peers, is_update_target, lock_recoverable,
    peer_shadowed_dependencies, pinned_children_scope, project_relative_cache_scope,
    resolve_wanted_cached,
};

/// Marks each children owner in `seeds` that would read children the
/// prior lockfile pinned, when resolving them under
/// `children_pkg_aliases` would split some package's dependencies and
/// peers differently than the lockfile recorded. A marked owner resolves
/// its children instead (see `RecordedChildrenContext::pins_children_over`).
///
/// Which of a package's dependencies its own peers shadow depends on the
/// scope it resolves in, and the lockfile does not record the overlap: an
/// entry that kept a dependency omits the same-named peer. So the
/// package's metadata is read, but only for a package with a locked
/// dependency or peer whose presence in scope differs from the scope the
/// pins were recorded under.
pub(super) async fn mark_changed_locked_splits<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    seeds: &mut [NodeSeed],
    children_pkg_aliases: &Arc<ParentPkgAliases>,
) where
    Chain: Resolver + ?Sized,
{
    for seed in seeds {
        let NodeSeed::Pending(pending) = seed else { continue };
        let Some(pinned_scope) = pinned_scope_read_by(ctx, pending) else { continue };
        let scopes = Scopes { current: Arc::clone(children_pkg_aliases), pinned: pinned_scope };
        let changes = locked_split_changes(ctx, resolver, &pending.identity.id, scopes).await;
        if let Some(claim) = pending.claim.as_mut() {
            claim.changes_locked_split = changes;
        }
    }
}

/// The scope a package's pinned children were recorded under, when
/// `pending` owns them and would otherwise read the pins.
fn pinned_scope_read_by(ctx: &TreeCtx, pending: &PendingNode) -> Option<Arc<ParentPkgAliases>> {
    let reads_pins = pending.claim.as_ref().is_some_and(|claim| claim.owns_children)
        && (pending.prior_key.is_none() || ctx.update_scope().unpins_every_edge())
        && matches!(ctx.update_reuse_scope(), super::super::UpdateReuseScope::All);
    if !reads_pins {
        return None;
    }
    pinned_children_scope(ctx, &pending.identity.id)
}

/// The scope one level of the pinned subtree resolves in now, and the one
/// it was recorded under. Every level below adds the same aliases to both.
#[derive(Clone)]
struct Scopes {
    current: Arc<ParentPkgAliases>,
    pinned: Arc<ParentPkgAliases>,
}

impl Scopes {
    fn extend(&self, aliases: &HashSet<String>) -> Self {
        Scopes {
            current: self.current.extend(aliases.clone()),
            pinned: self.pinned.extend(aliases.clone()),
        }
    }

    fn differ_on(&self, name: &str) -> bool {
        self.current.contains(name) != self.pinned.contains(name)
    }
}

/// Whether any package in the recorded subtree of `pkg_id` would split
/// its dependencies and peers differently when `pkg_id`'s children
/// resolve under `scopes.current`.
async fn locked_split_changes<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    pkg_id: &Arc<str>,
    scopes: Scopes,
) -> bool
where
    Chain: Resolver + ?Sized,
{
    let mut visited: HashSet<Arc<str>> = HashSet::default();
    let mut parents = vec![(Arc::clone(pkg_id), scopes)];
    while let Some((parent_id, scopes)) = parents.pop() {
        let Some(edges) = recorded_edges(ctx, &parent_id) else { continue };
        let grandchildren_scopes = scopes.extend(
            &edges
                .iter()
                .map(|edge| edge.alias.clone())
                .collect(),
        );
        for edge in edges.iter() {
            if !visited.insert(Arc::clone(&edge.pkg_id)) {
                continue;
            }
            if split_changes(ctx, resolver, &edge.pkg_id, &scopes).await {
                return true;
            }
            parents.push((Arc::clone(&edge.pkg_id), grandchildren_scopes.clone()));
        }
    }
    false
}

fn recorded_edges(
    ctx: &TreeCtx,
    pkg_id: &str,
) -> Option<Arc<Vec<crate::resolved_tree::ChildEdge>>> {
    lock_recoverable(&ctx.workspace.children.by_id)
        .get(pkg_id)
        .map(|recorded| Arc::clone(&recorded.edges))
}

/// Whether the package `pkg_id`, resolved under `scopes.current`, would
/// split its dependencies and peers differently than its recorded
/// children and peers do.
async fn split_changes<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    pkg_id: &str,
    scopes: &Scopes,
) -> bool
where
    Chain: Resolver + ?Sized,
{
    let Some(locked) = LockedSplit::of(ctx, pkg_id) else { return false };
    if !locked.may_change(scopes) {
        return false;
    }
    let Some(manifest) = registry_manifest(ctx, resolver, &locked).await else {
        return false;
    };
    let shadowed = peer_shadowed_dependencies(
        Some(&manifest),
        &scopes.current,
        ctx.workspace.policy.auto_install_peers,
    );
    dependencies_named_as_peers(Some(&manifest))
        .any(|(dep, _)| locked.deps.contains(dep) == shadowed.contains(dep))
}

/// A registry package's dependencies and peers as the prior lockfile
/// split them.
struct LockedSplit {
    name: String,
    version: String,
    deps: HashSet<String>,
    /// The peers whose shadowing depends on the scope: optional ones, and
    /// every one when peers are not auto-installed.
    scope_dependent_peers: Vec<String>,
}

impl LockedSplit {
    fn of(ctx: &TreeCtx, pkg_id: &str) -> Option<Self> {
        let auto_install_peers = ctx.workspace.policy.auto_install_peers;
        let packages = lock_recoverable(&ctx.workspace.tree.packages);
        let pkg = packages.get(pkg_id)?;
        let result = pkg.result();
        if result.resolved_via != "npm-registry" {
            return None;
        }
        let name_ver = result.package.name_ver.as_ref()?;
        Some(LockedSplit {
            name: name_ver.name.to_string(),
            version: name_ver.suffix.to_string(),
            deps: recorded_edges(ctx, pkg_id)
                .map(|edges| {
                    edges
                        .iter()
                        .map(|edge| edge.alias.clone())
                        .collect()
                })
                .unwrap_or_default(),
            scope_dependent_peers: pkg.peer_dependencies
                .iter()
                .filter(|(_, peer)| peer.optional || !auto_install_peers)
                .map(|(peer_name, _)| peer_name.clone())
                .collect(),
        })
    }

    /// Whether a name the split depends on is in only one of the scopes.
    fn may_change(&self, scopes: &Scopes) -> bool {
        self.deps
            .iter()
            .any(|dep| scopes.differ_on(dep))
            || self.scope_dependent_peers
                .iter()
                .any(|peer| scopes.differ_on(peer))
    }
}

/// The manifest of the locked package as a fresh resolve reads it, read-package
/// hooks applied. `None` when it cannot be resolved; the pins then stand.
async fn registry_manifest<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    locked: &LockedSplit,
) -> Option<Arc<serde_json::Value>>
where
    Chain: Resolver + ?Sized,
{
    let wanted = WantedDependency {
        alias: Some(locked.name.clone()),
        bare_specifier: Some(locked.version.clone()),
        ..WantedDependency::default()
    };
    let depth = 1;
    let opts = ctx.opts_for_depth(depth);
    let cache_key = WantedKey::new((
        wanted.alias.clone(),
        wanted.bare_specifier.clone(),
        wanted.optional,
        wanted.injected,
        opts.version.pick_lowest_version,
        (opts.policy.published_by, opts.policy.fallback_published_by),
        project_relative_cache_scope(&wanted, opts),
        None,
        Vec::new(),
        ctx.update_cache_scope(),
        is_update_target(ctx.update_scope(), &wanted, None, depth),
        opts.refresh.prefer_current_version,
    ));
    let result = resolve_wanted_cached(ctx, resolver, &wanted, opts, None, cache_key)
        .await
        .ok()?;
    result.package.manifest.clone()
}
