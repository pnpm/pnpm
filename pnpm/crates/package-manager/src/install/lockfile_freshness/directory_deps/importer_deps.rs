//! The directory dependencies an importer declares: which of its specs name
//! one, and the lockfile entry each resolves to.

use super::{
    FreshnessCheckError, ImporterSatisfactionCheck, LocalDepContext, is_declared_by_path,
    is_declared_local_directory, is_deploy_file_url_key, local_dependency_outdated,
    package::check_single_directory_dep_freshness,
    scope::{Visited, importer_aliases},
    spec, spec_dirs,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_deps_resolver::{
    dependency_is_injected, dependency_meta_is_injected, directory_package_dir,
};

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

/// Whether the config, the lockfile, or the manifest injects the dependency.
/// The lockfile's settings count too: the resolver injected under them.
fn may_be_injected(
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
    importer.dependencies_meta
        .as_ref()
        .and_then(|meta| meta.get(dep_name))
        .is_some_and(dependency_meta_is_injected)
        || dependency_is_injected(check.manifest.value(), dep_name)
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
    let satisfied =
        spec::importer_link_satisfies(&dirs, project_dir, dep_name, specifier, link_target)
            && (!check.config.link_workspace_packages.enabled_at_depth(0)
                || spec::importer_range_link_satisfies(
                    &dirs,
                    project_dir,
                    dep_name,
                    specifier,
                    link_target,
                ));
    if !satisfied {
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
        && (may_be_injected(check, importer, dep_name) || is_declared_local_directory(specifier))
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
    let local_dep_dir = directory_package_dir(check.lockfile_dir, rel_path);
    let dep = LocalDepContext {
        name: dep_str,
        declared_by_path: is_declared_by_path(&dep_spec.specifier),
        declaration_assumed: false,
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
