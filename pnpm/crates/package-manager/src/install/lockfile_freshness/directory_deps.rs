pub use injected_manifest::ProjectManifestsByDir;
pub(super) use injected_manifest::{InjectedDirectory, unhooked_manifest};
pub(crate) use injected_manifest::{dependency_manifests_by_dir, project_manifests_by_dir};
pub(crate) use spec::spec_target_and_range;

mod injected_manifest;
mod peers;
mod spec;

#[cfg(test)]
mod tests;

use super::manifest::ImporterSatisfactionCheck;
use crate::install::lockfile_freshness::FreshnessCheckError;
use peers::PeerShadowing;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::StalenessReason;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_deps_resolver::{
    ChildCatalogs, ChildSpec, ResolveDependencyTreeError, local_directory_child_specs,
};
use spec::{SpecDirs, spec_satisfies_snapshot_dep};
use std::{collections::HashSet, path::Path};

struct LocalDepContext<'a> {
    name: &'a str,
    /// The importer declared the dependency with a `file:` spec, which the
    /// resolver reads from the directory itself rather than from a workspace
    /// project.
    declared_as_file: bool,
    rel_path: &'a str,
    dir: &'a Path,
    dirs: SpecDirs<'a>,
    catalogs: &'a Catalogs,
}

impl<'a> LocalDepContext<'a> {
    fn outdated(&self) -> FreshnessCheckError {
        local_dependency_outdated(self.name, self.rel_path)
    }

    fn child(&self, name: &'a str, spec: &str, rel_path: &'a str, dir: &'a Path) -> Self {
        Self {
            name,
            declared_as_file: is_declared_local_directory(spec),
            rel_path,
            dir,
            dirs: self.dirs,
            catalogs: self.catalogs,
        }
    }
}

fn local_dependency_outdated(name: &str, path: &str) -> FreshnessCheckError {
    FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated {
        name: name.to_string(),
        path: path.to_string(),
    })
}

pub(crate) fn check_directory_dependencies_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    importer: &pnpm_lockfile::ProjectSnapshot,
) -> Result<(), FreshnessCheckError> {
    let mut visited = HashSet::new();
    for group in [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional] {
        if let Some(dep_map) = importer.get_map_by_group(group) {
            for (dep_name, dep_spec) in dep_map {
                check_single_dep_spec_directory_freshness(
                    check,
                    importer,
                    dep_name,
                    dep_spec,
                    &mut visited,
                )?;
            }
        }
    }
    Ok(())
}

