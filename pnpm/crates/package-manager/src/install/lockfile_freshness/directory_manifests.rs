//! The manifests of a lockfile's directory packages, as the resolver read
//! them.

use super::{
    FreshnessCheckError, LockfileFreshnessInputs,
    directory_deps::{InjectedDirectory, is_declared_local_directory, unhooked_manifest},
    project_manifests_by_dir,
};
use crate::install::{Arc, Lockfile, PackageManifest, Path, PathBuf};
use pnpm_package_manifest::DependencyGroup;
use std::collections::HashMap;

/// The manifests [`hooked_directory_manifests`] returns, by lexically
/// normalized directory and by whether a `file:` spec declared the package.
/// The two declarations can read different manifests; see
/// [`unhooked_manifest`].
#[derive(Default)]
pub(crate) struct DirectoryManifests(HashMap<(PathBuf, bool), PackageManifest>);

impl DirectoryManifests {
    pub(crate) fn get(&self, dir: &Path, declared_as_file: bool) -> Option<&PackageManifest> {
        self.0.get(&(pnpm_fs::lexical_normalize(dir), declared_as_file))
    }
}

/// The manifest of every directory package `lockfile` records that no
/// project in `inputs` describes, with the resolver's manifest hooks applied
/// as the resolver applies them: `packageExtensions`, the pnpmfile's
/// `readPackage` with the recorded directory, overrides, and
/// `ignoredOptionalDependencies`.
pub(super) async fn hooked_directory_manifests(
    lockfile: &Lockfile,
    inputs: &LockfileFreshnessInputs<'_, '_>,
    parsed_overrides: Option<&[pnpm_config_parse_overrides::VersionOverride]>,
) -> Result<DirectoryManifests, FreshnessCheckError> {
    let unhooked = unhooked_directory_manifests(lockfile, inputs);
    if unhooked.is_empty() {
        return Ok(DirectoryManifests::default());
    }
    let versions_overrider = parsed_overrides.map(|parsed| {
        Arc::new(crate::VersionsOverrider::new(parsed, inputs.lockfile_dir))
    });
    let hooks = crate::install_with_fresh_lockfile::resolver_manifest_hooks(
        inputs.config,
        versions_overrider,
    )
    .map_err(FreshnessCheckError::InvalidPackageExtensionSelector)?
    .with_pnpmfile(inputs.pnpmfile_hook.cloned());
    let hooked = futures_util::future::try_join_all(
        unhooked
            .into_iter()
            .map(|unhooked| {
                let hooks = &hooks;
                async move {
                    let hook_ctx = || pnpm_hooks::HookContext {
                        log: Arc::new(|_| {}),
                        dir: Some(unhooked.recorded_directory),
                    };
                    let value = hooks
                        .apply(Arc::new(unhooked.manifest.value().clone()), hook_ctx)
                        .await
                        .map_err(FreshnessCheckError::ReadPackageHook)?;
                    let mut manifest = unhooked.manifest;
                    *manifest.value_mut() = (*value).clone();
                    Ok::<_, FreshnessCheckError>((unhooked.dir, unhooked.declarations, manifest))
                }
            }),
    )
    .await?;
    let mut manifests = HashMap::new();
    for (dir, declarations, manifest) in hooked {
        for declared_as_file in declarations {
            manifests.insert((dir.clone(), declared_as_file), manifest.clone());
        }
    }
    Ok(DirectoryManifests(manifests))
}

/// The manifests a project with its own lockfile injects: the workspace
/// projects it does not hold among its own importers. Every other install
/// holds them, already hooked.
pub(super) fn injected_project_manifests<'a>(
    lockfile: &Lockfile,
    inputs: &LockfileFreshnessInputs<'_, '_>,
    directory_manifests: &'a DirectoryManifests,
) -> Vec<(PathBuf, &'a PackageManifest)> {
    if inputs.config.shares_one_lockfile() {
        return Vec::new();
    }
    let groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
    inputs.manifests
        .iter()
        .flat_map(|(importer_id, _)| lockfile.injected_dependencies(importer_id, groups))
        .filter_map(|dependency| {
            let dir = pnpm_fs::lexical_normalize(&inputs.lockfile_dir.join(
                &dependency.directory.directory,
            ));
            let declared_as_file = is_declared_local_directory(&dependency.spec.specifier);
            let manifest = directory_manifests.get(&dir, declared_as_file)?;
            Some((dir, manifest))
        })
        .collect()
}

struct UnhookedManifest {
    dir: PathBuf,
    /// The values of `declared_as_file` that read this manifest.
    declarations: Vec<bool>,
    recorded_directory: String,
    manifest: PackageManifest,
}

/// Each directory package's manifest under each declaration, once when both
/// read the same one, except a
/// `pnpm deploy` entry, whose snapshot already carries its hooks.
fn unhooked_directory_manifests(
    lockfile: &Lockfile,
    inputs: &LockfileFreshnessInputs<'_, '_>,
) -> Vec<UnhookedManifest> {
    let manifests_by_dir = project_manifests_by_dir(
        inputs.manifests.iter().map(|(_, manifest)| *manifest),
        inputs.dependency_manifests,
    );
    let workspace_root = inputs.config.workspace_dir.as_deref().unwrap_or(inputs.lockfile_dir);
    let mut directories: Vec<&str> = lockfile.packages
        .iter()
        .flatten()
        .filter_map(|(key, metadata)| match &metadata.resolution {
            pnpm_lockfile::LockfileResolution::Directory(directory)
                if !super::directory_deps::is_deploy_file_url_package_key(
                    key,
                    &directory.directory,
                ) =>
            {
                Some(directory.directory.as_str())
            }
            _ => None,
        })
        .collect();
    directories.sort_unstable();
    directories.dedup();
    let mut manifests = Vec::new();
    for recorded_directory in directories {
        let dir = pnpm_fs::lexical_normalize(&inputs.lockfile_dir.join(recorded_directory));
        if manifests_by_dir.contains_key(&dir) {
            continue;
        }
        let read = |declared_as_file| {
            unhooked_manifest(&InjectedDirectory {
                dir: &dir,
                declared_as_file,
                workspace_root,
                manifests_by_dir: &manifests_by_dir,
            })
        };
        let (as_file, as_workspace) = (read(true), read(false));
        let variants = match (as_file, as_workspace) {
            (Some(as_file), Some(as_workspace)) if as_file.value() == as_workspace.value() => {
                vec![(vec![true, false], as_file)]
            }
            (as_file, as_workspace) => as_file
                .map(|manifest| (vec![true], manifest))
                .into_iter()
                .chain(as_workspace.map(|manifest| (vec![false], manifest)))
                .collect(),
        };
        for (declarations, manifest) in variants {
            manifests.push(UnhookedManifest {
                dir: dir.clone(),
                declarations,
                recorded_directory: recorded_directory.to_string(),
                manifest,
            });
        }
    }
    manifests
}
