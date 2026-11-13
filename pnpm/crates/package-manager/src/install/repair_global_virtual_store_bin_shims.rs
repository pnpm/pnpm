use super::{InstallError, run::InstallExecution};
use pnpm_config::{Config, NodeLinker};
use pnpm_store_dir::StoreOperationLock;

pub(super) fn lock_store(
    config: &Config,
    execution: InstallExecution,
) -> Result<StoreOperationLock, InstallError> {
    if should_repair(config, execution) && needs_repair(config)? {
        let migration_lock = config.store_dir.lock_for_update().map_err(InstallError::StoreLock)?;
        if needs_repair(config)? {
            repair_global_virtual_store_bin_shims(config, execution)?;
        }
        drop(migration_lock);
    }
    lock_shared_store(config)
}

fn should_repair(config: &Config, execution: InstallExecution) -> bool {
    config.enable_global_virtual_store
        && !config.frozen_store
        && execution.node_linker == NodeLinker::Isolated
        && !execution.lockfile_only
        && !execution.dry_run
}

fn needs_repair(config: &Config) -> Result<bool, InstallError> {
    pnpm_deps_restorer::global_virtual_store_bin_shims_need_repair(config.virtual_store_dir())
        .map_err(InstallError::RepairGlobalVirtualStoreBins)
}

fn lock_shared_store(config: &Config) -> Result<StoreOperationLock, InstallError> {
    let _span = tracing::debug_span!(target: "pacquet::install", "lock_store").entered();
    let lock = if config.frozen_store {
        config.store_dir.lock_for_frozen_use()
    } else {
        config.store_dir.lock_for_use()
    };
    lock.map_err(InstallError::StoreLock)
}

fn repair_global_virtual_store_bin_shims(
    config: &Config,
    execution: InstallExecution,
) -> Result<(), InstallError> {
    pnpm_deps_restorer::repair_global_virtual_store_bin_shims(
        config.virtual_store_dir(),
        &pnpm_deps_restorer::shim_link_options(config, execution.node_linker),
    )
    .map_err(InstallError::RepairGlobalVirtualStoreBins)
}
