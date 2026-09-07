pub mod build_graph;
pub mod build_modules;
pub mod create_symlink_layout;
pub mod create_virtual_dir_by_snapshot;
pub mod create_virtual_store;
pub mod current_lockfile;
mod custom_fetcher;
pub mod deps_graph;
pub mod dir_clone_cache;
pub mod hoist;
pub mod hoisted_dep_graph;
pub mod hoisting_limits;
pub mod import_indexed_dir;
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
pub mod pnp;
pub mod prune_direct_deps;
pub mod prune_stale_modules;
pub mod remove_quarantine;
pub mod retry_config;
pub mod safe_join_modules_dir;
mod shared_side_effects;
pub mod store_init;
pub mod symlink_direct_dependencies;
pub mod symlink_package;
pub mod validate_lockfile_paths;
pub mod version_policy;
pub mod virtual_store_layout;

pub use build_graph::*;
pub use build_modules::*;
pub use create_symlink_layout::*;
pub use create_virtual_dir_by_snapshot::*;
pub use create_virtual_store::*;
pub use current_lockfile::*;
pub use custom_fetcher::CustomFetcherSession;
pub use deps_graph::*;
pub use dir_clone_cache::*;
pub use hoist::*;
pub use hoisted_dep_graph::*;
pub use hoisting_limits::*;
pub use import_indexed_dir::*;
pub use install_frozen_lockfile::*;
pub use install_package_by_snapshot::*;
pub use install_package_from_registry::*;
pub use installability::*;
pub use link_bins::*;
pub use link_file::*;
pub use link_hoisted_modules::*;
pub use link_root_component_members::*;
pub use package_map::*;
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

pub const NEEDS_BUILD_MARKER: &str = ".pnpm-needs-build";

pub fn store_index_key_for_resolution(
    resolution: &pnpm_lockfile::LockfileResolution,
    pkg_id: &str,
    built: bool,
) -> Option<String> {
    match resolution {
        pnpm_lockfile::LockfileResolution::Tarball(tarball) => {
            Some(pnpm_store_dir::pick_store_index_key(
                tarball.integrity.as_ref().map(ToString::to_string).as_deref(),
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
    pnpm_deps_path::index_of_dep_path_suffix(&snapshot_key.to_string()).patch_hash_index.is_some()
}

/// Returns the package identity used to match a lockfile snapshot against
/// `patchedDependencies`.
///
/// Non-registry package keys carry their resolution in the version slot, so
/// the manifest version recorded in `packages:` takes precedence when present.
#[must_use]
pub fn name_version_from_package_key(
    key: &pnpm_lockfile::PackageKey,
    packages: Option<
        &std::collections::HashMap<pnpm_lockfile::PackageKey, pnpm_lockfile::PackageMetadata>,
    >,
) -> (String, String) {
    let metadata_key = key.without_peer();
    let name = metadata_key.name.to_string();
    let version = packages
        .and_then(|packages| packages.get(&metadata_key))
        .and_then(|metadata| metadata.version.clone())
        .unwrap_or_else(|| metadata_key.suffix.version().to_string());
    (name, version)
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

#[must_use]
pub fn should_write_package_map(
    config: &pnpm_config::Config,
    node_linker: pnpm_config::NodeLinker,
) -> bool {
    node_linker == pnpm_config::NodeLinker::Isolated && !config.virtual_store_only
}
