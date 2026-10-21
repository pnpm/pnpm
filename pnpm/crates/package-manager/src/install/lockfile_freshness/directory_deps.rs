pub use injected_manifest::ProjectManifestsByDir;
pub(super) use injected_manifest::{InjectedDirectory, is_deploy_file_url_key, unhooked_manifest};
pub(crate) use injected_manifest::{dependency_manifests_by_dir, project_manifests_by_dir};
pub(crate) use spec::spec_target_and_range;

mod children;
use children::check_local_dep_children_freshness;
mod injected_manifest;
mod peers;
mod scope;
mod spec;

#[cfg(test)]
mod tests;

use super::manifest::ImporterSatisfactionCheck;
use crate::install::lockfile_freshness::FreshnessCheckError;
use peers::PeerShadowing;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::StalenessReason;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_deps_resolver::DependencySpec;
use scope::{AliasScope, Visited, importer_aliases, snapshot_aliases};
use spec::{SpecDirs, spec_satisfies_snapshot_dep};
use std::path::Path;

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
    let mut visited = Visited::new(check.lockfile);
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
        canonical_paths: check.workspace.canonical_paths,
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

fn check_single_dep_spec_directory_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    importer: &pnpm_lockfile::ProjectSnapshot,
    dep_name: &pnpm_lockfile::PkgName,
    dep_spec: &pnpm_lockfile::ResolvedDependencySpec,
    visited: &mut Visited,
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
    visited: &mut Visited,
) -> Result<(), FreshnessCheckError> {
    let snapshot = check.lockfile.snapshots
        .as_ref()
        .and_then(|s| s.get(pkg_key));
    if is_deploy_file_url_key(dep_spec, pkg_key, rel_path) {
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
        &importer_aliases(importer),
        &dep,
        (pkg_key, snapshot, pkg_meta),
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
    scope: &AliasScope,
    dep: &LocalDepContext<'_>,
    (pkg_key, snapshot, pkg_meta): (
        &pnpm_lockfile::PackageKey,
        Option<&pnpm_lockfile::SnapshotEntry>,
        &pnpm_lockfile::PackageMetadata,
    ),
    visited: &mut Visited,
) -> Result<(), FreshnessCheckError> {
    let local_manifest = read_and_override_manifest(check, dep)?;
    let Some(snapshot) = snapshot else {
        return Err(dep.outdated());
    };
    // A revisit in the same peer scope is a cycle or a shared dependency
    // already checked.
    if !visited.first_visit(pkg_key, &local_manifest, scope) {
        return Ok(());
    }
    let peers = PeerShadowing::of(
        &local_manifest,
        |name| scope.contains(name),
        check.config.auto_install_peers,
    );
    let children_scope: AliasScope = scope
        .iter()
        .cloned()
        .chain(snapshot_aliases(snapshot))
        .collect();
    check_local_dep_children_freshness(
        check,
        (dep, &children_scope),
        &local_manifest,
        snapshot,
        &peers.shadowed,
        visited,
    )?;
    peers.check_local_peer_deps_freshness(dep, &local_manifest, pkg_meta)
}
