//! A `file:` dependency a package pnpm never unpacks to a directory declares.

use super::{
    ChildEdge, ResolveDependencyTreeError, ResolveOptions, SkippedOptionalDependency, TreeCtx,
    WantedDependency, pkgs_info_from_ids, wanted_lockfile_contains_satisfying_entry,
};

/// Whether this failure is a `file:` specifier that names a path inside the
/// package declaring it.
///
/// A relative `file:` specifier resolves against the directory of the
/// manifest that declares it. A package pnpm resolved from a tarball or the
/// registry has no such directory — the target ships inside the package
/// itself — so the path can never exist and pnpm has nothing to resolve it
/// against. `@eslint/css@0.3.0` declared `"@types/css-tree":
/// "file:./typings/css-tree"` that way, and `pnpm add -D @eslint/css@0.3.0`
/// failed with `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND`.
///
/// A project of pnpm's own is the other way round: there the path is the
/// user's, and a path that does not exist is a mistake worth reporting.
fn is_unresolvable_file_dep_of_packed_pkg(
    wanted: &WantedDependency,
    edge: &ChildEdge<'_>,
    err: &ResolveDependencyTreeError,
) -> bool {
    !edge.parent_is_directory
        && matches!(err, ResolveDependencyTreeError::LinkedPkgDirNotFound(_))
        && is_relative_file_specifier(wanted.bare_specifier.as_deref())
}

/// Whether the specifier is a `file:` path written relative to the manifest
/// that declares it, the only form that can point inside the declaring
/// package. An absolute target names a place of its own, so its being
/// missing says nothing about that package and stays an error.
fn is_relative_file_specifier(bare_specifier: Option<&str>) -> bool {
    bare_specifier.is_some_and(|spec| spec.starts_with("file:./") || spec.starts_with("file:../"))
}

/// Report a dropped [`is_unresolvable_file_dep_of_packed_pkg`] edge through
/// the same sink a skipped optional dependency uses, so both reach the
/// reporter as `pnpm:skipped-optional-dependency` with
/// `reason=resolution_failure`.
///
/// `true` when the edge was dropped. The wanted lockfile still wins: an
/// entry satisfying the specifier means the install has to keep resolving
/// it, or the lockfile would differ depending on which machine ran it.
pub(super) fn drop_unresolvable_file_dep_edge(
    ctx: &TreeCtx,
    wanted: &WantedDependency,
    edge: &ChildEdge<'_>,
    opts: &ResolveOptions,
    err: &ResolveDependencyTreeError,
) -> bool {
    if !is_unresolvable_file_dep_of_packed_pkg(wanted, edge, err)
        || wanted_lockfile_contains_satisfying_entry(
            ctx.workspace.reuse.lockfile.as_deref(),
            wanted,
        )
    {
        return false;
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
    true
}
