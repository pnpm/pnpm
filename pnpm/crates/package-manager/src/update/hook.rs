use super::UpdateError;
use crate::InstallError;
use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::Reporter;
use std::{
    path::Path,
    sync::Arc,
};

/// A loaded `readPackage` hook paired with the log sink its `context.log`
/// calls are forwarded to.
pub(super) type ReadPackageHook = (Arc<dyn pnpm_hooks::PnpmfileHooks>, pnpm_hooks::LogFn);

pub(super) fn update_read_package_hook<Reporter: self::Reporter>(
    workspace_root: &Path,
    config: &Config,
) -> Result<Option<ReadPackageHook>, UpdateError> {
    let Some(hook) = crate::load_install_pnpmfiles(config, workspace_root)
        .map_err(UpdateError::MissingPnpmfile)?
    else {
        return Ok(None);
    };
    let log = match hook.source_path() {
        Some(from) => crate::install_with_fresh_lockfile::hook_log_fn::<Reporter>(
            workspace_root,
            from,
            "readPackage",
        ),
        None => Arc::new(|_| {}) as pnpm_hooks::LogFn,
    };
    Ok(Some((hook, log)))
}
pub(super) async fn apply_read_package_hook_to_update_manifest(
    manifest: &mut PackageManifest,
    hook: &Arc<dyn pnpm_hooks::PnpmfileHooks>,
    log: &pnpm_hooks::LogFn,
) -> Result<(), UpdateError> {
    let ctx = pnpm_hooks::HookContext { log: Arc::clone(log), dir: None };
    let value = hook
        .read_package(manifest.value().clone(), ctx)
        .await
        .map_err(InstallError::from)
        .map_err(UpdateError::Install)?;
    *manifest.value_mut() = (*value).clone();
    Ok(())
}
