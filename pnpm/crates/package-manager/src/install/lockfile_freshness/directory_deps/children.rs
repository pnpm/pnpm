//! The children a directory dependency's snapshot records, and whether each
//! one still satisfies the spec the resolver walks for it.

use super::{
    FreshnessCheckError, ImporterSatisfactionCheck, LocalDepContext, is_declared_local_directory,
    package::check_single_directory_dep_freshness,
    scope::{AliasScope, Visited},
    spec::spec_satisfies_snapshot_dep,
};
use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::{
    ChildCatalogs, DependencySpec, ResolveDependencyTreeError, declared_peer_dependencies,
    directory_package_dir, local_directory_child_specs,
};
use std::collections::{HashMap, HashSet};
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
    let children = local_dep_children(check, (dep, local_manifest), shadowed)?;
    check_snapshot_keys_are_children(check, dep, local_manifest, (&children, snapshot))?;
    for child in &children {
        if !child_is_fresh(check, dep, snapshot, child) {
            return Err(dep.outdated());
        }
        check_child_directory_dep_freshness(check, (snapshot, scope), dep, child, visited)?;
    }
    Ok(())
}

fn local_dep_children(
    check: &ImporterSatisfactionCheck<'_>,
    (dep, manifest): (&LocalDepContext<'_>, &PackageManifest),
    shadowed: &HashSet<&str>,
) -> Result<Vec<DependencySpec>, FreshnessCheckError> {
    let catalogs = (!dep.declared_by_path).then_some(ChildCatalogs {
        catalogs: dep.catalogs,
        workspace_dir: check.config.workspace_dir.as_deref(),
        package_dir: dep.dir,
    });
    let is_peer_shadowed = |name: &str| shadowed.contains(name);
    local_directory_child_specs(manifest.value(), dep.name, is_peer_shadowed, catalogs)
        .map_err(|error| match error {
            ResolveDependencyTreeError::CatalogMisconfiguration(error) => {
                FreshnessCheckError::InvalidCatalog(error)
            }
            _ => dep.outdated(),
        })
}

pub(super) fn check_recorded_local_targets(
    check: &ImporterSatisfactionCheck<'_>,
    (dep, scope): (&LocalDepContext<'_>, &AliasScope),
    local_manifest: &PackageManifest,
    (snapshot, key): (&pnpm_lockfile::SnapshotEntry, &pnpm_lockfile::PackageKey),
    shadowed: &HashSet<&str>,
    visited: &mut Visited,
) -> Result<(), FreshnessCheckError> {
    let children = local_dep_children(check, (dep, local_manifest), shadowed)?;
    let bound = std::cell::LazyCell::new(|| {
        pnpm_lockfile::BoundProviders::from_recorded_snapshot(check.lockfile.into(), key)
    });
    let child_catalogs = local_child_catalogs(dep, local_manifest)?;
    for child in &children {
        if child.optional && !check.config.optional {
            continue;
        }
        if (child_catalogs
            .get(&child.alias)
            .is_some_and(|catalog| {
                recorded_catalog_spec_matches(check, dep, &child.alias, catalog) == Some(false)
                    || !(catalog_child_is_fresh(check, dep, snapshot, child)
                        || peer_replaces_child(
                            check,
                            dep,
                            local_manifest,
                            (snapshot, key, &bound),
                            child,
                        ))
            }))
            || !recorded_local_target_is_fresh(check, dep, snapshot, child)
        {
            return Err(dep.outdated());
        }
        check_child_directory_dep_freshness(check, (snapshot, scope), dep, child, visited)?;
    }
    Ok(())
}

fn local_child_catalogs(
    dep: &LocalDepContext<'_>,
    manifest: &PackageManifest,
) -> Result<HashMap<String, String>, FreshnessCheckError> {
    if dep.declared_by_path {
        return Ok(HashMap::new());
    }
    Ok(pnpm_resolving_deps_resolver::manifest_child_specs(manifest.value(), dep.name)
        .map_err(|_| dep.outdated())?
        .into_iter()
        .filter_map(|child| {
            parse_catalog_protocol(&child.bare_specifier)
                .map(|catalog| (child.alias.clone(), catalog.to_string()))
        })
        .collect())
}

fn peer_replaces_child(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
    manifest: &PackageManifest,
    (snapshot, key, bound): (
        &pnpm_lockfile::SnapshotEntry,
        &pnpm_lockfile::PackageKey,
        &pnpm_lockfile::BoundProviders,
    ),
    child: &DependencySpec,
) -> bool {
    let Some(optional) =
        declared_peer_dependencies(manifest.value()).get(&child.alias).map(|peer| peer.optional)
    else {
        return false;
    };
    if optional && !check.config.optional {
        return false;
    }
    let group = if optional { RecordedDeps::Optional } else { RecordedDeps::Required };
    let Some(recorded) = recorded_child(snapshot, &child.alias, group) else {
        // The shared children's owner may take a peer this occurrence leaves unbound.
        return recorded_child(snapshot, &child.alias, RecordedDeps::Required).is_none()
            && recorded_child(snapshot, &child.alias, RecordedDeps::Optional).is_none()
            && check.workspace.peer_children.contains(check.lockfile, key, &child.alias);
    };
    (!bound.is_hashed()
        && child.alias
            .parse()
            .ok()
            .and_then(|name| recorded.resolve(&name))
            .is_some_and(|provider| bound.binds(&provider)))
        || implicit_workspace_target_satisfies(dep, child, recorded)
            .unwrap_or_else(|| recorded_child_satisfies_spec(check, dep, child, recorded))
}

fn recorded_catalog_spec_matches(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
    alias: &str,
    catalog: &str,
) -> Option<bool> {
    let recorded = check.lockfile.catalogs
        .as_ref()?
        .get(catalog)?
        .get(alias)?;
    Some(
        dep.catalogs
            .get(catalog)
            .and_then(|entries| entries.get(alias))
            == Some(&recorded.specifier),
    )
}

pub(super) fn catalog_child_is_fresh(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
    snapshot: &pnpm_lockfile::SnapshotEntry,
    child: &DependencySpec,
) -> bool {
    recorded_child(snapshot, &child.alias, RecordedDeps::of(child))
        .and_then(|recorded| implicit_workspace_target_satisfies(dep, child, recorded))
        .unwrap_or_else(|| child_is_fresh(check, dep, snapshot, child))
}

fn recorded_local_target_is_fresh(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
    snapshot: &pnpm_lockfile::SnapshotEntry,
    child: &DependencySpec,
) -> bool {
    let recorded = recorded_child(snapshot, &child.alias, RecordedDeps::of(child));
    if let Some(satisfied) =
        recorded.and_then(|recorded| implicit_workspace_target_satisfies(dep, child, recorded))
    {
        return satisfied;
    }
    let local = child.bare_specifier.starts_with("workspace:")
        || child.bare_specifier.starts_with("link:")
        || super::is_declared_by_path(&child.bare_specifier)
        || recorded.is_some_and(|recorded| recorded.as_link_target().is_some());
    if !local {
        return true;
    }
    child_is_fresh(check, dep, snapshot, child)
        && recorded
            .and_then(|recorded| recorded.as_link_target())
            .is_none_or(|target| {
                super::spec::importer_range_link_satisfies(
                    &dep.dirs,
                    check.lockfile_dir,
                    &child.alias,
                    &child.bare_specifier,
                    target,
                )
            })
}

fn implicit_workspace_target_satisfies(
    dep: &LocalDepContext<'_>,
    child: &DependencySpec,
    recorded: &pnpm_lockfile::SnapshotDepRef,
) -> Option<bool> {
    if recorded
        .ver_peer()
        .is_some_and(|version| matches!(version.version(), pnpm_lockfile::VersionPart::File(_)))
        && !child.bare_specifier.starts_with("workspace:")
        && !super::is_declared_by_path(&child.bare_specifier)
    {
        let spec = child.bare_specifier.strip_prefix("npm:").unwrap_or(&child.bare_specifier);
        return Some(
            super::spec::spec_satisfies_snapshot_dep(
                &dep.dirs,
                dep.dir,
                &child.alias,
                &format!("workspace:{spec}"),
                recorded,
            )
            .unwrap_or(false),
        );
    }
    None
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
    let local_dep_dir = directory_package_dir(check.lockfile_dir, &dir_res.directory);
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
        // No resolver accepts the `catalog:` spec of a child of a package
        // declared by path, so a frozen install must not keep the old one.
        Some(_)
            if dep.declared_by_path && parse_catalog_protocol(&child.bare_specifier).is_some() =>
        {
            dep.declaration_assumed
        }
        Some(recorded) => recorded_child_satisfies_spec(check, dep, child, recorded),
        // A child recorded as required has changed, not gone unresolved.
        None => {
            child.optional
                && check.optional_exclusions.frozen_lockfile
                && recorded_child(snapshot, name, RecordedDeps::Required).is_none()
        }
    }
}

fn recorded_child_satisfies_spec(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
    child: &DependencySpec,
    recorded: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    spec_satisfies_snapshot_dep(&dep.dirs, dep.dir, &child.alias, &child.bare_specifier, recorded)
        .unwrap_or(check.optional_exclusions.frozen_lockfile)
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
    let peers = declared_peer_dependencies(local_manifest.value());
    let expected: HashSet<&str> = children
        .iter()
        .map(|spec| spec.alias.as_str())
        .chain(peers.keys().map(String::as_str))
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
