use super::manifest::ImporterSatisfactionCheck;
use crate::install::lockfile_freshness::FreshnessCheckError;
use pnpm_lockfile::StalenessReason;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use std::path::Path;

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

fn is_declared_local_directory(importer: &pnpm_lockfile::ProjectSnapshot, dep_name: &str) -> bool {
    let Some(specifiers) = importer.specifiers.as_ref() else {
        return false;
    };
    let Some(spec) = specifiers.get(dep_name) else {
        return false;
    };
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
        && !is_declared_local_directory(importer, &dep_str)
    {
        return Ok(());
    }
    let Some(pkg_key) = dep_spec.version.resolved_key(dep_name) else {
        return Ok(());
    };
    let Some(pkg_meta) = check.lockfile.packages
        .as_ref()
        .and_then(|p| p.get(&pkg_key))
    else {
        return Ok(());
    };
    if let pnpm_lockfile::LockfileResolution::Directory(dir_res) = &pkg_meta.resolution {
        let local_dep_dir = check.lockfile_dir.join(&dir_res.directory);
        let snapshot = check.lockfile.snapshots
            .as_ref()
            .and_then(|s| s.get(&pkg_key));
        check_single_directory_dep_freshness(
            check,
            &dep_name.to_string(),
            &dir_res.directory,
            &local_dep_dir,
            snapshot,
        )?;
    }
    Ok(())
}

fn check_single_directory_dep_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    dep_name: &str,
    rel_path: &str,
    local_dep_dir: &Path,
    snapshot: Option<&pnpm_lockfile::SnapshotEntry>,
) -> Result<(), FreshnessCheckError> {
    let mut local_manifest = pnpm_workspace::safe_read_project_manifest_only(local_dep_dir)
        .ok()
        .flatten()
        .ok_or_else(|| {
            FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated {
                name: dep_name.to_string(),
                path: rel_path.to_string(),
            })
        })?;
    if let Some(parsed) = check.parsed_overrides {
        crate::VersionsOverrider::new(parsed, check.lockfile_dir)
            .apply(&mut local_manifest, Some(local_dep_dir));
    }
    let Some(snapshot) = snapshot else {
        return Err(FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated {
            name: dep_name.to_string(),
            path: rel_path.to_string(),
        }));
    };
    check_local_dep_group_freshness(
        dep_name,
        rel_path,
        &local_manifest,
        DependencyGroup::Prod,
        snapshot.dependencies.as_ref(),
        false,
    )?;
    if check.config.optional {
        check_local_dep_group_freshness(
            dep_name,
            rel_path,
            &local_manifest,
            DependencyGroup::Optional,
            snapshot.optional_dependencies.as_ref(),
            check.optional_exclusions.allow_unresolved,
        )?;
    }
    Ok(())
}

fn check_local_dep_group_freshness(
    dep_name: &str,
    rel_path: &str,
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
        check_snapshot_keys_in_manifest(dep_name, rel_path, &valid_keys, snapshot_deps)?;
    }
    check_manifest_specs_satisfy_snapshot(
        dep_name,
        rel_path,
        &manifest_deps,
        snapshot_deps,
        allow_unresolved,
    )
}

fn check_snapshot_keys_in_manifest(
    dep_name: &str,
    rel_path: &str,
    manifest_deps: &std::collections::HashMap<&str, &str>,
    snapshot_deps: &std::collections::HashMap<
        pnpm_lockfile::PkgName,
        pnpm_lockfile::SnapshotDepRef,
    >,
) -> Result<(), FreshnessCheckError> {
    for lockfile_dep_name in snapshot_deps.keys() {
        let lockfile_name_str = lockfile_dep_name.to_string();
        if !manifest_deps.contains_key(lockfile_name_str.as_str()) {
            return Err(FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated {
                name: dep_name.to_string(),
                path: rel_path.to_string(),
            }));
        }
    }
    Ok(())
}

fn check_manifest_specs_satisfy_snapshot(
    dep_name: &str,
    rel_path: &str,
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
            return Err(FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated {
                name: dep_name.to_string(),
                path: rel_path.to_string(),
            }));
        };
        if !spec_satisfies_snapshot_dep(spec, lockfile_dep) {
            return Err(FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated {
                name: dep_name.to_string(),
                path: rel_path.to_string(),
            }));
        }
    }
    Ok(())
}

fn spec_satisfies_snapshot_dep(spec: &str, lockfile_dep: &pnpm_lockfile::SnapshotDepRef) -> bool {
    if spec.starts_with("file:") || spec.starts_with("link:") || spec.starts_with("workspace:") {
        return true;
    }
    let clean_spec = if let Some(stripped) = spec.strip_prefix("npm:") {
        stripped
            .rfind('@')
            .map_or(stripped, |idx| &stripped[idx + 1..])
    } else {
        spec
    };
    let Ok(range) = clean_spec.parse::<node_semver::Range>() else {
        return true;
    };
    let Some(version) = lockfile_dep.ver_peer().and_then(|v| v.version_semver()) else {
        return true;
    };
    range.satisfies(version)
}
