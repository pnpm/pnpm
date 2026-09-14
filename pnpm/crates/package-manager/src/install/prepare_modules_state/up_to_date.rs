use super::super::{
    Arc, Config, Host, InstallError, Lockfile, LogEvent, LogLevel, Path, PnpmLog, Reporter,
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
    lockfile.packages
        .iter()
        .flat_map(|packages| packages.values())
        .any(|metadata| {
            matches!(metadata.resolution, pnpm_lockfile::LockfileResolution::Directory(_))
        })
}
/// Everything the "nothing to do" verdict rests on.
pub(super) struct FrozenTreeUpToDate<'a> {
    pub(crate) tree: crate::install::state_options::ModulesTreeContext<'a>,
    pub(crate) repeat: crate::install::state_options::RepeatInstallPolicy<'a>,
    pub(super) lockfile: Option<&'a Lockfile>,
    pub(super) current_lockfile: Option<&'a Lockfile>,
    pub(super) modules_manifest: Option<&'a pnpm_modules_yaml::ModulesLayout>,
}
/// The lockfile and modules manifest of a tree nothing has to be done to, or
/// `None` when the install has to materialize.
pub(super) fn frozen_tree_up_to_date<'a>(
    context: &FrozenTreeUpToDate<'a>,
) -> Option<(&'a Lockfile, &'a pnpm_modules_yaml::ModulesLayout)> {
    let config = context.tree.config;
    if context.repeat.frozen
        && !context.repeat.filtered
        && !context.repeat.disable_optimistic_check
        // `--force` reinstalls everything, so an up-to-date tree
        // must not short-circuit the materialization.
        && !config.force
        && let Some(wanted_lockfile) = context.lockfile
        && let Some(current) = context.current_lockfile
        // Past this gate `current` is the graph this install would
        // materialize, so every probe below reads it instead of the wanted
        // lockfile: a snapshot no importer reaches has no slot on disk and no
        // say in whether the tree is complete.
        && materialized_shape_matches(wanted_lockfile, current, context.tree.included)
        // A `file:` dependency resolves to a directory whose
        // contents can change with nothing in the lockfile or
        // `.modules.yaml` moving, so an equal-lockfile tree is not
        // evidence that its slot is current. pnpm's `file:` is a
        // copy taken at install time, not a symlink, so the copy
        // has to be retaken; the TypeScript CLI has no gate at this
        // level at all and instead forces every directory dep
        // through materialization in `lockfileToDepGraph`.
        && !has_directory_snapshot(current)
        && let Some(modules) = context.modules_manifest
        && modules_consistent_with(modules, config, context.tree.node_linker, context.tree.included)
        // A `supportedArchitectures` change alters the skip set
        // without touching the lockfile or `.modules.yaml`, so the
        // unchanged-layout premise doesn't hold and the platform
        // packages must be re-evaluated.
        && crate::optimistic_repeat_install::recorded_supported_architectures_match(
            context.tree.workspace_root,
            context.repeat.supported_architectures,
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
            current,
            config,
            context.tree.workspace_root,
            context.repeat.effective_node_version,
        )
        // An explicit `pacquet rebuild` always re-runs the build phase,
        // so it never short-circuits here.
        && context.repeat.rebuild.is_none()
        && !modules_cache_prune_due(config, context.modules_manifest)
        && frozen_tree_intact(
            current,
            modules,
            config,
            context.tree.workspace_root,
            context.tree.node_linker,
        )
    {
        return Some((wanted_lockfile, modules));
    }
    None
}
/// Whether `current` already records what materializing `wanted` would
/// produce.
///
/// The current lockfile keeps only what the importers reach
/// ([`crate::filter_lockfile_for_current`]), so a wanted lockfile carrying a
/// snapshot no importer reaches any more can never equal it. Comparing the
/// same shape both sides is what lets such a tree settle instead of
/// re-materializing on every run.
///
/// The equal case is the common one and answers without building the
/// filtered shape at all.
fn materialized_shape_matches(
    wanted: &Lockfile,
    current: &Lockfile,
    included: pnpm_modules_yaml::IncludedDependencies,
) -> bool {
    if wanted == current {
        return true;
    }
    // A transient skip (a failed optional fetch) prunes the current lockfile
    // further, and its set is not known here. Such a tree simply falls
    // through to materialization, which retries the fetch anyway.
    current
        == &crate::filter_lockfile_for_current(wanted, included, &crate::SkippedSnapshots::new())
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
    pub(crate) tree: crate::install::state_options::ModulesTreeContext<'a>,
    pub(crate) projects: crate::install::state_options::InstallProjectMetadata<'a>,
    pub(crate) verification:
        crate::install::state_options::LockfileVerificationInputs<'a, 'install>,
    pub(crate) write: crate::install::state_options::LockfileWritePolicy,
    pub(super) wanted_lockfile: &'a Lockfile,
    pub(super) modules: &'a pnpm_modules_yaml::ModulesLayout,
    pub(super) supported_architectures:
        Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    pub(super) filtered_install: bool,
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
        context.verification.override_check.take(),
        context.verification.verifiers,
        (context.verification.path, &context.tree.config.cache_dir),
    )
    .await?;
    enforce_recorded_build_policy(&context)?;
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: "Lockfile is up to date, resolution step is skipped".to_string(),
        prefix: context.projects.prefix.to_string(),
    }));
    Reporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: context.projects.prefix.to_string(),
        stage: Stage::ImportingDone,
    }));
    save_merged_wanted_lockfile(
        context.wanted_lockfile,
        context.tree.config,
        context.tree.workspace_root,
        (context.write.synthesized_from_current, context.write.fast_updated, context.write.save),
    )?;
    refresh_up_to_date_workspace(&context)?;
    Reporter::emit(&LogEvent::Summary(SummaryLog {
        level: LogLevel::Debug,
        prefix: context.projects.prefix.to_string(),
    }));
    Ok(())
}
// Verification must reject tampering before this policy can report ignored builds.
pub(super) fn enforce_recorded_build_policy(
    context: &UpToDateInstall<'_, '_>,
) -> Result<(), InstallError> {
    if context.tree.config.strict_dep_builds
        && let Ok(Some(package_names)) =
            unapproved_recorded_ignored_builds(context.modules, context.tree.config)
    {
        return Err(InstallError::IgnoredBuilds { package_names });
    }
    Ok(())
}
pub(super) fn refresh_up_to_date_workspace(
    context: &UpToDateInstall<'_, '_>,
) -> Result<(), InstallError> {
    update_workspace_state(
        context.tree.workspace_root,
        &build_workspace_state::<Host>(
            context.tree.workspace_root,
            context.tree.config,
            context.tree.node_linker,
            context.tree.included,
            context.supported_architectures,
            context.projects.catalogs,
            context.projects.manifests,
            context.filtered_install,
            filesystem_now_ms(context.tree.workspace_root),
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
