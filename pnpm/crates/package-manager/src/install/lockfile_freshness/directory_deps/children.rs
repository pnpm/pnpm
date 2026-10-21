//! The children a directory dependency's snapshot records, and whether each
//! one still satisfies the spec the resolver walks for it.

use super::{
    FreshnessCheckError, ImporterSatisfactionCheck, LocalDepContext, is_declared_local_directory,
    package::check_single_directory_dep_freshness,
    peers,
    scope::{AliasScope, Visited},
    spec::spec_satisfies_snapshot_dep,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_deps_resolver::{
    ChildCatalogs, ChildSpec, ResolveDependencyTreeError, local_directory_child_specs,
};
use std::collections::HashSet;

/// Compares the children the resolver walks for the package (see
/// [`local_directory_child_specs`]) with the ones its snapshot records.
pub(super) fn check_local_dep_children_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    (dep, scope): (&LocalDepContext<'_>, &AliasScope),
    local_manifest: &PackageManifest,
    snapshot: &pnpm_lockfile::SnapshotEntry,
    shadowed: &HashSet<&str>,
    visited: &mut Visited,
) -> Result<(), FreshnessCheckError> {
    // A `file:` dependency resolves through the local resolver, which leaves
    // its children's `catalog:` specs alone.
    let catalogs = (!dep.declared_as_file).then_some(ChildCatalogs {
        catalogs: dep.catalogs,
        workspace_dir: check.config.workspace_dir.as_deref(),
        package_dir: dep.dir,
    });
    let is_peer_shadowed = |name: &str| shadowed.contains(name);
    let children =
        local_directory_child_specs(local_manifest.value(), dep.name, is_peer_shadowed, catalogs)
            .map_err(|error| match error {
                ResolveDependencyTreeError::CatalogMisconfiguration(error) => {
                    FreshnessCheckError::InvalidCatalog(error)
                }
                _ => dep.outdated(),
            })?;
    check_snapshot_keys_are_children(check, dep, local_manifest, (&children, snapshot))?;
    for child in &children {
        if !child_is_fresh(check, dep, snapshot, child) {
            return Err(dep.outdated());
        }
        check_child_directory_dep_freshness(check, (snapshot, scope), dep, child, visited)?;
    }
    Ok(())
}

fn check_child_directory_dep_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    (parent_snapshot, scope): (&pnpm_lockfile::SnapshotEntry, &AliasScope),
    parent_dep: &LocalDepContext<'_>,
    (name, spec, optional, _): &ChildSpec,
    visited: &mut Visited,
) -> Result<(), FreshnessCheckError> {
    let Some(pkg_key) = (!*optional || check.config.optional)
        .then(|| recorded_child(parent_snapshot, name, *optional))
        .flatten()
        .zip(pnpm_lockfile::PkgName::parse(name).ok())
        .and_then(|(recorded, pkg_name)| recorded.resolve(&pkg_name))
    else {
        return Ok(());
    };
    let Some(pkg_meta) = check.lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&pkg_key.without_peer()))
    else {
        return if is_declared_local_directory(spec) || spec.starts_with("workspace:") {
            Err(parent_dep.outdated())
        } else {
            Ok(())
        };
    };
    let pnpm_lockfile::LockfileResolution::Directory(dir_res) = &pkg_meta.resolution else {
        return Ok(());
    };
    let local_dep_dir = check.lockfile_dir.join(&dir_res.directory);
    let snapshot = check.lockfile.snapshots
        .as_ref()
        .and_then(|s| s.get(&pkg_key));
    let child_dep = parent_dep.child(name, spec, &dir_res.directory, &local_dep_dir);
    check_single_directory_dep_freshness(
        check,
        scope,
        &child_dep,
        (&pkg_key, snapshot, pkg_meta),
        visited,
    )
}

/// Whether the snapshot records a resolution of `child` that its spec
/// accepts. An optional child the install leaves out is only checked not to
/// be recorded as a required one.
fn child_is_fresh(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
    snapshot: &pnpm_lockfile::SnapshotEntry,
    (name, spec, optional, _): &ChildSpec,
) -> bool {
    if *optional && !check.config.optional {
        return recorded_child(snapshot, name, false).is_none();
    }
    match recorded_child(snapshot, name, *optional) {
        Some(recorded) => spec_satisfies_snapshot_dep(&dep.dirs, dep.dir, name, spec, recorded)
            .unwrap_or(check.optional_exclusions.frozen_lockfile),
        // A child recorded as required has changed, not gone unresolved.
        None => {
            *optional
                && check.optional_exclusions.frozen_lockfile
                && recorded_child(snapshot, name, false).is_none()
        }
    }
}

/// The resolution the snapshot records for a child, in the map its
/// optionality puts it in.
fn recorded_child<'s>(
    snapshot: &'s pnpm_lockfile::SnapshotEntry,
    name: &str,
    optional: bool,
) -> Option<&'s pnpm_lockfile::SnapshotDepRef> {
    let name = pnpm_lockfile::PkgName::parse(name).ok()?;
    let recorded = if optional { &snapshot.optional_dependencies } else { &snapshot.dependencies };
    recorded.as_ref()?.get(&name)
}

/// Every child the snapshot records is one the resolver walks, or a peer
/// the package declares, which the snapshot records as resolved.
fn check_snapshot_keys_are_children(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
    local_manifest: &PackageManifest,
    (children, snapshot): (&[ChildSpec], &pnpm_lockfile::SnapshotEntry),
) -> Result<(), FreshnessCheckError> {
    let expected: HashSet<&str> = children
        .iter()
        .map(|(name, ..)| name.as_str())
        .chain(
            local_manifest
                .dependencies([DependencyGroup::Peer])
                .map(|(name, _)| name),
        )
        .chain(peers::optional_peer_names(local_manifest))
        .collect();
    // An install without optional dependencies leaves the optional map alone.
    let optional =
        check.config.optional.then_some(snapshot.optional_dependencies.as_ref()).flatten();
    let recorded = [snapshot.dependencies.as_ref(), optional]
        .into_iter()
        .flatten()
        .flat_map(|deps| deps.keys());
    for name in recorded {
        if !expected.contains(name.to_string().as_str()) {
            return Err(dep.outdated());
        }
    }
    Ok(())
}
