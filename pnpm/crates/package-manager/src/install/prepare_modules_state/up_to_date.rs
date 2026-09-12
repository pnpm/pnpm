use super::super::{
    Arc, Catalogs, Config, Host, IncludedDependencies, InstallError, Lockfile, LogEvent, LogLevel,
    NodeLinker, PackageManifest, Path, PathBuf, PnpmLog, RebuildOptions, Reporter,
    ResolutionVerifier, Stage, StageLog, SummaryLog, SystemTime, build_workspace_state,
    frozen_tree_intact, gvs_build_marker_present, has_newly_allowed_ignored_builds,
    has_revoked_allowed_builds, map_frozen_lockfile_error, modules_consistent_with,
    unapproved_recorded_ignored_builds, update_workspace_state, verify_lockfile_eagerly,
};
use crate::optimistic_repeat_install::filesystem_now_ms;

/// Whether any package in the lockfile resolves to a local directory.
///
/// Such a package's source is mutable between installs, so its
/// materialized copy can go stale while every install-state artifact
/// still says the tree is current.
pub(super) fn has_directory_snapshot(lockfile: &Lockfile) -> bool {
    lockfile.packages.iter().flat_map(|packages| packages.values()).any(|metadata| {
        matches!(metadata.resolution, pnpm_lockfile::LockfileResolution::Directory(_))
    })
}
/// Everything the "nothing to do" verdict rests on.
pub(super) struct FrozenTreeUpToDate<'a> {
    pub(super) take_frozen_path: bool,
    pub(super) filtered_install: bool,
    pub(super) disable_optimistic_repeat_install: bool,
    pub(super) config: &'static Config,
    pub(super) workspace_root: &'a Path,
    pub(super) node_linker: NodeLinker,
    pub(super) included: IncludedDependencies,
    pub(super) lockfile: Option<&'a Lockfile>,
    pub(super) current_lockfile: Option<&'a Lockfile>,
    pub(super) modules_manifest: Option<&'a pnpm_modules_yaml::ModulesLayout>,
    pub(super) supported_architectures:
        Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    pub(super) rebuild: Option<&'a RebuildOptions>,
    pub(super) effective_node_version: Option<&'a str>,
}
/// The lockfile and modules manifest of a tree nothing has to be done to, or
/// `None` when the install has to materialize.
pub(super) fn frozen_tree_up_to_date<'a>(
    context: &FrozenTreeUpToDate<'a>,
) -> Option<(&'a Lockfile, &'a pnpm_modules_yaml::ModulesLayout)> {
    let config = context.config;
    if context.take_frozen_path
        && !context.filtered_install
        && !context.disable_optimistic_repeat_install
        // `--force` reinstalls everything, so an up-to-date tree
        // must not short-circuit the materialization.
        && !config.force
        && let Some(wanted_lockfile) = context.lockfile
        && let Some(current) = context.current_lockfile
        && wanted_lockfile == current
        // A `file:` dependency resolves to a directory whose
        // contents can change with nothing in the lockfile or
        // `.modules.yaml` moving, so an equal-lockfile tree is not
        // evidence that its slot is current. pnpm's `file:` is a
        // copy taken at install time, not a symlink, so the copy
        // has to be retaken; the TypeScript CLI has no gate at this
        // level at all and instead forces every directory dep
        // through materialization in `lockfileToDepGraph`.
        && !has_directory_snapshot(wanted_lockfile)
        && let Some(modules) = context.modules_manifest
        && modules_consistent_with(modules, config, context.node_linker, context.included)
        // A `supportedArchitectures` change alters the skip set
        // without touching the lockfile or `.modules.yaml`, so the
        // unchanged-layout premise doesn't hold and the platform
        // packages must be re-evaluated.
        && crate::optimistic_repeat_install::recorded_supported_architectures_match(
            context.workspace_root,
            context.supported_architectures,
        )
        // An `allowBuilds` change that now permits a previously-ignored
        // build must rebuild it, even though the lockfile and layout are
        // unchanged.
        && !has_newly_allowed_ignored_builds(modules, config)
        // The mirror image: an approval the user has since withdrawn
        // must be re-evaluated, or a strict install would exit 0 on a
        // package it is no longer allowed to build.
        && !has_revoked_allowed_builds(modules, config)
        // A build marker lives in the shared slot, outside every
        // project-state input checked above. Let materialization inspect
        // buildable and patched GVS slots instead of declaring the local
        // tree complete from importer links alone.
        && !gvs_build_marker_present(
            wanted_lockfile,
            config,
            context.workspace_root,
            context.effective_node_version,
        )
        // An explicit `pacquet rebuild` always re-runs the build phase,
        // so it never short-circuits here.
        && context.rebuild.is_none()
        && !modules_cache_prune_due(config, context.modules_manifest)
        && frozen_tree_intact(
            wanted_lockfile,
            modules,
            config,
            context.workspace_root,
            context.node_linker,
        )
    {
        return Some((wanted_lockfile, modules));
    }
    None
}
pub(super) fn modules_cache_prune_due(
    config: &Config,
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
) -> bool {
    modules_manifest.is_some_and(|modules| {
        crate::prune_virtual_store::should_prune_virtual_store(
            crate::prune_virtual_store::same_dir(
                config.effective_virtual_store_dir(),
                &config.global_virtual_store_dir,
            ),
            Some(modules.pruned_at.as_str()),
            config.modules_cache_max_age,
            SystemTime::now(),
        )
    })
}
/// What the up-to-date early return still has to write and report.
pub(super) struct UpToDateInstall<'a, 'install> {
    pub(super) config: &'static Config,
    pub(super) workspace_root: &'a Path,
    pub(super) node_linker: NodeLinker,
    pub(super) included: IncludedDependencies,
    pub(super) wanted_lockfile: &'a Lockfile,
    pub(super) modules: &'a pnpm_modules_yaml::ModulesLayout,
    pub(super) supported_architectures:
        Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    pub(super) catalogs: &'a Catalogs,
    pub(super) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(super) filtered_install: bool,
    pub(super) prefix: &'a str,
    pub(super) resolution_verifiers: &'a [Arc<dyn ResolutionVerifier>],
    pub(super) derived_lockfile_path: Option<&'a Path>,
    pub(super) lockfile_verification_override:
        Option<super::super::LockfileVerificationOverride<'install>>,
    pub(super) lockfile_synthesized_from_current: bool,
    pub(super) lockfile_was_fast_updated: bool,
    pub(super) save_lockfile: bool,
}
/// Up-to-date installs still enforce dependency-name verification and recorded build policy.
pub(super) async fn report_up_to_date<Reporter: self::Reporter + 'static>(
    mut context: UpToDateInstall<'_, '_>,
) -> Result<(), InstallError> {
    pnpm_lockfile_verification::verify_lockfile_dependency_names(context.wanted_lockfile)
        .map_err(InstallError::LockfileVerification)?;
    // Nothing to materialize means no fetch to overlap; verify
    // eagerly before the up-to-date early return.
    verify_up_to_date_lockfile::<Reporter>(
        context.wanted_lockfile,
        context.lockfile_verification_override.take(),
        context.resolution_verifiers,
        (context.derived_lockfile_path, &context.config.cache_dir),
    )
    .await?;
    enforce_recorded_build_policy(&context)?;
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: "Lockfile is up to date, resolution step is skipped".to_string(),
        prefix: context.prefix.to_string(),
    }));
    Reporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: context.prefix.to_string(),
        stage: Stage::ImportingDone,
    }));
    save_merged_wanted_lockfile(
        context.wanted_lockfile,
        context.config,
        context.workspace_root,
        (
            context.lockfile_synthesized_from_current,
            context.lockfile_was_fast_updated,
            context.save_lockfile,
        ),
    )?;
    refresh_up_to_date_workspace(&context)?;
    Reporter::emit(&LogEvent::Summary(SummaryLog {
        level: LogLevel::Debug,
        prefix: context.prefix.to_string(),
    }));
    Ok(())
}
// Verification must reject tampering before this policy can report ignored builds.
pub(super) fn enforce_recorded_build_policy(
    context: &UpToDateInstall<'_, '_>,
) -> Result<(), InstallError> {
    if context.config.strict_dep_builds
        && let Ok(Some(package_names)) =
            unapproved_recorded_ignored_builds(context.modules, context.config)
    {
        return Err(InstallError::IgnoredBuilds { package_names });
    }
    Ok(())
}
pub(super) fn refresh_up_to_date_workspace(
    context: &UpToDateInstall<'_, '_>,
) -> Result<(), InstallError> {
    update_workspace_state(
        context.workspace_root,
        &build_workspace_state::<Host>(
            context.workspace_root,
            context.config,
            context.node_linker,
            context.included,
            context.supported_architectures,
            context.catalogs,
            context.project_manifests,
            context.filtered_install,
            filesystem_now_ms(context.workspace_root),
        ),
    )
    .map_err(InstallError::WriteWorkspaceState)
}
pub(super) async fn verify_up_to_date_lockfile<Reporter: self::Reporter + 'static>(
    wanted_lockfile: &Lockfile,
    lockfile_verification_override: Option<super::super::LockfileVerificationOverride<'_>>,
    resolution_verifiers: &[Arc<dyn ResolutionVerifier>],
    paths: (Option<&Path>, &Path),
) -> Result<(), InstallError> {
    let (derived_lockfile_path, cache_dir) = paths;
    let Some(lockfile_verification_override) = lockfile_verification_override else {
        return verify_lockfile_eagerly::<Reporter>(
            wanted_lockfile,
            resolution_verifiers,
            derived_lockfile_path,
            cache_dir,
        )
        .await;
    };
    lockfile_verification_override.await.map_err(map_frozen_lockfile_error)
}
/// A merge produced a lockfile that no file on disk holds, so it has to be
/// written back even when nothing else changed.
pub(super) fn save_merged_wanted_lockfile(
    wanted_lockfile: &Lockfile,
    config: &Config,
    workspace_root: &Path,
    merge: (bool, bool, bool),
) -> Result<(), InstallError> {
    let (lockfile_synthesized_from_current, lockfile_was_fast_updated, save_lockfile) = merge;
    if (lockfile_synthesized_from_current
        || lockfile_was_fast_updated
        || config.merge_git_branch_lockfiles)
        && config.lockfile
        && save_lockfile
    {
        wanted_lockfile
            .save_to_path(&workspace_root.join(config.wanted_lockfile_name()))
            .map_err(InstallError::SaveWantedLockfile)?;
    }
    Ok(())
}
