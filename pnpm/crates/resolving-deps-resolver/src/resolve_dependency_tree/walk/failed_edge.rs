use super::{
    ChildEdge, ResolveDependencyTreeError, ResolveOptions, SkippedOptionalDependency, TreeCtx,
    WantedDependency, pkgs_info_from_ids, wanted_lockfile_contains_satisfying_entry,
};

/// A resolution failure inside an optional subtree drops the edge instead of
/// failing the install. On an optional edge the edge alone is dropped. On a
/// regular edge below an optional one, the parent package is recorded as
/// broken: once the walk is done, the nearest optional dependency above it is
/// dropped with it, or the install fails if a regular path reaches it (see
/// [`fn@crate::resolve_workspace`]).
///
/// The wanted lockfile holding a satisfying entry still fails the install,
/// since the silent skip would erase the locked entries (see
/// [`fn@wanted_lockfile_contains_satisfying_entry`]). `Ok(())` means the edge
/// is dropped; every other failure propagates.
pub(super) fn drop_failed_edge(
    ctx: &TreeCtx,
    wanted: &WantedDependency,
    edge: &ChildEdge<'_>,
    opts: &ResolveOptions,
    err: ResolveDependencyTreeError,
) -> Result<(), ResolveDependencyTreeError> {
    let optional = wanted.optional.unwrap_or(false);
    let broken_parent = edge.ancestor_ids
        .last()
        .filter(|_| !optional && edge.parent_optional);
    if !(optional || broken_parent.is_some()) || !is_droppable_resolve_error(&err) {
        return Err(err);
    }
    if wanted_lockfile_contains_satisfying_entry(ctx.workspace.reuse.lockfile.as_deref(), wanted) {
        return Err(ResolveDependencyTreeError::LockedOptionalResolutionFailure(Box::new(err)));
    }
    if let Some(log) = ctx.workspace.hooks.skipped_optional_log.as_ref() {
        log(SkippedOptionalDependency {
            details: err.to_string(),
            name: wanted.alias.clone(),
            version: wanted.alias
                .is_some()
                .then(|| wanted.bare_specifier.clone())
                .flatten(),
            bare_specifier: wanted.bare_specifier.clone().unwrap_or_default(),
            parents: pkgs_info_from_ids(ctx, edge.ancestor_ids),
            prefix: opts.project.project_dir.display().to_string(),
        });
    }
    if let Some(parent) = broken_parent {
        ctx.workspace.record_broken_package(parent, err);
    }
    Ok(())
}

/// Hook errors keep aborting even for optional edges.
pub(super) fn is_droppable_resolve_error(err: &ResolveDependencyTreeError) -> bool {
    matches!(
        err,
        ResolveDependencyTreeError::Resolve(_)
            | ResolveDependencyTreeError::NoMatchingVersion(_)
            | ResolveDependencyTreeError::RegistryResponse(_)
            | ResolveDependencyTreeError::GitResolve(_)
            | ResolveDependencyTreeError::Pick(_)
            | ResolveDependencyTreeError::SpecNotSupported { .. },
    )
}
