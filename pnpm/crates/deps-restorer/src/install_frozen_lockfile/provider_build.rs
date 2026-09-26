use super::{
    BuildPhaseError, FrozenInputs, InstallFrozenLockfileError, LockfileVerificationOverride,
    SkipSetPlan, resolve_snapshot_patches,
};
use crate::{
    AllowBuildPolicy, CreateVirtualStoreOutput, SkippedSnapshots, VirtualStoreLayout,
    direct_dep_names_for_importer, link_top_level_bins,
    symlink_direct_dependencies::importer_root_dir,
};
use pnpm_lockfile::Lockfile;
use pnpm_reporter::{IgnoredScriptsLog, LogEvent, LogLevel, Reporter};
use std::ffi::OsStr;

pub(super) async fn materialize_frozen_provider(
    install: &FrozenInputs<'_>,
    allow_build_policy: &AllowBuildPolicy,
    package_provider: &str,
    settled: &mut SkipSetPlan,
    layout: &mut VirtualStoreLayout,
    verification_override: Option<LockfileVerificationOverride<'_>>,
) -> Result<CreateVirtualStoreOutput, InstallFrozenLockfileError> {
    if let Some(verification) = verification_override {
        verification.await?;
    }
    let patches = resolve_snapshot_patches(
        install.drivers.config,
        install.build_policy(allow_build_policy).patch_groups,
        install.entries().snapshots,
        install.entries().packages,
    )
    .map_err(InstallFrozenLockfileError::BuildPhase)?;
    let provided = crate::materialize_through_package_provider(&crate::PackageProviderInputs {
        package_provider,
        lockfile_dir: install.projects.workspace_root,
        snapshots: install.entries().snapshots,
        packages: install.entries().packages,
        skipped: &settled.skipped,
        patches: patches.as_ref(),
        engine: settled.engine_name.as_deref(),
        config: install.drivers.config,
    })
    .await
    .map_err(InstallFrozenLockfileError::PackageProvider)?;
    layout.set_provider_paths(provided.paths);
    settled.skipped.add_provider_failed(provided.skipped);
    Ok(CreateVirtualStoreOutput::default())
}

pub(super) fn link_provider_top_level_bins<Reporter: self::Reporter>(
    install: &FrozenInputs<'_>,
    linked: &crate::linking::LinkPhaseOutput,
    skipped: &SkippedSnapshots,
) -> Result<(), InstallFrozenLockfileError> {
    Reporter::emit(&LogEvent::IgnoredScripts(IgnoredScriptsLog {
        level: LogLevel::Debug,
        package_names: Vec::new(),
        strict_dep_builds: install.drivers.config.strict_dep_builds,
    }));
    let modules_dir_basename: &OsStr = install.drivers.config.modules_dir
        .file_name()
        .unwrap_or_else(|| OsStr::new("node_modules"));
    let link_options =
        crate::shim_link_options(install.drivers.config, install.platform.node_linker);
    for (importer_id, importer_snapshot) in &install.lockfiles.wanted.importers {
        let project_dir = importer_root_dir(install.projects.workspace_root, importer_id);
        let modules_dir = project_dir.join(modules_dir_basename);
        let direct_names = direct_dep_names_for_importer(
            importer_snapshot,
            install.projects.dependency_groups.iter().copied(),
            skipped,
            false,
        );
        let hoisted_names: &[String] = if *importer_id == Lockfile::ROOT_IMPORTER_KEY {
            &linked.publicly_hoisted_for_post_build
        } else {
            &[]
        };
        link_top_level_bins(&modules_dir, &direct_names, hoisted_names, &[], &link_options)
            .map_err(BuildPhaseError::TopLevelBinLink)
            .map_err(InstallFrozenLockfileError::BuildPhase)?;
    }
    Ok(())
}
