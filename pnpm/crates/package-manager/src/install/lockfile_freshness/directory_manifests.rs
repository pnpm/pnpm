//! The manifests of the directory packages a lockfile check reads, as the
//! resolver read them.

use super::{
    FreshnessCheckError, LockfileFreshnessInputs,
    directory_deps::{
        GraphEdges, NestedByImporter, NestedDirectoryPackage, is_declared_by_path,
        is_deploy_file_url_key, nested_directory_packages as nested_directory_packages_along,
    },
    project_manifests_by_dir,
};
use crate::install::{Arc, Lockfile, PackageManifest, PathBuf};
use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_deps_resolver::{ChildCatalogs, ManifestTransformHooks, directory_package_dir};
use std::collections::{HashMap, HashSet};

mod read;

/// A directory dependency as the check reaches it: the directory the
/// lockfile records, as written, and whether a path spec declared it
/// ([`is_declared_by_path`]).
type DirectoryDep = (String, bool);

/// The manifests [`hooked_directory_manifests`] returns, by [`DirectoryDep`].
#[derive(Default)]
pub(crate) struct DirectoryManifests(HashMap<DirectoryDep, PackageManifest>);

impl DirectoryManifests {
    /// No manifests: every directory package is read from its directory.
    #[cfg(test)]
    pub(crate) fn none() -> &'static Self {
        static NONE: std::sync::OnceLock<DirectoryManifests> = std::sync::OnceLock::new();
        NONE.get_or_init(Self::default)
    }

    pub(crate) fn get(&self, directory: &str, declared_by_path: bool) -> Option<&PackageManifest> {
        self.0.get(&(directory.to_string(), declared_by_path))
    }
}

/// The manifest of every directory package the importers of `inputs` reach in
/// `lockfile`, with the resolver's manifest hooks applied, for a check that
/// compares those importers outside [`super::check_lockfile_freshness`].
pub(crate) async fn hooked_importer_directory_manifests(
    lockfile: &Lockfile,
    inputs: &LockfileFreshnessInputs<'_, '_>,
    overrider: Option<&Arc<crate::VersionsOverrider>>,
) -> Result<DirectoryManifests, FreshnessCheckError> {
    let nested_packages = nested_directory_packages(lockfile, inputs);
    let nested = checked_nested(&nested_packages, inputs);
    hooked_directory_manifests(lockfile, inputs, (overrider, &nested)).await
}

pub(super) fn nested_directory_packages(
    lockfile: &Lockfile,
    inputs: &LockfileFreshnessInputs<'_, '_>,
) -> Vec<NestedDirectoryPackage> {
    nested_directory_packages_along(lockfile, GraphEdges::installed(inputs.config.optional))
}

/// The nested directory packages of the importers `inputs` checks.
pub(super) fn checked_nested<'n>(
    nested: &'n [NestedDirectoryPackage],
    inputs: &LockfileFreshnessInputs<'_, '_>,
) -> NestedByImporter<'n> {
    let checked: std::collections::HashSet<&str> = inputs.manifests
        .iter()
        .map(|(id, _)| id.as_str())
        .collect();
    NestedByImporter::assign(nested, |importer_id| checked.contains(importer_id))
}

