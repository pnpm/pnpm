//! The manifests a workspace project with its own lockfile injects, as the
//! resolver read them.

use super::{
    FreshnessCheckError, LockfileFreshnessInputs,
    directory_deps::{
        InjectedDirectory, is_declared_local_directory, is_deploy_file_url_key, unhooked_manifest,
    },
    project_manifests_by_dir,
};
use crate::install::{Arc, Lockfile, PackageManifest, PathBuf};
use pnpm_package_manifest::DependencyGroup;
use std::collections::HashSet;

/// The manifests of the dependencies `lockfile` records as injected into
/// the importers of `inputs`, keyed by their directory, with
/// `packageExtensions` and the pnpmfile's `readPackage` applied as the
/// resolver applied them.
///
/// Empty unless the importer is a workspace project with its own lockfile.
/// Every other install holds the injected projects among its own, already
/// hooked; this one holds only itself.
pub(super) async fn hooked_injected_manifests(
    lockfile: &Lockfile,
    inputs: &LockfileFreshnessInputs<'_, '_>,
) -> Result<Vec<(PathBuf, PackageManifest)>, FreshnessCheckError> {
    let config = inputs.config;
    if config.shares_one_lockfile()
        || (inputs.pnpmfile_hook.is_none() && config.package_extensions.is_none())
    {
        return Ok(Vec::new());
    }
    let mut injected = unhooked_injected_manifests(lockfile, inputs);
    if let Some(extensions) = config.package_extensions.as_ref() {
        let extender = crate::PackageExtender::new(extensions)
            .map_err(FreshnessCheckError::InvalidPackageExtensionSelector)?;
        for manifest in &mut injected {
            extender.apply(manifest.manifest.value_mut());
        }
    }
    let Some(hook) = inputs.pnpmfile_hook else {
        return Ok(injected
            .into_iter()
            .map(|injected| (injected.dir, injected.manifest))
            .collect());
    };
    futures_util::future::try_join_all(
        injected
            .into_iter()
            .map(|injected| async move {
                // The resolver hands the hook the directory the lockfile records.
                let ctx = pnpm_hooks::HookContext {
                    log: Arc::new(|_| {}),
                    dir: Some(injected.recorded_directory),
                };
                let hooked = hook
                    .read_package(injected.manifest.value().clone(), ctx)
                    .await
                    .map_err(FreshnessCheckError::ReadPackageHook)?;
                let mut manifest = injected.manifest;
                *manifest.value_mut() = (*hooked).clone();
                Ok((injected.dir, manifest))
            }),
    )
    .await
}

struct InjectedManifest {
    dir: PathBuf,
    recorded_directory: String,
    manifest: PackageManifest,
}

fn unhooked_injected_manifests(
    lockfile: &Lockfile,
    inputs: &LockfileFreshnessInputs<'_, '_>,
) -> Vec<InjectedManifest> {
    let manifests_by_dir = project_manifests_by_dir(
        inputs.manifests.iter().map(|(_, manifest)| *manifest),
        inputs.dependency_manifests,
    );
    let workspace_root = inputs.config.workspace_dir.as_deref().unwrap_or(inputs.lockfile_dir);
    let mut manifests: Vec<InjectedManifest> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
    let dependencies = inputs.manifests
        .iter()
        .flat_map(|(importer_id, _)| lockfile.injected_dependencies(importer_id, groups))
        .filter(|dep| !is_deploy_injected_dependency(dep));
    for dependency in dependencies {
        let dir =
            pnpm_fs::lexical_normalize(&inputs.lockfile_dir.join(&dependency.directory.directory));
        if manifests_by_dir.contains_key(&dir) || !seen.insert(dir.clone()) {
            continue;
        }
        let Some(manifest) = unhooked_manifest(&InjectedDirectory {
            dir: &dir,
            declared_as_file: is_declared_local_directory(&dependency.spec.specifier),
            workspace_root,
            manifests_by_dir: &manifests_by_dir,
        }) else {
            continue;
        };
        manifests.push(InjectedManifest {
            dir,
            recorded_directory: dependency.directory.directory.clone(),
            manifest,
        });
    }
    manifests
}

fn is_deploy_injected_dependency(dependency: &pnpm_lockfile::InjectedDependency<'_>) -> bool {
    dependency.spec.version
        .resolved_key(dependency.alias)
        .is_some_and(|key| {
            is_deploy_file_url_key(dependency.spec, &key, &dependency.directory.directory)
        })
}
