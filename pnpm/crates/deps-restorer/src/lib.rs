pub mod build_graph;
pub mod build_modules;
pub mod create_symlink_layout;
pub mod create_virtual_dir_by_snapshot;
pub mod create_virtual_store;
pub mod current_lockfile;
pub mod deps_graph;
pub mod dir_clone_cache;
pub mod hoist;
pub mod hoisted_dep_graph;
pub mod hoisting_limits;
pub mod import_indexed_dir;
pub mod install_context;
pub mod install_frozen_lockfile;
pub mod install_package_by_snapshot;
pub mod install_package_from_registry;
pub mod installability;
pub mod link_bins;
pub mod link_file;
pub mod link_hoisted_modules;
pub mod link_root_component_members;
pub mod linking;
pub mod materialization_plan;
pub mod package_map;
pub mod package_provider;
pub mod pnp;
pub mod prune_direct_deps;
pub mod prune_stale_modules;
pub mod remove_quarantine;
pub mod report_direct_dependency_changes;
pub mod retry_config;
pub mod safe_join_modules_dir;
pub mod store_init;
pub mod symlink_direct_dependencies;
pub mod symlink_package;
pub mod validate_lockfile_paths;
pub mod version_policy;
pub mod virtual_store_layout;
pub use build_graph::*;
pub use build_modules::*;
pub use build_options::{
    BuildCacheContext, BuildGraphInputs, BuildLayout, BuildProgress, BuildScriptOptions,
    BuildSnapshotInputs, PatchedEngineCheck, ScriptPath,
};
pub use create_symlink_layout::*;
pub use create_virtual_dir_by_snapshot::*;
pub use create_virtual_store::*;
pub use current_lockfile::*;
pub use custom_fetcher::{CustomFetcherSession, ResolvedTarballMetadata};
pub use deps_graph::*;
pub use dir_clone_cache::*;
pub use frozen_install_options::{
    FrozenInstallDrivers, FrozenInstallSeed, FrozenLockfileInputs, FrozenPlatformOptions,
    FrozenProjectInputs, PriorMaterialization,
};
pub use hoist::*;
pub use hoisted_dep_graph::*;
pub use hoisting_limits::*;
pub use import_indexed_dir::*;
pub use install_context::*;
pub use install_frozen_lockfile::*;
pub use install_options::{
    DirectLinkPolicy, ImporterDependencyGraph, ImporterLinkContext, SkipSetClosure,
    SnapshotSelection, VirtualStoreFetchInputs,
};
pub use install_package_by_snapshot::*;
pub use install_package_from_registry::*;
pub use installability::*;
pub use installed_hoisted_state::InstalledHoistedState;
pub use link_bins::*;
pub use link_file::*;
pub use link_hoisted_modules::*;
pub use link_root_component_members::*;
pub use materialization_options::{
    ModuleLinkerContext, PackageImportOptions, RegistryFetchContext, SlotImportSource,
    SnapshotDependencyLinks, SnapshotFetchContext, VirtualStoreLinkOptions,
};
pub use package_map::*;
pub use package_provider::*;
pub use phase_options::{
    BuildPhaseCache, BuildPhaseDirectories, BuildPhaseGraph, BuildPhasePolicy, HoistedLinkGraph,
    HoistedProjects, LinkLockfiles, LinkPackageData, LinkProjects, PriorHoistedState,
    PriorLinkState,
};
pub use pnp::*;
pub use pnpm_workspace_task_scheduler::{GraphSequencerResult, PathNode, graph_sequencer};
pub use prune_direct_deps::*;
pub use prune_stale_modules::*;
pub use safe_join_modules_dir::*;
pub use symlink_direct_dependencies::*;
pub use symlink_package::*;
pub use validate_lockfile_paths::*;
pub use version_policy::*;
pub use virtual_store_layout::*;

mod custom_fetcher;
mod gvs_slot_lock;
mod installed_hoisted_state;
mod shared_side_effects;

pub const NEEDS_BUILD_MARKER: &str = ".pnpm-needs-build";

pub fn store_index_key_for_resolution(
    resolution: &pnpm_lockfile::LockfileResolution,
    pkg_id: &str,
    built: bool,
) -> Option<String> {
    match resolution {
        pnpm_lockfile::LockfileResolution::Tarball(tarball) => {
            Some(pnpm_store_dir::pick_store_index_key(
                tarball.integrity
                    .as_ref()
                    .map(ToString::to_string)
                    .as_deref(),
                tarball.is_git_hosted(),
                pkg_id,
                built,
            ))
        }
        pnpm_lockfile::LockfileResolution::Git(_) => {
            Some(pnpm_store_dir::git_hosted_store_index_key(pkg_id, built))
        }
        _ => resolution
            .integrity()
            .map(|integrity| pnpm_store_dir::store_index_key(&integrity.to_string(), pkg_id)),
    }
}

#[must_use]
pub fn snapshot_has_patch(snapshot_key: &pnpm_lockfile::PackageKey) -> bool {
    pnpm_deps_path::index_of_dep_path_suffix(&snapshot_key.to_string())
        .patch_hash_index
        .is_some()
}

const MAX_SCRIPT_THREADS: usize = 256;

#[must_use]
pub fn script_thread_count(child_concurrency: u32, max_work_items: usize) -> usize {
    usize::try_from(child_concurrency)
        .expect("u32 child concurrency fits in usize")
        .max(1)
        .min(max_work_items.max(1))
        .min(MAX_SCRIPT_THREADS)
}

/// Whether this install writes `node_modules/.package-map.json`.
///
/// Nothing reads the map unless `nodeExperimentalPackageMap` is set:
/// `package_map_path_for_execution` returns `None` without it, so
/// `pnpm run` and `pnpm exec` never hand the file to Node.
///
/// Hoisted installs answer the same question through
/// [`should_write_hoisted_package_map`], because they write the map
/// from their own linker.
#[must_use]
pub fn should_write_package_map(
    config: &pnpm_config::Config,
    node_linker: pnpm_config::NodeLinker,
) -> bool {
    config.node_experimental_package_map
        && node_linker == pnpm_config::NodeLinker::Isolated
        && !config.virtual_store_only
}

/// [`should_write_package_map`] for the hoisted linker, which builds
/// the map from the real `node_modules` layout rather than the virtual
/// store and so has a writer of its own.
#[must_use]
pub fn should_write_hoisted_package_map(config: &pnpm_config::Config) -> bool {
    config.node_experimental_package_map && !config.virtual_store_only
}

mod build_options;

mod materialization_options;

mod frozen_install_options;

mod install_options;

mod phase_options;
