mod peers;
mod spec;

#[cfg(test)]
mod tests;

use super::manifest::ImporterSatisfactionCheck;
use crate::install::lockfile_freshness::FreshnessCheckError;
use peers::PeerShadowing;
use pnpm_catalogs_types::Catalogs;
use pnpm_injected_deps_syncer::publish_source_dir;
use pnpm_lockfile::StalenessReason;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_deps_resolver::{
    ChildCatalogs, DependencySpec, ResolveDependencyTreeError, local_directory_child_specs,
};
use spec::{SpecDirs, spec_satisfies_snapshot_dep};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// Project manifests keyed by their lexically normalized project directory.
pub type ProjectManifestsByDir<'a> = HashMap<PathBuf, &'a PackageManifest>;

/// Each project's manifest as another importer's injected dependency reads
/// it: the project's entry in `dependency_manifests` when it has one, else
/// its importer manifest.
pub(crate) fn project_manifests_by_dir<'a>(
    manifests: impl IntoIterator<Item = &'a PackageManifest>,
    dependency_manifests: Option<&ProjectManifestsByDir<'a>>,
) -> ProjectManifestsByDir<'a> {
    let mut by_dir: ProjectManifestsByDir<'a> = manifests
        .into_iter()
        .filter_map(|manifest| {
            let dir = manifest.path().parent()?;
            Some((pnpm_fs::lexical_normalize(dir), manifest))
        })
        .collect();
    if let Some(dependency_manifests) = dependency_manifests {
        by_dir.extend(
            dependency_manifests
                .iter()
                .map(|(dir, manifest)| (dir.clone(), *manifest)),
        );
    }
    by_dir
}

/// The dependency manifests `projects` carry (see
/// [`pnpm_workspace::Project::dependency_manifest`]), keyed by project
/// directory. `None` when no project carries one, which is every install the
/// Node-API binding did not hand such a manifest to.
pub(crate) fn dependency_manifests_by_dir(
    projects: Option<&[pnpm_workspace::Project]>,
) -> Option<ProjectManifestsByDir<'_>> {
    let by_dir: ProjectManifestsByDir<'_> = projects?
        .iter()
        .filter_map(|project| {
            let manifest = project.dependency_manifest.as_ref()?;
            Some((pnpm_fs::lexical_normalize(&project.root_dir), manifest))
        })
        .collect();
    (!by_dir.is_empty()).then_some(by_dir)
}

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