fn dependency_is_injected(
    check: &ImporterSatisfactionCheck<'_>,
    importer: &pnpm_lockfile::ProjectSnapshot,
    dep_name: &str,
) -> bool {
    if check.config.inject_workspace_packages {
        return true;
    }
    if check.lockfile.settings.as_ref().is_some_and(|s| s.inject_workspace_packages) {
        return true;
    }
    if let Some(meta) = importer.dependencies_meta.as_ref()
        && meta
            .get(dep_name)
            .and_then(|e| e.get("injected"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    {
        return true;
    }
    check.manifest
        .value()
        .get("dependenciesMeta")
        .and_then(serde_json::Value::as_object)
        .and_then(|meta| meta.get(dep_name))
        .and_then(|entry| entry.get("injected"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn is_declared_local_directory(spec: &str) -> bool {
    let Some(path) = spec.strip_prefix("file:") else {
        return false;
    };
    !pnpm_lockfile::is_local_tarball_path(path)
}

fn spec_dirs<'a>(check: &'a ImporterSatisfactionCheck<'a>) -> SpecDirs<'a> {
    SpecDirs {
        workspace_root: check.config.workspace_dir.as_deref().unwrap_or(check.lockfile_dir),
        lockfile_dir: check.lockfile_dir,
        manifests_by_dir: check.workspace.manifests_by_dir,
    }
}

fn check_importer_link_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    dep_name: &str,
    specifier: &str,
    link_target: &str,
) -> Result<(), FreshnessCheckError> {
    let project_dir = check.manifest
        .path()
        .parent()
        .unwrap_or(check.lockfile_dir);
    let dirs = spec_dirs(check);
    if !spec::importer_link_satisfies(&dirs, project_dir, dep_name, specifier, link_target) {
        return Err(local_dependency_outdated(dep_name, link_target));
    }
    Ok(())
}

fn should_check_directory_dep(
    check: &ImporterSatisfactionCheck<'_>,
    importer: &pnpm_lockfile::ProjectSnapshot,
    dep_name: &str,
    specifier: &str,
) -> bool {
    check.lockfile.packages.is_some()
        && check.lockfile.snapshots.is_some()
        && (dependency_is_injected(check, importer, dep_name)
            || is_declared_local_directory(specifier))
}

/// Whether `key` is a `<name>@file://...` directory entry synthesized by
/// `pnpm deploy`, whose snapshot already carries the source workspace's
/// resolved catalog entries, overrides, extensions, hooks, and peer bindings.
pub(super) fn is_deploy_file_url_key(key: &pnpm_lockfile::PackageKey) -> bool {
    matches!(
        key.suffix.version(),
        pnpm_lockfile::VersionPart::File(path)
            if path.starts_with("//") && !pnpm_lockfile::is_local_tarball_path(path),
    )
}

fn check_single_dep_spec_directory_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    importer: &pnpm_lockfile::ProjectSnapshot,
    dep_name: &pnpm_lockfile::PkgName,
    dep_spec: &pnpm_lockfile::ResolvedDependencySpec,
    visited: &mut HashSet<pnpm_lockfile::PackageKey>,
) -> Result<(), FreshnessCheckError> {
    let dep_str = dep_name.to_string();
    if let pnpm_lockfile::ImporterDepVersion::Link(link_target) = &dep_spec.version {
        return check_importer_link_freshness(check, &dep_str, &dep_spec.specifier, link_target);
    }
    if !should_check_directory_dep(check, importer, &dep_str, &dep_spec.specifier) {
        return Ok(());
    }
    let Some(pkg_key) = dep_spec.version.resolved_key(dep_name) else { return Ok(()) };
    let Some(pkg_meta) = check.lockfile.packages
        .as_ref()
        .and_then(|p| p.get(&pkg_key.without_peer()))
    else {
        return Err(local_dependency_outdated(&dep_str, &dep_spec.specifier));
    };
    let pnpm_lockfile::LockfileResolution::Directory(dir_res) = &pkg_meta.resolution else {
        return Ok(());
    };
    if !visited.insert(pkg_key.clone()) {
        return Ok(());
    }
    check_directory_resolution(
        check,
        importer,
        (&dep_str, dep_spec),
        (&pkg_key, pkg_meta, &dir_res.directory),
        visited,
    )
}

fn check_directory_resolution(
    check: &ImporterSatisfactionCheck<'_>,
    importer: &pnpm_lockfile::ProjectSnapshot,
    (dep_str, dep_spec): (&str, &pnpm_lockfile::ResolvedDependencySpec),
    (pkg_key, pkg_meta, rel_path): (
        &pnpm_lockfile::PackageKey,
        &pnpm_lockfile::PackageMetadata,
        &str,
    ),
    visited: &mut HashSet<pnpm_lockfile::PackageKey>,
) -> Result<(), FreshnessCheckError> {
    let snapshot = check.lockfile.snapshots
        .as_ref()
        .and_then(|s| s.get(pkg_key));
    if is_deploy_file_url_key(pkg_key) {
        return snapshot
            .map(|_| ())
            .ok_or_else(|| local_dependency_outdated(dep_str, rel_path));
    }
    let local_dep_dir = check.lockfile_dir.join(rel_path);
    let dep = LocalDepContext {
        name: dep_str,
        declared_as_file: is_declared_local_directory(&dep_spec.specifier),
        rel_path,
        dir: &local_dep_dir,
        dirs: spec_dirs(check),
        catalogs: check.workspace.catalogs,
    };
    check_single_directory_dep_freshness(
        check,
        |name| importer_declares(importer, name),
        &dep,
        (snapshot, pkg_meta),
        visited,
    )
}

fn read_and_override_manifest(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
) -> Result<PackageManifest, FreshnessCheckError> {
    let mut local_manifest = unhooked_manifest(&InjectedDirectory {
        dir: dep.dir,
        declared_as_file: dep.declared_as_file,
        workspace_root: dep.dirs.workspace_root,
        manifests_by_dir: check.workspace.manifests_by_dir,
    })
    .ok_or_else(|| dep.outdated())?;
    if let Some(parsed) = check.parsed_overrides {
        crate::VersionsOverrider::new(parsed, check.lockfile_dir)
            .apply(&mut local_manifest, Some(dep.dir));
    }
    Ok(local_manifest)
}

fn check_single_directory_dep_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    parent_declares: impl Fn(&str) -> bool,
    dep: &LocalDepContext<'_>,
    (snapshot, pkg_meta): (Option<&pnpm_lockfile::SnapshotEntry>, &pnpm_lockfile::PackageMetadata),
    visited: &mut HashSet<pnpm_lockfile::PackageKey>,
) -> Result<(), FreshnessCheckError> {
    let local_manifest = read_and_override_manifest(check, dep)?;
    let Some(snapshot) = snapshot else {
        return Err(dep.outdated());
    };
    let peers =
        PeerShadowing::of(&local_manifest, parent_declares, check.config.auto_install_peers);
    check_local_dep_children_freshness(
        check,
        dep,
        &local_manifest,
        snapshot,
        &peers.shadowed,
        visited,
    )?;
    peers.check_local_peer_deps_freshness(dep, &local_manifest, pkg_meta)
}

fn importer_declares(importer: &pnpm_lockfile::ProjectSnapshot, name: &str) -> bool {
    let Ok(name) = pnpm_lockfile::PkgName::parse(name) else { return false };
    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional]
        .into_iter()
        .filter_map(|group| importer.get_map_by_group(group))
        .any(|dependencies| dependencies.contains_key(&name))
}

fn snapshot_declares(snapshot: &pnpm_lockfile::SnapshotEntry, name: &str) -> bool {
    let Ok(name) = pnpm_lockfile::PkgName::parse(name) else { return false };
    [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
        .into_iter()
        .flatten()
        .any(|dependencies| dependencies.contains_key(&name))
}

/// Compares the children the resolver walks for the package (see
/// [`local_directory_child_specs`]) with the ones its snapshot records.
fn check_local_dep_children_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
    local_manifest: &PackageManifest,
    snapshot: &pnpm_lockfile::SnapshotEntry,
    shadowed: &HashSet<&str>,
    visited: &mut HashSet<pnpm_lockfile::PackageKey>,
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
        check_child_directory_dep_freshness(check, snapshot, dep, child, visited)?;
    }
    Ok(())
}

fn check_child_directory_dep_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    parent_snapshot: &pnpm_lockfile::SnapshotEntry,
    parent_dep: &LocalDepContext<'_>,
    (name, spec, optional, _): &ChildSpec,
    visited: &mut HashSet<pnpm_lockfile::PackageKey>,
) -> Result<(), FreshnessCheckError> {
    let Some(pkg_key) = (!*optional || check.config.optional)
        .then(|| recorded_child(parent_snapshot, name, *optional))
        .flatten()
        .zip(pnpm_lockfile::PkgName::parse(name).ok())
        .and_then(|(recorded, pkg_name)| recorded.resolve(&pkg_name))
        .filter(|pkg_key| visited.insert(pkg_key.clone()))
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
        |peer_name| snapshot_declares(parent_snapshot, peer_name),
        &child_dep,
        (snapshot, pkg_meta),
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
