use crate::{
    DependenciesGraphNode, DirCloneCache, RequiresBuildBySnapshot,
    create_virtual_store::{dir_clone_cacheable, package_content_changed},
};
use pnpm_lockfile::{PackageKey, PackageMetadata};
use pnpm_reporter::Reporter;
use std::{
    collections::{HashMap, HashSet},
    fmt, fs,
    path::PathBuf,
};

/// Projects only pristine, immutable snapshots into fresh hoisted destinations.
pub struct HoistedDirCloneCache<'a> {
    cache: &'a DirCloneCache<'a>,
    snapshots: HashSet<&'a PackageKey>,
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
        requires_build: Option<&'a RequiresBuildBySnapshot>,
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
                    // Every mutable source is already excluded by the
                    // integrity check `dir_clone_cacheable` ends on: a
                    // `file:` directory records no integrity at all,
                    // and a `file:` tarball's does not pin its bytes.
                    false,
                    // A snapshot whose recorded content differs from
                    // the previous install's must be re-imported rather
                    // than served from the slot that install populated.
                    package_content_changed(current_packages, packages, key),
                )
            })
            .map(|(key, _)| key)
            .collect::<HashSet<_>>();
        tracing::debug!(
            target: "pacquet::dir_clone_cache",
            eligible_snapshots = snapshots.len(),
            "qualified hoisted snapshots",
        );
        Some(Self { cache, snapshots })
    }

    pub(super) fn try_import<Log: Reporter>(
        &self,
        node: &DependenciesGraphNode,
        import: crate::PackageImportOptions<'_>,
        cas_paths: &HashMap<String, PathBuf>,
    ) -> bool {
        if node.present || node.package.patch.is_some() || node.package.has_bundled_dependencies {
            return false;
        }
        let Ok(key) = node.package.dep_path.as_str().parse::<PackageKey>() else { return false };
        if !self.snapshots.contains(&key) || ships_bundled_modules(cas_paths) {
            return false;
        }
        // A hoisted alias can have a different scope from its source package.
        let Some(parent) = node.dir.parent() else { return false };
        if fs::create_dir_all(parent).is_err() {
            return false;
        }
        self.cache.try_import::<Log>(
            import.logged_methods,
            import.method,
            &key,
            &node.dir,
            cas_paths,
        )
    }
}

/// Whether the package's own files include a `node_modules` directory.
/// A tarball can ship one without declaring `bundledDependencies`, and
/// the hoisted importer merges such a directory with the nested
/// packages the walker places inside it (`keep_modules_dir`) — which a
/// clone of the canonical slot cannot reproduce.
fn ships_bundled_modules(cas_paths: &HashMap<String, PathBuf>) -> bool {
    cas_paths
        .keys()
        .any(|path| {
            path.split('/')
                .any(|part| part == "node_modules")
        })
}

#[cfg(test)]
mod tests;
