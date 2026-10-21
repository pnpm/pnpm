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
    ChildCatalogs, DependencySpec, ResolveDependencyTreeError, local_directory_child_specs,
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
    child: &DependencySpec,
    visited: &mut Visited,
) -> Result<(), FreshnessCheckError> {
    let Some(pkg_key) = (!child.optional || check.config.optional)
        .then(|| recorded_child(parent_snapshot, &child.alias, RecordedDeps::of(child)))
        .flatten()
        .zip(child.alias.parse::<pnpm_lockfile::PkgName>().ok())
        .and_then(|(recorded, pkg_name)| recorded.resolve(&pkg_name))
    else {
        return Ok(());
    };
    let Some(pkg_meta) = check.lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&pkg_key.without_peer()))
    else {
        let spec = child.bare_specifier.as_str();
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
    let child_dep =
        parent_dep.child(&child.alias, &child.bare_specifier, &dir_res.directory, &local_dep_dir);
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
    child: &DependencySpec,
) -> bool {
    let name = child.alias.as_str();
    if child.optional && !check.config.optional {
        return recorded_child(snapshot, name, RecordedDeps::Required).is_none();
    }
    match recorded_child(snapshot, name, RecordedDeps::of(child)) {
        Some(recorded) => {
            spec_satisfies_snapshot_dep(&dep.dirs, dep.dir, name, &child.bare_specifier, recorded)
                .unwrap_or(check.optional_exclusions.frozen_lockfile)
        }
        // A child recorded as required has changed, not gone unresolved.
        None => {
            child.optional
                && check.optional_exclusions.frozen_lockfile
                && recorded_child(snapshot, name, RecordedDeps::Required).is_none()
        }
    }
}

/// Which of a snapshot's two dependency maps a child is recorded in.
#[derive(Clone, Copy)]
enum RecordedDeps {
    Required,
    Optional,
}

impl RecordedDeps {
    /// Where a child of this optionality is recorded.
    fn of(child: &DependencySpec) -> Self {
        if child.optional { RecordedDeps::Optional } else { RecordedDeps::Required }
    }
}

/// The resolution the snapshot records for a child in `deps`.
fn recorded_child<'s>(
    snapshot: &'s pnpm_lockfile::SnapshotEntry,
    name: &str,
    deps: RecordedDeps,
) -> Option<&'s pnpm_lockfile::SnapshotDepRef> {
    let name = pnpm_lockfile::PkgName::parse(name).ok()?;
    let recorded = match deps {
        RecordedDeps::Required => &snapshot.dependencies,
        RecordedDeps::Optional => &snapshot.optional_dependencies,
    };
    recorded.as_ref()?.get(&name)
}

/// Every child the snapshot records is one the resolver walks, or a peer
/// the package declares, which the snapshot records as resolved.
fn check_snapshot_keys_are_children(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
    local_manifest: &PackageManifest,
    (children, snapshot): (&[DependencySpec], &pnpm_lockfile::SnapshotEntry),
) -> Result<(), FreshnessCheckError> {
    let expected: HashSet<&str> = children
        .iter()
        .map(|spec| spec.alias.as_str())
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
