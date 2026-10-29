//! A directory package whose lockfile entry records the specifiers the
//! resolver read (see [`pnpm_lockfile::DeclaredSpecifiers`]): the check
//! compares them with the directory's hooked manifest before it compares the
//! snapshot with the children, so a changed specifier is a change even while
//! its locked resolution still satisfies it.

use super::{
    FreshnessCheckError, ImporterSatisfactionCheck, LocalDepContext,
    children::check_local_dep_children_freshness,
    scope::{AliasScope, Visited},
};
use pnpm_lockfile::DeclaredSpecifiers;
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::{declared_specifiers_of, manifest_child_specs};
use std::collections::HashSet;

/// The specifiers `local_manifest` declares must equal `recorded`. The
/// children are then checked against the snapshot as those of a package
/// without a record are, since a workspace project or a directory can drift
/// without its specifier changing.
pub(super) fn check_recorded_children_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    (dep, scope): (&LocalDepContext<'_>, &AliasScope),
    (local_manifest, recorded): (&PackageManifest, &DeclaredSpecifiers),
    snapshot: &pnpm_lockfile::SnapshotEntry,
    shadowed: &HashSet<&str>,
    visited: &mut Visited,
) -> Result<(), FreshnessCheckError> {
    let declared =
        manifest_child_specs(local_manifest.value(), dep.name).map_err(|_| dep.outdated())?;
    if declared_specifiers_of(&declared) != *recorded {
        return Err(dep.outdated());
    }
    check_local_dep_children_freshness(
        check,
        (dep, scope),
        local_manifest,
        snapshot,
        shadowed,
        visited,
    )
}