impl LocalDepContext<'_> {
    fn outdated(&self) -> FreshnessCheckError {
        local_dependency_outdated(self.name, self.rel_path)
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
    if check.lockfile.packages.is_none() || check.lockfile.snapshots.is_none() {
        return Ok(());
    }
    for group in [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional] {
        if let Some(dep_map) = importer.get_map_by_group(group) {
            for (dep_name, dep_spec) in dep_map {
                check_single_dep_spec_directory_freshness(check, importer, dep_name, dep_spec)?;
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

pub(super) fn is_declared_local_directory(spec: &str) -> bool {
    let Some(path) = spec.strip_prefix("file:") else {
        return false;
    };
    !pnpm_lockfile::is_local_tarball_path(path)
}

fn check_single_dep_spec_directory_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    importer: &pnpm_lockfile::ProjectSnapshot,
    dep_name: &pnpm_lockfile::PkgName,
    dep_spec: &pnpm_lockfile::ResolvedDependencySpec,
) -> Result<(), FreshnessCheckError> {
    if matches!(dep_spec.version, pnpm_lockfile::ImporterDepVersion::Link(_)) {
        return Ok(());
    }
    let dep_str = dep_name.to_string();
    if !dependency_is_injected(check, importer, &dep_str)
        && !is_declared_local_directory(&dep_spec.specifier)
    {
        return Ok(());
    }
    let Some(pkg_key) = dep_spec.version.resolved_key(dep_name) else {
        return Ok(());
    };
    let Some(pkg_meta) = check.lockfile.packages
        .as_ref()
        .and_then(|p| p.get(&pkg_key.without_peer()))
    else {
        return Err(local_dependency_outdated(&dep_str, &dep_spec.specifier));
    };
    if let pnpm_lockfile::LockfileResolution::Directory(dir_res) = &pkg_meta.resolution {
        let local_dep_dir = check.lockfile_dir.join(&dir_res.directory);
        let snapshot = check.lockfile.snapshots
            .as_ref()
            .and_then(|s| s.get(&pkg_key));
        let dep = LocalDepContext {
            name: &dep_str,
            declared_as_file: is_declared_local_directory(&dep_spec.specifier),
            rel_path: &dir_res.directory,
            dir: &local_dep_dir,
            dirs: SpecDirs {
                workspace_root: check.config.workspace_dir.as_deref().unwrap_or(check.lockfile_dir),
                lockfile_dir: check.lockfile_dir,
                manifests_by_dir: check.workspace.manifests_by_dir,
            },
            catalogs: check.workspace.catalogs,
        };
        check_single_directory_dep_freshness(check, importer, &dep, (snapshot, pkg_meta))?;
    }
    Ok(())
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

/// A local directory dependency's directory and where its manifest may come
/// from.
pub(super) struct InjectedDirectory<'a> {
    pub(super) dir: &'a Path,
    /// See [`LocalDepContext::declared_as_file`].
    pub(super) declared_as_file: bool,
    pub(super) workspace_root: &'a Path,
    pub(super) manifests_by_dir: &'a ProjectManifestsByDir<'a>,
}

/// The manifest the resolver described the dependency with, before any hook.
/// A workspace project injected as its `publishConfig.directory` is
/// described by the project's own manifest, not by a `package.json` a build
/// wrote into that directory. A `file:` dependency is described by the
/// directory's `package.json`, and by the publishing project's only while
/// that is missing.
pub(super) fn unhooked_manifest(injected: &InjectedDirectory<'_>) -> Option<PackageManifest> {
    let project_manifest = || {
        injected.manifests_by_dir
            .get(&pnpm_fs::lexical_normalize(injected.dir))
            .map(|manifest| (*manifest).clone())
    };
    let on_disk = || pnpm_workspace::safe_read_project_manifest_only(injected.dir).ok().flatten();
    let publishing_project = || manifest_of_project_publishing_from(injected);
    if injected.declared_as_file {
        project_manifest().or_else(on_disk).or_else(publishing_project)
    } else {
        project_manifest().or_else(publishing_project).or_else(on_disk)
    }
}

/// The manifest of the workspace project whose `publishConfig.directory` is
/// `injected.dir`: the project's entry in `manifests_by_dir` when it has one,
/// else its `package.json`. Only projects between `injected.dir` and the
/// workspace root are considered.
fn manifest_of_project_publishing_from(
    injected: &InjectedDirectory<'_>,
) -> Option<PackageManifest> {
    let target = pnpm_fs::lexical_normalize(injected.dir);
    if !target.starts_with(pnpm_fs::lexical_normalize(injected.workspace_root)) {
        return None;
    }
    let mut candidate = injected.dir.parent()?;
    loop {
        let manifest = injected.manifests_by_dir
            .get(&pnpm_fs::lexical_normalize(candidate))
            .map(|manifest| (*manifest).clone())
            .or_else(|| pnpm_workspace::safe_read_project_manifest_only(candidate).ok().flatten());
        if let Some(manifest) = manifest
            && pnpm_fs::lexical_normalize(&publish_source_dir(candidate, Some(manifest.value())))
                == target
        {
            return Some(manifest);
        }
        if candidate == injected.workspace_root {
            return None;
        }
        candidate = candidate.parent()?;
    }
}

fn check_single_directory_dep_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    importer: &pnpm_lockfile::ProjectSnapshot,
    dep: &LocalDepContext<'_>,
    (snapshot, pkg_meta): (Option<&pnpm_lockfile::SnapshotEntry>, &pnpm_lockfile::PackageMetadata),
) -> Result<(), FreshnessCheckError> {
    let local_manifest = read_and_override_manifest(check, dep)?;
    let Some(snapshot) = snapshot else {
        return Err(dep.outdated());
    };
    // The importer's own dependencies are the scope the resolver shadows
    // the directory dependency's peers in.
    let peers = PeerShadowing::of(
        &local_manifest,
        |name| importer_declares(importer, name),
        check.config.auto_install_peers,
    );
    check_local_dep_children_freshness(check, dep, &local_manifest, snapshot, &peers.shadowed)?;
    peers.check_local_peer_deps_freshness(dep, &local_manifest, pkg_meta)
}

fn importer_declares(importer: &pnpm_lockfile::ProjectSnapshot, name: &str) -> bool {
    let Ok(name) = pnpm_lockfile::PkgName::parse(name) else { return false };
    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional]
        .into_iter()
        .filter_map(|group| importer.get_map_by_group(group))
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
    }
    Ok(())
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
    let recorded_in = if child.optional { RecordedDeps::Optional } else { RecordedDeps::Required };
    match recorded_child(snapshot, name, recorded_in) {
        Some(recorded) => {
            spec_satisfies_snapshot_dep(&dep.dirs, dep.dir, name, &child.bare_specifier, recorded)
        }
        None => child.optional && check.optional_exclusions.allow_unresolved,
    }
}

/// Which of a snapshot's two dependency maps a child is recorded in.
#[derive(Clone, Copy)]
enum RecordedDeps {
    Required,
    Optional,
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

fn optional_peer_names(manifest: &PackageManifest) -> impl Iterator<Item = &str> {
    manifest
        .value()
        .get("peerDependenciesMeta")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|meta| meta.iter())
        .filter_map(|(name, entry)| {
            (entry.get("optional").and_then(serde_json::Value::as_bool) == Some(true)).then_some(
                name.as_str(),
            )
        })
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
        .chain(optional_peer_names(local_manifest))
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
