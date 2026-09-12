use crate::{
    DependenciesGraphNode, DirCloneCache, RequiresBuildBySnapshot,
    create_virtual_store::{dir_clone_cacheable, package_content_changed},
};
use pnpm_config::PackageImportMethod;
use pnpm_lockfile::{PackageKey, PackageMetadata};
use pnpm_reporter::Reporter;
use std::{
    collections::{HashMap, HashSet},
    fmt, fs,
    path::PathBuf,
    sync::atomic::AtomicU8,
};

/// Projects only pristine, immutable snapshots into fresh hoisted destinations.
pub struct HoistedDirCloneCache<'a> {
    cache: &'a DirCloneCache<'a>,
    snapshots: HashSet<PackageKey>,
}

impl fmt::Debug for HoistedDirCloneCache<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("HoistedDirCloneCache").finish_non_exhaustive()
    }
}

impl<'a> HoistedDirCloneCache<'a> {
    pub(crate) fn new(
        cache: Option<&'a DirCloneCache<'a>>,
        packages: Option<&HashMap<PackageKey, PackageMetadata>>,
        current_packages: Option<&HashMap<PackageKey, PackageMetadata>>,
        requires_build: Option<&RequiresBuildBySnapshot>,
        force: bool,
    ) -> Option<Self> {
        if force {
            return None;
        }
        let cache = cache?;
        let packages = packages?;
        let snapshots = requires_build?
            .iter()
            .filter(|(key, requires_build)| {
                dir_clone_cacheable(
                    packages,
                    key,
                    **requires_build || crate::snapshot_has_patch(key),
                    false,
                    package_content_changed(current_packages, packages, key),
                )
            })
            .map(|(key, _)| key.clone())
            .collect::<HashSet<_>>();
        tracing::debug!(target: "pacquet::dir_clone_cache", eligible_snapshots = snapshots.len(), "qualified hoisted snapshots");
        Some(Self { cache, snapshots })
    }

    pub(super) fn try_import<Log: Reporter>(
        &self,
        node: &DependenciesGraphNode,
        logged_methods: &AtomicU8,
        import_method: PackageImportMethod,
        cas_paths: &HashMap<String, PathBuf>,
    ) -> bool {
        if node.present || node.patch.is_some() || node.has_bundled_dependencies {
            return false;
        }
        let Ok(key) = node.dep_path.as_str().parse::<PackageKey>() else { return false };
        if !self.snapshots.contains(&key)
            || cas_paths.keys().any(|path| path.split('/').any(|part| part == "node_modules"))
        {
            return false;
        }
        // A hoisted alias can have a different scope from its source package.
        let Some(parent) = node.dir.parent() else { return false };
        if fs::create_dir_all(parent).is_err() {
            return false;
        }
        self.cache.try_import::<Log>(logged_methods, import_method, &key, &node.dir, cas_paths)
    }
}

#[cfg(test)]
mod tests;
