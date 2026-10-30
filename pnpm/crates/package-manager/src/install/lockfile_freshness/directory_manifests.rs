//! The manifests of the directory packages a lockfile check reads, as the
//! resolver read them.

use super::{
    FreshnessCheckError, LockfileFreshnessInputs,
    directory_deps::{
        InjectedDirectory, NestedByImporter, is_declared_local_directory, is_deploy_file_url_key,
        overridden_unhooked_manifest, unhooked_manifest,
    },
    project_manifests_by_dir,
};
use crate::install::{Arc, Lockfile, PackageManifest, PathBuf};
use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_deps_resolver::{ChildCatalogs, ManifestTransformHooks};
use std::collections::{HashMap, HashSet};

/// A directory dependency as the check reaches it: the directory the
/// lockfile records, as written, and whether a `file:` spec declared it.
/// The two declarations can read different manifests; see
/// [`unhooked_manifest`].
type DirectoryDep = (String, bool);

/// The manifests [`hooked_directory_manifests`] returns, by [`DirectoryDep`].
#[derive(Default)]
pub(crate) struct DirectoryManifests(HashMap<DirectoryDep, PackageManifest>);

impl DirectoryManifests {
    /// No manifests: every directory package is read from its directory.
    pub(crate) fn none() -> &'static Self {
        static NONE: std::sync::OnceLock<DirectoryManifests> = std::sync::OnceLock::new();
        NONE.get_or_init(Self::default)
    }

    pub(crate) fn get(&self, directory: &str, declared_as_file: bool) -> Option<&PackageManifest> {
        self.0.get(&(directory.to_string(), declared_as_file))
    }
}

/// The manifest of every directory package the importers of `inputs` reach
/// through directory packages, and of every `nested` one they reach and the directory
/// packages beneath it, that no project in `inputs` describes, with
/// the resolver's manifest hooks applied as the resolver applies them:
/// `packageExtensions`, the pnpmfile's `readPackage` with the recorded
/// directory, overrides, and `ignoredOptionalDependencies`.
///
/// The children of a package come from its hooked manifest, so the packages
/// are hooked one level of the graph at a time.
pub(super) async fn hooked_directory_manifests(
    lockfile: &Lockfile,
    inputs: &LockfileFreshnessInputs<'_, '_>,
    (parsed_overrides, nested): (
        Option<&[pnpm_config_parse_overrides::VersionOverride]>,
        &NestedByImporter<'_>,
    ),
) -> Result<DirectoryManifests, FreshnessCheckError> {
    let mut level = direct_directory_deps(lockfile, inputs);
    level.extend(nested_directory_deps(nested));
    if level.is_empty() {
        return Ok(DirectoryManifests::default());
    }
    let walk = DirectoryWalk {
        lockfile,
        inputs,
        parsed_overrides,
        manifests_by_dir: project_manifests_by_dir(
            inputs.manifests.iter().map(|(_, manifest)| *manifest),
            inputs.dependency_manifests,
        ),
        hooks: resolver_hooks(inputs, parsed_overrides)?,
    };
    // A directory is read once, but each snapshot of it is walked: peer
    // variants of one package can record different children.
    let mut walked: HashSet<(pnpm_lockfile::PackageKey, DirectoryDep)> = HashSet::new();
    let mut read: HashMap<DirectoryDep, Option<(PackageManifest, bool)>> = HashMap::new();
    while !level.is_empty() {
        level.retain(|visit| walked.insert((visit.key.clone(), visit.dep.clone())));
        walk.read_level(&level, &mut read).await?;
        level = level
            .iter()
            .filter_map(|visit| {
                let (manifest, _) = read.get(&visit.dep)?.as_ref()?;
                Some(walk.children(visit, manifest))
            })
            .flatten()
            .collect();
    }
    let manifests = read
        .into_iter()
        .filter_map(|(dep, manifest)| match manifest {
            Some((manifest, true)) => Some((dep, manifest)),
            _ => None,
        })
        .collect();
    Ok(DirectoryManifests(manifests))
}

fn resolver_hooks(
    inputs: &LockfileFreshnessInputs<'_, '_>,
    parsed_overrides: Option<&[pnpm_config_parse_overrides::VersionOverride]>,
) -> Result<ManifestTransformHooks, FreshnessCheckError> {
    let versions_overrider = parsed_overrides.map(|parsed| {
        Arc::new(crate::VersionsOverrider::new(parsed, inputs.lockfile_dir))
    });
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
            let declared_as_file = is_declared_local_directory(&dependency.spec.specifier);
            let manifest = directory_manifests.get(directory, declared_as_file)?;
            let dir = pnpm_fs::lexical_normalize(&inputs.lockfile_dir.join(directory));
            Some((declared_as_file, dir, manifest))
        })
        .collect();
    // The map these extend is keyed by directory and serves workspace specs,
    // so a directory reached both ways keeps the workspace declaration's
    // manifest: it comes last.
    injected.sort_by_key(|(declared_as_file, ..)| !declared_as_file);
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
                dep: (directory.clone(), is_declared_local_directory(&dependency.spec.specifier)),
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
            [false, true].map(|declared_as_file| Visit {
                dep: (package.directory.clone(), declared_as_file),
                alias: package.alias.clone(),
                key: package.key.clone(),
            })
        })
}

struct DirectoryWalk<'a, 'i> {
    lockfile: &'a Lockfile,
    inputs: &'a LockfileFreshnessInputs<'i, 'i>,
    parsed_overrides: Option<&'a [pnpm_config_parse_overrides::VersionOverride]>,
    manifests_by_dir: super::ProjectManifestsByDir<'a>,
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

    /// The manifest the check reads for `visit`, and whether it was hooked
    /// here. A project's manifest comes in already hooked.
    async fn read(
        &self,
        visit: &Visit,
    ) -> Result<Option<(PackageManifest, bool)>, FreshnessCheckError> {
        let (directory, declared_as_file) = &visit.dep;
        let dir = pnpm_fs::lexical_normalize(&self.inputs.lockfile_dir.join(directory));
        let injected = InjectedDirectory {
            dir: &dir,
            declared_as_file: *declared_as_file,
            workspace_root: self.inputs.config.workspace_dir
                .as_deref()
                .unwrap_or(self.inputs.lockfile_dir),
            manifests_by_dir: &self.manifests_by_dir,
        };
        if self.manifests_by_dir.contains_key(&dir) {
            let manifest = overridden_unhooked_manifest(
                &injected,
                self.parsed_overrides,
                self.inputs.lockfile_dir,
            );
            return Ok(manifest.map(|manifest| (manifest, false)));
        }
        let Some(mut manifest) = unhooked_manifest(&injected) else { return Ok(None) };
        let hook_ctx =
            || pnpm_hooks::HookContext { log: Arc::new(|_| {}), dir: Some(directory.clone()) };
        let value = self.hooks
            .apply(Arc::new(manifest.value().clone()), hook_ctx)
            .await
            .map_err(FreshnessCheckError::ReadPackageHook)?;
        *manifest.value_mut() = (*value).clone();
        Ok(Some((manifest, true)))
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
        let (directory, declared_as_file) = &visit.dep;
        let dir = self.inputs.lockfile_dir.join(directory);
        let catalogs = (!declared_as_file).then_some(ChildCatalogs {
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
            dep: (resolution.directory.clone(), is_declared_local_directory(&child.bare_specifier)),
            alias: child.alias,
            key,
        })
    }
}
