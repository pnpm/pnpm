use super::{FreshPlan, InstallWithFreshLockfileError};
use pnpm_lockfile::Lockfile;

pub(super) async fn maybe_materialize_through_package_provider(
    config: &'static pnpm_config::Config,
    lockfile_dir: &std::path::Path,
    materialization_lockfile: &Lockfile,
    patches_record: Option<&pnpm_patching::PatchGroupRecord>,
    plan: &mut FreshPlan<'_>,
) -> Result<(), InstallWithFreshLockfileError> {
    let Some(package_provider) = config.package_provider.as_deref() else {
        return Ok(());
    };
    let patches = pnpm_deps_restorer::resolve_snapshot_patches(
        config,
        patches_record,
        materialization_lockfile.snapshots.as_ref(),
        materialization_lockfile.packages.as_ref(),
    )
    .map_err(InstallWithFreshLockfileError::BuildPhase)?;
    let inputs = pnpm_deps_restorer::PackageProviderInputs {
        package_provider,
        lockfile_dir,
        snapshots: materialization_lockfile.snapshots.as_ref(),
        packages: materialization_lockfile.packages.as_ref(),
        skipped: &plan.skipped,
        patches: patches.as_ref(),
        engine: plan.engine_name.as_deref(),
        config,
    };
    let provided = pnpm_deps_restorer::materialize_through_package_provider(&inputs)
        .await
        .map_err(InstallWithFreshLockfileError::PackageProvider)?;
    plan.layout.set_provider_paths(provided.paths);
    plan.skipped.add_provider_failed(provided.skipped);
    Ok(())
}
