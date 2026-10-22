//! The directory dependencies an importer declares: which of its specs name
//! one, and the lockfile entry each resolves to.

use super::{
    FreshnessCheckError, ImporterSatisfactionCheck, LocalDepContext, is_declared_local_directory,
    is_deploy_file_url_key, local_dependency_outdated,
    package::check_single_directory_dep_freshness,
    scope::{Visited, importer_aliases},
    spec, spec_dirs,
};
use pnpm_package_manifest::DependencyGroup;

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
