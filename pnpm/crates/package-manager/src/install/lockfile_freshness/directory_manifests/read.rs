use super::{DirectoryDep, DirectoryManifests, DirectoryWalk, FreshnessCheckError, Visit};
use crate::install::lockfile_freshness::directory_deps::{
    InjectedDirectory, overridden_unhooked_manifest, unhooked_manifest,
};
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::directory_package_dir;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

impl DirectoryWalk<'_, '_> {
    pub(super) async fn read_all(
        &self,
        mut level: Vec<Visit>,
    ) -> Result<DirectoryManifests, FreshnessCheckError> {
        let mut walked: HashSet<(pnpm_lockfile::PackageKey, DirectoryDep)> = HashSet::new();
        let mut read: HashMap<DirectoryDep, Option<(PackageManifest, bool)>> = HashMap::new();
        while !level.is_empty() {
            self.normalize_sources(&mut level);
            level.retain(|visit| walked.insert((visit.key.clone(), visit.dep.clone())));
            self.read_level(&level, &mut read).await?;
            level = level
                .iter()
                .filter_map(|visit| {
                    let (manifest, _) = read.get(&visit.dep)?.as_ref()?;
                    Some(self.children(visit, manifest))
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
    fn normalize_sources(&self, level: &mut [Visit]) {
        for visit in level {
            if let Some(source) = self.lockfile.packages
                .as_ref()
                .and_then(|packages| packages.get(&visit.key.without_peer()))
                .and_then(|metadata| metadata.specifiers.as_ref())
                .and_then(|recorded| recorded.manifest_source)
            {
                visit.dep.1 = matches!(source, pnpm_lockfile::DirectoryManifestSource::Path);
            }
        }
    }

    pub(super) async fn read(
        &self,
        visit: &Visit,
    ) -> Result<Option<(PackageManifest, bool)>, FreshnessCheckError> {
        let (directory, declared_by_path) = &visit.dep;
        let dir = directory_package_dir(self.inputs.lockfile_dir, directory);
        let recorded = self.lockfile.packages
            .as_ref()
            .and_then(|packages| packages.get(&visit.key.without_peer()))
            .is_some_and(|metadata| metadata.specifiers.is_some());
        let manifests_by_dir = if recorded {
            self.declared_manifests_by_dir.as_ref().unwrap_or(&self.manifests_by_dir)
        } else {
            &self.manifests_by_dir
        };
        let injected = InjectedDirectory {
            dir: &dir,
            declared_by_path: *declared_by_path,
            workspace_root: self.inputs.config.workspace_dir
                .as_deref()
                .unwrap_or(self.inputs.lockfile_dir),
            manifests_by_dir,
        };
        if manifests_by_dir.contains_key(&dir)
            && !self.inputs.config.add_missing_peer_types
            && !recorded
        {
            let manifest = overridden_unhooked_manifest(
                &injected,
                self.parsed_overrides,
                self.inputs.config.root_project_manifest_dir(self.inputs.lockfile_dir),
                self.inputs.catalogs,
            );
            return Ok(manifest.map(|manifest| (manifest, false)));
        }
        let Some(manifest) = unhooked_manifest(&injected) else { return Ok(None) };
        let manifest = self.hook_manifest(manifest, directory).await?;
        Ok(Some((manifest, true)))
    }

    async fn hook_manifest(
        &self,
        mut manifest: PackageManifest,
        directory: &str,
    ) -> Result<PackageManifest, FreshnessCheckError> {
        let hook_ctx =
            || pnpm_hooks::HookContext { log: Arc::new(|_| {}), dir: Some(directory.to_string()) };
        let value = self.hooks
            .apply(Arc::new(manifest.value().clone()), hook_ctx)
            .await
            .map_err(FreshnessCheckError::ReadPackageHook)?;
        *manifest.value_mut() = (*value).clone();
        Ok(manifest)
    }
}
