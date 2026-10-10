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

fn is_declared_local_directory(spec: &str) -> bool {
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
            rel_path: &dir_res.directory,
            dir: &local_dep_dir,
            dirs: SpecDirs {
                workspace_root: check.config.workspace_dir.as_deref().unwrap_or(check.lockfile_dir),
                lockfile_dir: check.lockfile_dir,
                manifests_by_dir: check.workspace.manifests_by_dir,
            },
            catalogs: check.workspace.catalogs,
        };
        check_single_directory_dep_freshness(check, &dep, snapshot, pkg_meta)?;
    }
    Ok(())
}

fn read_and_override_manifest(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
) -> Result<PackageManifest, FreshnessCheckError> {
    let mut local_manifest = check.workspace.manifests_by_dir
        .get(&pnpm_fs::lexical_normalize(dep.dir))
        .map(|manifest| (*manifest).clone())
        .or_else(|| pnpm_workspace::safe_read_project_manifest_only(dep.dir).ok().flatten())
        .or_else(|| workspace_manifest_for_unbuilt_publish_dir(dep))
        .ok_or_else(|| dep.outdated())?;
    if let Some(parsed) = check.parsed_overrides {
        crate::VersionsOverrider::new(parsed, check.lockfile_dir)
            .apply(&mut local_manifest, Some(dep.dir));
    }
    Ok(local_manifest)
}

/// A workspace project that publishes from `publishConfig.directory` is
/// injected as that directory rather than its root, so a fresh checkout (or
/// a clean before `--frozen-lockfile`) has nothing at `dep.dir` until the
/// project's own build script runs. Walk up from `dep.dir` toward the
/// workspace root looking for the project whose manifest resolves its
/// publish directory back to `dep.dir`, and read its manifest from there
/// instead of reporting the lockfile outdated over a directory the coming
/// install step is about to create.
fn workspace_manifest_for_unbuilt_publish_dir(
    dep: &LocalDepContext<'_>,
) -> Option<PackageManifest> {
    let target = pnpm_fs::lexical_normalize(dep.dir);
    let mut candidate = dep.dir.parent()?;
    loop {
        if let Some(manifest) =
            pnpm_workspace::safe_read_project_manifest_only(candidate).ok().flatten()
            && pnpm_fs::lexical_normalize(&publish_source_dir(candidate, Some(manifest.value())))
                == target
        {
            return Some(manifest);
        }
        if candidate == dep.dirs.workspace_root {
            return None;
        }
        candidate = candidate.parent()?;
    }
}

fn check_single_directory_dep_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
    snapshot: Option<&pnpm_lockfile::SnapshotEntry>,
    pkg_meta: &pnpm_lockfile::PackageMetadata,
) -> Result<(), FreshnessCheckError> {
    let local_manifest = read_and_override_manifest(check, dep)?;
    let Some(snapshot) = snapshot else {
        return Err(dep.outdated());
    };
    let peers = PeerShadowing::of(&local_manifest, pkg_meta, check.config.auto_install_peers);
    check_local_dep_group_freshness(
        dep,
        &local_manifest,
        DependencyGroup::Prod,
        snapshot.dependencies.as_ref(),
        (false, &peers.shadowed),
    )?;
    if check.config.optional {
        check_local_dep_group_freshness(
            dep,
            &local_manifest,
            DependencyGroup::Optional,
            snapshot.optional_dependencies.as_ref(),
            (check.optional_exclusions.allow_unresolved, &HashSet::new()),
        )?;
    }
    peers.check_local_peer_deps_freshness(dep, &local_manifest, pkg_meta)
}

fn check_local_dep_group_freshness(
    dep: &LocalDepContext<'_>,
    local_manifest: &PackageManifest,
    group: DependencyGroup,
    snapshot_deps: Option<
        &std::collections::HashMap<pnpm_lockfile::PkgName, pnpm_lockfile::SnapshotDepRef>,
    >,
    (allow_unresolved, shadowed): (bool, &HashSet<&str>),
) -> Result<(), FreshnessCheckError> {
    let mut manifest_deps: std::collections::HashMap<&str, &str> = local_manifest
        .dependencies([group])
        .collect();
    if let Some(snapshot_deps) = snapshot_deps {
        let mut valid_keys = manifest_deps.clone();
        if group == DependencyGroup::Prod {
            valid_keys.extend(local_manifest.dependencies([DependencyGroup::Peer]));
        }
        if group == DependencyGroup::Optional {
            valid_keys.extend(optional_peer_names(local_manifest).map(|name| (name, "*")));
        }
        check_snapshot_keys_in_manifest(dep, &valid_keys, snapshot_deps)?;
    }
    // The snapshot records the peer's resolution for these, whose range the
    // peer check compares.
    manifest_deps.retain(|name, _| !shadowed.contains(name));
    check_manifest_specs_satisfy_snapshot(dep, &manifest_deps, snapshot_deps, allow_unresolved)
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

fn check_snapshot_keys_in_manifest(
    dep: &LocalDepContext<'_>,
    manifest_deps: &std::collections::HashMap<&str, &str>,
    snapshot_deps: &std::collections::HashMap<
        pnpm_lockfile::PkgName,
        pnpm_lockfile::SnapshotDepRef,
    >,
) -> Result<(), FreshnessCheckError> {
    for lockfile_dep_name in snapshot_deps.keys() {
        let lockfile_name_str = lockfile_dep_name.to_string();
        if !manifest_deps.contains_key(lockfile_name_str.as_str()) {
            return Err(dep.outdated());
        }
    }
    Ok(())
}

fn check_manifest_specs_satisfy_snapshot(
    dep: &LocalDepContext<'_>,
    manifest_deps: &std::collections::HashMap<&str, &str>,
    snapshot_deps: Option<
        &std::collections::HashMap<pnpm_lockfile::PkgName, pnpm_lockfile::SnapshotDepRef>,
    >,
    allow_unresolved: bool,
) -> Result<(), FreshnessCheckError> {
    for (name, spec) in manifest_deps {
        let lockfile_dep = snapshot_deps.and_then(|deps| {
            pnpm_lockfile::PkgName::parse(*name)
                .ok()
                .and_then(|n| deps.get(&n))
        });
        let Some(lockfile_dep) = lockfile_dep else {
            if allow_unresolved {
                continue;
            }
            return Err(dep.outdated());
        };
        if !spec_satisfies_snapshot_dep(&dep.dirs, dep.dir, name, spec, lockfile_dep) {
            return Err(dep.outdated());
        }
    }
    Ok(())
}
