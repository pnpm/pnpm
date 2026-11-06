//! The walker's and the linker's options, read from the install inputs.

use super::{BTreeSet, HoistedLinkerInputs, Lockfile, LockfileToHoistedDepGraphOptions};

pub(super) fn hoisted_walker_options<'a>(
    inputs: &HoistedLinkerInputs<'a>,
    lockfile: &Lockfile,
    walker_skipped: BTreeSet<String>,
) -> LockfileToHoistedDepGraphOptions<'a> {
    let config = inputs.config;
    LockfileToHoistedDepGraphOptions {
        installability: crate::HoistedInstallability {
            engine_strict: config.effective_engine_strict(),
            current_node_version: inputs.host_node
                .map(|host| host.version.clone())
                .unwrap_or_default(),
            current_os: pnpm_graph_hasher::host_platform().to_string(),
            current_cpu: pnpm_graph_hasher::host_arch().to_string(),
            current_libc: pnpm_graph_hasher::host_libc().to_string(),
            supported_architectures: inputs.supported_architectures.cloned(),
        },
        placement: crate::HoistedPlacementOptions {
            auto_install_peers: config.auto_install_peers,
            hoist_workspace_packages: config.hoist_workspace_packages,
            hoisting_limits: crate::get_hoisting_limits(
                &lockfile.importers,
                config.hoisting_limits,
            ),
            external_dependencies: config.external_dependencies.clone(),
        },
        modules_dirs: crate::ImporterModulesDirs::from_config(
            config,
            inputs.projects.walker_lockfile_dir,
        ),

        skipped: walker_skipped,
        force: config.force,
        include_incompatible_packages: config.installs_incompatible_packages(),
        current_hoisted_locations: inputs.prior.current_hoisted_locations,
    }
}

pub(super) fn hoisted_dir_clone_cache<'a>(
    inputs: &HoistedLinkerInputs<'a>,
    lockfile: &Lockfile,
) -> Option<crate::link_hoisted_modules::HoistedDirCloneCache<'a>> {
    crate::link_hoisted_modules::HoistedDirCloneCache::new(
        inputs.materialization.dir_clone_cache,
        lockfile.packages.as_ref(),
        inputs.prior.current_lockfile.and_then(|lockfile| lockfile.packages.as_ref()),
        inputs.materialization.requires_build_by_snapshot,
        inputs.config.force,
    )
}