/// The manifest of every directory package the importers of `inputs` reach
/// through directory packages, and of every `nested` one they reach and the directory
/// packages beneath it, with the resolver's manifest hooks applied as the resolver applies them:
/// `packageExtensions`, the pnpmfile's `readPackage` with the recorded
/// directory, overrides, and `ignoredOptionalDependencies`. Projects are included
/// when `addMissingPeerTypes` requires their dependency hook chain.
///
/// The children of a package come from its hooked manifest, so the packages
/// are hooked one level of the graph at a time.
pub(super) async fn hooked_directory_manifests(
    lockfile: &Lockfile,
    inputs: &LockfileFreshnessInputs<'_, '_>,
    (overrider, nested): (Option<&Arc<crate::VersionsOverrider>>, &NestedByImporter<'_>),
) -> Result<DirectoryManifests, FreshnessCheckError> {
    let mut level = direct_directory_deps(lockfile, inputs);
    level.extend(nested_directory_deps(nested));
    if level.is_empty() {
        return Ok(DirectoryManifests::default());
    }
    let walk = DirectoryWalk {
        lockfile,
        inputs,
        overrider: overrider.map(Arc::as_ref),
        manifests_by_dir: dependency_manifest_sources(inputs),
        declared_manifests_by_dir: lockfile.packages
            .as_ref()
            .filter(|packages| {
                packages.values().any(|metadata| metadata.specifiers.is_some())
            })
            .map(|_| {
                project_manifests_by_dir(
                    inputs.dependency_manifests.declared.iter().map(|(_, manifest)| *manifest),
                    inputs.dependency_manifests.injected,
                )
            }),
        hooks: resolver_hooks(inputs, overrider.cloned())?,
    };
    walk.read_all(level).await
}

fn resolver_hooks(
    inputs: &LockfileFreshnessInputs<'_, '_>,
    versions_overrider: Option<Arc<crate::VersionsOverrider>>,
) -> Result<ManifestTransformHooks, FreshnessCheckError> {
    Ok(crate::install_with_fresh_lockfile::resolver_manifest_hooks(
        inputs.config,
        versions_overrider,
    )
    .map_err(FreshnessCheckError::InvalidPackageExtensionSelector)?
    .with_pnpmfile(inputs.pnpmfile_hook.cloned()))
}

/// The manifests a project with its own lockfile injects: the workspace
/// projects it does not hold among its own importers. Every other install
/// holds them, already hooked. Later entries replace earlier ones for the
/// same directory.
pub(super) fn injected_project_manifests<'a>(
    lockfile: &Lockfile,
    inputs: &LockfileFreshnessInputs<'_, '_>,
    directory_manifests: &'a DirectoryManifests,
) -> Vec<(PathBuf, &'a PackageManifest)> {
    if inputs.config.shares_one_lockfile() {
        return Vec::new();
    }
    let groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
    let mut injected: Vec<(bool, PathBuf, &PackageManifest)> = inputs.manifests
        .iter()
        .flat_map(|(importer_id, _)| lockfile.injected_dependencies(importer_id, groups))
        .filter_map(|dependency| {
            let directory = &dependency.directory.directory;
            let declared_by_path = is_declared_by_path(&dependency.spec.specifier);
            let manifest = directory_manifests.get(directory, declared_by_path)?;
            let dir = directory_package_dir(inputs.lockfile_dir, directory);
            Some((declared_by_path, dir, manifest))
        })
        .collect();
    // The map these extend is keyed by directory and serves workspace specs,
    // so a directory reached both ways keeps the workspace declaration's
    // manifest: it comes last.
    injected.sort_by_key(|(declared_by_path, ..)| !declared_by_path);
    injected
        .into_iter()
        .map(|(_, dir, manifest)| (dir, manifest))
        .collect()
}

/// A directory package the walk reads, and the lockfile entry that records
/// its children.
struct Visit {
    dep: DirectoryDep,
    alias: String,
    key: pnpm_lockfile::PackageKey,
}

/// The importers' own directory dependencies, except a `pnpm deploy` entry,
/// whose snapshot already carries its hooks.
fn direct_directory_deps(
    lockfile: &Lockfile,
    inputs: &LockfileFreshnessInputs<'_, '_>,
) -> Vec<Visit> {
    let groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
    inputs.manifests
        .iter()
        .flat_map(|(importer_id, _)| lockfile.injected_dependencies(importer_id, groups))
        .filter_map(|dependency| {
            let key = dependency.spec.version.resolved_key(dependency.alias)?;
            let directory = &dependency.directory.directory;
            (!is_deploy_file_url_key(dependency.spec, &key, directory)).then(|| Visit {
                dep: (directory.clone(), is_declared_by_path(&dependency.spec.specifier)),
                alias: dependency.alias.to_string(),
                key,
            })
        })
        .collect()
}

/// Each nested package under both declarations, which the check tries (see
/// [`super::directory_deps::check_nested_directory_package`]).
fn nested_directory_deps<'n>(nested: &'n NestedByImporter<'_>) -> impl Iterator<Item = Visit> + 'n {
    nested
        .packages()
        .flat_map(|package| {
            [false, true].map(|declared_by_path| Visit {
                dep: (package.directory.clone(), declared_by_path),
                alias: package.alias.clone(),
                key: package.key.clone(),
            })
        })
}

struct DirectoryWalk<'a, 'i> {
    lockfile: &'a Lockfile,
    inputs: &'a LockfileFreshnessInputs<'i, 'i>,
    overrider: Option<&'a crate::VersionsOverrider>,
    manifests_by_dir: super::ProjectManifestsByDir<'a>,
    declared_manifests_by_dir: Option<super::ProjectManifestsByDir<'a>>,
    hooks: ManifestTransformHooks,
}

