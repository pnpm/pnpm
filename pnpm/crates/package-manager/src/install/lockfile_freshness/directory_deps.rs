mod spec;

use super::manifest::ImporterSatisfactionCheck;
use crate::install::lockfile_freshness::FreshnessCheckError;
use pnpm_injected_deps_syncer::publish_source_dir;
use pnpm_lockfile::StalenessReason;
use pnpm_package_manifest::{DependencyGroup, ManifestFormat, PackageManifest};
use spec::spec_satisfies_snapshot_dep;
use std::path::Path;

struct LocalDepContext<'a> {
    name: &'a str,
    rel_path: &'a str,
    dir: &'a Path,
    lockfile_dir: &'a Path,
    workspace_root: &'a Path,
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
            lockfile_dir: check.lockfile_dir,
            workspace_root: check.config.workspace_dir.as_deref().unwrap_or(check.lockfile_dir),
        };
        check_single_directory_dep_freshness(check, &dep, snapshot, pkg_meta)?;
    }
    Ok(())
}

fn read_and_override_manifest(
    check: &ImporterSatisfactionCheck<'_>,
    dep: &LocalDepContext<'_>,
) -> Result<PackageManifest, FreshnessCheckError> {
    // Directory dependencies resolve from their `package.json`, which the
    // default order selects first.
    let mut local_manifest =
        pnpm_workspace::safe_read_project_manifest_only(dep.dir, ManifestFormat::default())
            .ok()
            .flatten()
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
            pnpm_workspace::safe_read_project_manifest_only(candidate, ManifestFormat::default())
                .ok()
                .flatten()
            && pnpm_fs::lexical_normalize(&publish_source_dir(candidate, Some(manifest.value())))
                == target
        {
            return Some(manifest);
        }
        if candidate == dep.workspace_root {
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
    check_local_dep_group_freshness(
        dep,
        &local_manifest,
        DependencyGroup::Prod,
        snapshot.dependencies.as_ref(),
        false,
    )?;
    if check.config.optional {
        check_local_dep_group_freshness(
            dep,
            &local_manifest,
            DependencyGroup::Optional,
            snapshot.optional_dependencies.as_ref(),
            check.optional_exclusions.allow_unresolved,
        )?;
    }
    check_local_peer_deps_freshness(dep, &local_manifest, pkg_meta, snapshot.dependencies.as_ref())
}

fn check_local_peer_deps_freshness(
    dep: &LocalDepContext<'_>,
    local_manifest: &PackageManifest,
    pkg_meta: &pnpm_lockfile::PackageMetadata,
    snapshot_deps: Option<
        &std::collections::HashMap<pnpm_lockfile::PkgName, pnpm_lockfile::SnapshotDepRef>,
    >,
) -> Result<(), FreshnessCheckError> {
    let manifest_peers: std::collections::HashMap<&str, &str> = local_manifest
        .dependencies([DependencyGroup::Peer])
        .collect();

    check_recorded_peer_specs_match(dep, &manifest_peers, pkg_meta)?;
    check_peer_dependencies_meta_freshness(dep, local_manifest, pkg_meta)?;

    for (name, spec) in &manifest_peers {
        let lockfile_dep = snapshot_deps.and_then(|deps| {
            pnpm_lockfile::PkgName::parse(*name)
                .ok()
                .and_then(|n| deps.get(&n))
        });
        if let Some(lockfile_dep) = lockfile_dep
            && !spec_satisfies_snapshot_dep(
                dep.workspace_root,
                dep.lockfile_dir,
                dep.dir,
                name,
                spec,
                lockfile_dep,
            )
        {
            return Err(dep.outdated());
        }
    }

    Ok(())
}

fn check_recorded_peer_specs_match(
    dep: &LocalDepContext<'_>,
    manifest_peers: &std::collections::HashMap<&str, &str>,
    pkg_meta: &pnpm_lockfile::PackageMetadata,
) -> Result<(), FreshnessCheckError> {
    let recorded_count =
        pkg_meta.peer_dependencies.as_ref().map_or(0, std::collections::HashMap::len);
    if manifest_peers.len() != recorded_count {
        return Err(dep.outdated());
    }
    for (name, spec) in manifest_peers {
        let recorded_spec = pkg_meta.peer_dependencies
            .as_ref()
            .and_then(|p| p.get(*name));
        if recorded_spec.map(String::as_str) != Some(spec) {
            return Err(dep.outdated());
        }
    }
    Ok(())
}

fn check_peer_dependencies_meta_freshness(
    dep: &LocalDepContext<'_>,
    local_manifest: &PackageManifest,
    pkg_meta: &pnpm_lockfile::PackageMetadata,
) -> Result<(), FreshnessCheckError> {
    let manifest_meta = local_manifest
        .value()
        .get("peerDependenciesMeta")
        .and_then(serde_json::Value::as_object);
    let recorded_meta = pkg_meta.peer_dependencies_meta.as_ref();
    let manifest_optional_count = manifest_meta.map_or(0, |meta| {
        meta.values()
            .filter(|entry| {
                entry.get("optional").and_then(serde_json::Value::as_bool) == Some(true)
            })
            .count()
    });
    let recorded_optional_count = recorded_meta.map_or(0, |meta| {
        meta.values()
            .filter(|m| m.optional)
            .count()
    });
    if manifest_optional_count != recorded_optional_count {
        return Err(dep.outdated());
    }
    if let Some(recorded) = recorded_meta {
        for (name, meta) in recorded {
            let manifest_optional = manifest_meta
                .and_then(|m| m.get(name))
                .and_then(|entry| entry.get("optional"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if meta.optional != manifest_optional {
                return Err(dep.outdated());
            }
        }
    }
    Ok(())
}

fn check_local_dep_group_freshness(
    dep: &LocalDepContext<'_>,
    local_manifest: &PackageManifest,
    group: DependencyGroup,
    snapshot_deps: Option<
        &std::collections::HashMap<pnpm_lockfile::PkgName, pnpm_lockfile::SnapshotDepRef>,
    >,
    allow_unresolved: bool,
) -> Result<(), FreshnessCheckError> {
    let manifest_deps: std::collections::HashMap<&str, &str> = local_manifest
        .dependencies([group])
        .collect();
    if let Some(snapshot_deps) = snapshot_deps {
        let valid_keys: std::collections::HashMap<&str, &str> = if group == DependencyGroup::Prod {
            local_manifest
                .dependencies([DependencyGroup::Prod, DependencyGroup::Peer])
                .collect()
        } else {
            manifest_deps.clone()
        };
        check_snapshot_keys_in_manifest(dep, &valid_keys, snapshot_deps)?;
    }
    check_manifest_specs_satisfy_snapshot(dep, &manifest_deps, snapshot_deps, allow_unresolved)
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
        if !spec_satisfies_snapshot_dep(
            dep.workspace_root,
            dep.lockfile_dir,
            dep.dir,
            name,
            spec,
            lockfile_dep,
        ) {
            return Err(dep.outdated());
        }
    }
    Ok(())
}
