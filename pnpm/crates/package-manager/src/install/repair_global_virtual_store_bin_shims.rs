use super::{InstallError, run::InstallExecution};
use pnpm_config::{Config, NodeLinker};
use pnpm_store_dir::StoreOperationLock;

pub(super) fn lock_store(
    config: &Config,
    execution: InstallExecution,
) -> Result<StoreOperationLock, InstallError> {
    if should_repair(config, execution) {
        ensure_global_virtual_store_bin_shims(config, execution.node_linker)?;
    }
    lock_shared_store(config)
}

pub fn ensure_global_virtual_store_bin_shims(
    config: &Config,
    node_linker: NodeLinker,
) -> Result<(), InstallError> {
    if !config.enable_global_virtual_store || config.frozen_store || !needs_repair(config)? {
        return Ok(());
    }
    let migration_lock = config.store_dir.lock_for_update().map_err(InstallError::StoreLock)?;
    if needs_repair(config)? {
        repair_global_virtual_store_bin_shims(config, node_linker)?;
    }
    drop(migration_lock);
    Ok(())
}

fn should_repair(config: &Config, execution: InstallExecution) -> bool {
    config.enable_global_virtual_store
        && !config.frozen_store
        && execution.mutation != crate::ProjectMutation::NoInstall
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
    node_linker: NodeLinker,
) -> Result<(), InstallError> {
    pnpm_deps_restorer::repair_global_virtual_store_bin_shims(
        config.virtual_store_dir(),
        &pnpm_deps_restorer::shim_link_options(config, node_linker),
    )
    .map_err(InstallError::RepairGlobalVirtualStoreBins)
}