impl DirectoryWalk<'_, '_> {
    /// [`Self::read`] each directory of `level` that `read` does not hold yet.
    async fn read_level(
        &self,
        level: &[Visit],
        read: &mut HashMap<DirectoryDep, Option<(PackageManifest, bool)>>,
    ) -> Result<(), FreshnessCheckError> {
        let mut queued: HashSet<&DirectoryDep> = HashSet::new();
        let unread: Vec<&Visit> = level
            .iter()
            .filter(|visit| !read.contains_key(&visit.dep) && queued.insert(&visit.dep))
            .collect();
        let manifests = futures_util::future::try_join_all(
            unread
                .iter()
                .map(|visit| self.read(visit)),
        )
        .await?;
        read.extend(
            unread
                .into_iter()
                .map(|visit| visit.dep.clone())
                .zip(manifests),
        );
        Ok(())
    }

    /// The children of `visit` the check walks into: the ones `manifest`
    /// declares that the snapshot records as directory packages.
    fn children(&self, visit: &Visit, manifest: &PackageManifest) -> Vec<Visit> {
        let Some(snapshot) = self.lockfile.snapshots
            .as_ref()
            .and_then(|s| s.get(&visit.key))
        else {
            return Vec::new();
        };
        if self.lockfile.packages
            .as_ref()
            .and_then(|packages| packages.get(&visit.key.without_peer()))
            .is_some_and(|metadata| metadata.specifiers.is_some())
        {
            return self.recorded_directory_children(snapshot);
        }
        let (directory, declared_by_path) = &visit.dep;
        let dir = directory_package_dir(self.inputs.lockfile_dir, directory);
        let catalogs = (!declared_by_path).then_some(ChildCatalogs {
            catalogs: self.inputs.catalogs,
            workspace_dir: self.inputs.config.workspace_dir.as_deref(),
            package_dir: &dir,
        });
        let Ok(children) = pnpm_resolving_deps_resolver::local_directory_child_specs(
            manifest.value(),
            &visit.alias,
            |_| false,
            catalogs,
        ) else {
            return Vec::new();
        };
        children
            .into_iter()
            .filter(|child| !child.optional || self.inputs.config.optional)
            .filter_map(|child| self.child_visit(snapshot, child))
            .collect()
    }

    fn recorded_directory_children(&self, snapshot: &pnpm_lockfile::SnapshotEntry) -> Vec<Visit> {
        [
            snapshot.dependencies.as_ref(),
            self.inputs.config.optional
                .then_some(snapshot.optional_dependencies.as_ref())
                .flatten(),
        ]
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|(name, reference)| {
            let key = reference.resolve(name)?;
            let metadata = self.lockfile.packages
                .as_ref()?
                .get(&key.without_peer())?;
            let pnpm_lockfile::LockfileResolution::Directory(resolution) = &metadata.resolution
            else {
                return None;
            };
            let source = metadata.specifiers.as_ref().and_then(|recorded| recorded.manifest_source);
            Some(Visit {
                dep: (
                    resolution.directory.clone(),
                    matches!(source, Some(pnpm_lockfile::DirectoryManifestSource::Path)),
                ),
                alias: name.to_string(),
                key,
            })
        })
        .collect()
    }

    fn child_visit(
        &self,
        snapshot: &pnpm_lockfile::SnapshotEntry,
        child: pnpm_resolving_deps_resolver::DependencySpec,
    ) -> Option<Visit> {
        let name = pnpm_lockfile::PkgName::parse(&child.alias).ok()?;
        let recorded =
            if child.optional { &snapshot.optional_dependencies } else { &snapshot.dependencies };
        let key = recorded
            .as_ref()?
            .get(&name)?
            .resolve(&name)?;
        let metadata = self.lockfile.packages
            .as_ref()?
            .get(&key.without_peer())?;
        let pnpm_lockfile::LockfileResolution::Directory(resolution) = &metadata.resolution else {
            return None;
        };
        Some(Visit {
            dep: (resolution.directory.clone(), is_declared_by_path(&child.bare_specifier)),
            alias: child.alias,
            key,
        })
    }
}

fn dependency_manifest_sources<'a>(
    inputs: &LockfileFreshnessInputs<'_, 'a>,
) -> super::ProjectManifestsByDir<'a> {
    if inputs.config.add_missing_peer_types {
        project_manifests_by_dir(
            inputs.dependency_manifests.declared.iter().map(|(_, manifest)| *manifest),
            inputs.dependency_manifests.injected,
        )
    } else {
        project_manifests_by_dir(
            inputs.manifests.iter().map(|(_, manifest)| *manifest),
            inputs.dependency_manifests.injected,
        )
    }
}
