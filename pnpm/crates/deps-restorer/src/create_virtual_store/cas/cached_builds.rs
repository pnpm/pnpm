use super::{CreateVirtualStoreOutput, immutable_download, retain_dependency_closure};
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use std::{collections::HashMap, sync::Arc};

struct CachedBuildInputs<'a> {
    selected: &'a mut HashMap<PackageKey, SnapshotEntry>,
    snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    packages: Option<&'a HashMap<PackageKey, pnpm_lockfile::PackageMetadata>>,
    fetched: &'a CreateVirtualStoreOutput,
    config: &'a pnpm_config::Config,
    allow_build_policy: &'a crate::AllowBuildPolicy,
    engine_name: Option<&'a str>,
}

impl CachedBuildInputs<'_> {
    fn retain(mut self) -> HashMap<PackageKey, String> {
        let Some(snapshots) = self.snapshots else { return HashMap::new() };
        let Some(packages) = self.packages else { return HashMap::new() };
        if !self.config.side_effects_cache_read()
            && self.config.remote_side_effects_cache.is_none()
        {
            return HashMap::new();
        }
        let roots = self.fetched.requires_build_by_snapshot
            .iter()
            .filter(|(_, requires_build)| **requires_build)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        if !roots
            .iter()
            .any(|key| self.fetched.side_effects.maps.contains_key(key))
        {
            return HashMap::new();
        }
        let graph = crate::build_deps_subgraph(snapshots, packages, roots);
        let mut deps_state_cache = pnpm_graph_hasher::DepsStateCache::new();
        self.fetched.requires_build_by_snapshot
            .iter()
            .filter(|(_, requires_build)| **requires_build)
            .filter_map(|(key, _)| {
                self.cache_key(&graph, &mut deps_state_cache, key)
                    .map(|cache_key| (key.clone(), cache_key))
            })
            .collect()
    }

    fn cache_key(
        &mut self,
        graph: &HashMap<PackageKey, pnpm_graph_hasher::DepsGraphNode<PackageKey>>,
        deps_state_cache: &mut pnpm_graph_hasher::DepsStateCache<PackageKey>,
        key: &PackageKey,
    ) -> Option<String> {
        if self.selected.contains_key(key) || !self.allow_build_policy.caches_build(key) {
            return None;
        }
        let snapshots = self.snapshots?;
        if immutable_download(self.fetched.cas_paths_by_pkg_id.as_ref()?, key).is_none() {
            retain_dependency_closure(self.selected, snapshots, vec![key.clone()]);
            return None;
        }
        let overlays = self.fetched.side_effects.maps.get(key)?;
        let should_run_scripts = !self.config.ignore_scripts
            && self.allow_build_policy.check(&key.without_peer().to_string()) == Some(true);
        let cache_key = pnpm_graph_hasher::calc_dep_state(
            graph,
            deps_state_cache,
            key,
            &pnpm_graph_hasher::CalcDepStateOptions {
                engine_name: self.engine_name?,
                patch_file_hash: None,
                include_dep_graph_hash: should_run_scripts,
            },
        );
        let overlay = overlays.get(&cache_key)?;
        if !overlay.symlinks.is_empty()
            || overlay.files
                .keys()
                .any(|path| path.ends_with(".node"))
        {
            retain_dependency_closure(self.selected, snapshots, vec![key.clone()]);
            return None;
        }
        Some(cache_key)
    }
}

pub(super) fn retain(
    selected: &mut HashMap<PackageKey, SnapshotEntry>,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    packages: Option<&HashMap<PackageKey, pnpm_lockfile::PackageMetadata>>,
    fetched: &CreateVirtualStoreOutput,
    context: &crate::InstallContext<'_>,
) -> HashMap<PackageKey, String> {
    CachedBuildInputs {
        selected,
        snapshots,
        packages,
        fetched,
        config: context.config,
        allow_build_policy: context.allow_build_policy,
        engine_name: context.linker.layout.engine_name(),
    }
    .retain()
}

/// Adds an overlay of the pristine files for each empty side-effects row
/// in `fetched`, so a build that changed nothing in its package loads
/// from CAS without running again.
///
/// Only the loaded linker restores such a build. The other linkers drop
/// the row and rebuild, so a script whose effect lands outside its
/// package runs in every project (pnpm/pnpm#14717). The loaded linker
/// leaves that to `sideEffectsCache.exclude`.
pub(super) fn restore_empty_builds(fetched: &mut CreateVirtualStoreOutput) {
    let Some(downloaded) = fetched.cas_paths_by_pkg_id.as_ref() else { return };
    for (key, rows) in &fetched.side_effects.diffs {
        let Some(files) = immutable_download(downloaded, key) else { continue };
        for (cache_key, _) in rows.iter().filter(|(_, diff)| diff.is_empty()) {
            Arc::make_mut(fetched.side_effects.maps.entry(key.clone()).or_default())
                .entry(cache_key.clone())
                .or_insert_with(|| (*files.cas_paths).clone().into());
        }
    }
}
