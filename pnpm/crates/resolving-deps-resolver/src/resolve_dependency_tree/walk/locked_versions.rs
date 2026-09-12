use super::{
    Arc, CurrentPkg, LockfileResolution, PkgNameVerPeer, PreferredVersionsOverlay,
    ResolveDependencyTreeError, ResolvedPackage, TarballRevision, TreeCtx, WantedDependency,
    overlay_lookup_names, update_unpins_edge,
};

/// The overlay's view for one edge, as it joins the resolve cache key: the
/// same range can legitimately pick different versions under levels that
/// resolved different siblings. Each candidate name (alias, `npm:` inner
/// target, folded `jsr:` name) keeps its own versions — the picker consults
/// the overlay per name, so a flat union of versions could collide two
/// overlays that distribute the same versions across different names. Empty
/// for almost every edge, so the dedup keeps working where it matters.
pub(super) fn overlay_version_view(
    overlay: &Arc<PreferredVersionsOverlay>,
    wanted: &WantedDependency,
) -> Vec<(String, Vec<String>)> {
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
}

/// Under `update=patches` the edge re-resolves at its recorded version, so a
/// republished revision of that same version is picked up.
pub(super) fn pin_patched_revision(
    wanted: &mut WantedDependency,
    current_pkg: Option<&CurrentPkg>,
    prior_key: Option<&PkgNameVerPeer>,
) {
    let Some(version) = current_pkg.and_then(|current| current.version.as_deref()) else {
        return;
    };
    let Some(specifier) = wanted.bare_specifier.as_deref() else { return };
    wanted.bare_specifier = exact_registry_specifier_for_revision_refresh(
        specifier,
        version,
        prior_key.and_then(|key| key.suffix.registry_qualified().map(|(name, _)| name)),
    )
    .into();
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
/// directory — see [`declaring_manifest_dir`](crate::resolve_dependency_tree::tree_ctx::declaring_manifest_dir).
///
/// `parent_pkg_aliases` is the scope this edge's level resolves in; it
/// decides which of the resolved package's `dependencies` its own
/// `peerDependencies` shadow (see [`peer_shadowed_dependencies`](crate::parent_pkg_aliases::peer_shadowed_dependencies)).
/// Locked-version pin, the fresh-resolve counterpart of subtree reuse: a
/// transitive edge whose recorded version still satisfies its manifest range
/// (`prior_key` is satisfies-gated) resolves to exactly that version even when
/// its subtree cannot be reused wholesale. Without it, a re-resolve picks open
/// ranges (`*`) against the whole preferred-versions pool and lands every such
/// edge on the highest locked version, churning the lockfile.
///
/// Mirrors the TypeScript resolver's `replaceVersionInBareSpecifier` under
/// `!update`: direct deps (depth 0) keep recomputing their specifier, and an
/// edge a `pacquet update` reaches keeps re-picking. Only plain semver ranges
/// pin; aliased (`npm:`), named-registry, and exotic specifiers keep today's
/// behavior.
pub(super) fn pin_locked_version(
    ctx: &TreeCtx,
    wanted: &mut WantedDependency,
    prior_key: Option<&PkgNameVerPeer>,
    depth: i32,
) {
    let locked_version = prior_key.and_then(|key| key.suffix.version_semver());
    if depth > 0
        && !update_unpins_edge(ctx.update_scope(), wanted, locked_version, depth)
        && let Some(version) = locked_version
        && wanted
            .bare_specifier
            .as_deref()
            .is_some_and(|spec| spec.parse::<node_semver::Range>().is_ok())
    {
        wanted.bare_specifier = Some(version.to_string());
    }
}

pub(super) fn exact_registry_specifier_for_revision_refresh(
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

pub(super) fn has_registry_revision_specifier(specifier: &str) -> bool {
    let selector_start =
        specifier.rfind([':', '@']).map_or(0, |delimiter| delimiter.saturating_add(1));
    let selector = &specifier[selector_start..];
    if node_semver::Version::parse(selector).is_err() {
        return false;
    }
    let Some((_, revision)) = selector.rsplit_once("+r") else { return false };
    !revision.is_empty() && revision.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) fn registry_revisions_conflict(
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
pub(in super::super) fn node_alias(
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

pub(super) fn ensure_same_registry_revision(
    existing: &ResolvedPackage,
    result: &pnpm_resolving_resolver_base::ResolveResult,
) -> Result<(), ResolveDependencyTreeError> {
    if registry_revisions_conflict(&existing.result.resolution, &result.resolution) {
        let name_ver = result.name_ver.as_ref().expect("registry result has name and version");
        return Err(ResolveDependencyTreeError::RevisionConflict {
            name: name_ver.name.to_string(),
            version: name_ver.suffix.to_string(),
        });
    }
    Ok(())
}
