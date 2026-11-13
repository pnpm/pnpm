use super::{InstallError, run::InstallExecution};
use pnpm_config::{Config, NodeLinker};

pub(super) fn repair_global_virtual_store_bin_shims(
    config: &Config,
    execution: InstallExecution,
) -> Result<(), InstallError> {
    if !config.enable_global_virtual_store
        || config.frozen_store
        || execution.node_linker != NodeLinker::Isolated
        || execution.lockfile_only
        || execution.dry_run
    {
        return Ok(());
    }

    pnpm_deps_restorer::repair_global_virtual_store_bin_shims(
        config.virtual_store_dir(),
        &pnpm_deps_restorer::shim_link_options(config, execution.node_linker),
    )
    .map_err(InstallError::RepairGlobalVirtualStoreBins)
}
