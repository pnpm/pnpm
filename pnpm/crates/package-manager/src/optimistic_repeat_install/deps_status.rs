//! The pre-run dependency-status check behind `verifyDepsBeforeRun`.

use super::{
    Config, Host, Lockfile, LockfileConflictCheckFailure, ManifestStat, NodeLinker,
    OptimisticRepeatInstallCheck, WorkspaceState, catalogs_cache_matches,
    current_lockfile_file_has_content, current_lockfile_unusable_with_non_empty_wanted,
    filesystem_now_ms, first_lockfile_requiring_conflict_safe_install,
    first_project_missing_modules_dir, first_setting_drift, modified_at_or_after,
    modified_manifests_match_lockfile, patches_modified_since, pnpmfiles_drift,
    project_structure_matches, stat_manifests, update_workspace_state, wanted_lockfile_modified,
};

/// Outcome of [`check_deps_status_before_run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunDepsStatus {
    UpToDate,
    /// `node-linker: pnp` installs cannot be inspected. The caller
    /// warns ("verify-deps-before-run does not work with
    /// node-linker=pnp") and runs the script.
    SkippedPnp,
    Outdated {
        /// pnpm's issue wording for the detected drift, shown by the
        /// `warn` and `error` actions.
        issue: String,
        /// `pnpm install` arguments reproducing the dependency groups
        /// the workspace state recorded (`--prod` / `--dev` /
        /// `--no-optional`), for the `install` and `prompt` actions.
        install_args: Vec<String>,
    },
}

/// The verify-deps-before-run twin of
/// [`crate::optimistic_repeat_install::check_optimistic_repeat_install`]: the same freshness checks, with
/// the differences pnpm's run gate carries over its install fast path —
/// it runs regardless of `optimisticRepeatInstall`, never treats local
/// file dependencies as outdated, ignores `dev`/`optional`/`production`
/// drift (scripts always run with the default groups), compares
/// configuration dependencies, and reports drift with pnpm's
/// user-facing issue wording instead of a diagnostic-only reason.
/// `state` arrives from the caller, which already had to load it to
/// decide whether a check is possible at all (a missing state is
/// "Cannot check whether dependencies are outdated").
pub fn check_deps_status_before_run(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
) -> RunDepsStatus {
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        node_linker,
        included,
        supported_architectures,
        project_manifests,
        is_workspace_install,
        catalogs,
        ..
    } = check;

    let install_args = install_args_from_state(state);
    let outdated =
        |issue: String| RunDepsStatus::Outdated { issue, install_args: install_args.clone() };

    if node_linker == NodeLinker::Pnp {
        return RunDepsStatus::SkippedPnp;
    }

    if let Some((lockfile_path, failure)) =
        first_lockfile_requiring_conflict_safe_install(check, state.last_validated_timestamp)
    {
        let lockfile_dir = lockfile_path.parent().unwrap_or(workspace_root).display();
        let issue = match failure {
            LockfileConflictCheckFailure::MergeConflict => {
                format!("The lockfile in {lockfile_dir} has merge conflicts")
            }
            LockfileConflictCheckFailure::Unsafe => {
                format!("The lockfile in {lockfile_dir} cannot be checked for merge conflicts")
            }
        };
        return outdated(issue);
    }

    if let Some(setting) = first_setting_drift(
        state,
        config,
        node_linker,
        included,
        supported_architectures,
        &["dev", "optional", "production"],
    ) {
        return outdated(format!("The value of the {setting} setting has changed"));
    }
    if config_dependencies_drifted(config, state) {
        return outdated("Configuration dependencies are not up to date".to_string());
    }
    if !catalogs_cache_matches(state.settings.catalogs.as_ref(), catalogs) {
        return outdated("Catalogs cache outdated".to_string());
    }
    if !project_structure_matches(state, project_manifests) {
        return outdated("The workspace structure has changed since last install".to_string());
    }
    // A filtered install legitimately leaves unselected projects
    // without a modules directory.
    if !state.filtered_install
        && let Some(id) = first_project_missing_modules_dir(config, node_linker, project_manifests)
    {
        return outdated(format!(
            "Workspace package {id} has dependencies but does not have a modules directory",
        ));
    }
    if !is_workspace_install
        && !workspace_root.join(config.wanted_lockfile_name()).exists()
        && !current_lockfile_file_has_content(&config.virtual_store_dir)
    {
        return outdated(format!("Cannot find a lockfile in {}", workspace_root.display()));
    }
    if patches_modified_since(workspace_root, config, state.last_validated_timestamp) {
        return outdated("Patches were modified".to_string());
    }
    if let Some(issue) =
        pnpmfiles_drift(workspace_root, config, &state.pnpmfiles, state.last_validated_timestamp)
    {
        return outdated(issue);
    }

    let Some(manifest_stats) = stat_manifests(project_manifests) else {
        return outdated("Cannot check whether dependencies are outdated".to_string());
    };
    let modified: Vec<&ManifestStat<'_>> = manifest_stats
        .iter()
        .filter(|stat| modified_at_or_after(stat.mtime, state.last_validated_timestamp))
        .collect();
    let lockfile_modified =
        wanted_lockfile_modified(workspace_root, config, state.last_validated_timestamp);

    match current_lockfile_unusable_with_non_empty_wanted(check) {
        Ok(true) => {
            return outdated(
                "The lockfile requires dependencies but none were installed".to_string(),
            );
        }
        Ok(false) => {}
        Err(reason) => return outdated(reason.to_string()),
    }

    if modified.is_empty() && !lockfile_modified {
        return match missing_wanted_lockfile_stand_in_ok(check) {
            Ok(()) => RunDepsStatus::UpToDate,
            Err(reason) => outdated(reason),
        };
    }

    let projects_to_check: Vec<&ManifestStat<'_>> =
        if lockfile_modified { manifest_stats.iter().collect() } else { modified };
    let filesystem_now =
        if is_workspace_install { filesystem_now_ms(workspace_root) } else { None };
    // The TypeScript run/exec handler does not forward `dedupePeers`
    // into `checkDepsStatus`, so its pre-run lockfile check uses the
    // false default even when the workspace setting is true.
    match modified_manifests_match_lockfile(check, state, &projects_to_check, false) {
        Ok(_) => {
            if let Err(reason) = missing_wanted_lockfile_stand_in_ok(check) {
                return outdated(reason);
            }
            if is_workspace_install {
                let mut new_state = crate::install::build_workspace_state::<Host>(
                    workspace_root,
                    config,
                    node_linker,
                    included,
                    supported_architectures,
                    catalogs,
                    project_manifests,
                    state.filtered_install,
                    filesystem_now,
                );
                // The gate ignored `dev`/`optional`/`production` drift
                // above; writing today's (default-group) values here
                // would clobber what the last real install recorded and
                // flip its next repeat-install check into "drift".
                new_state.settings.dev = state.settings.dev;
                new_state.settings.optional = state.settings.optional;
                new_state.settings.production = state.settings.production;
                if let Err(error) = update_workspace_state(workspace_root, &new_state) {
                    tracing::warn!(
                        target: "pacquet::run",
                        ?error,
                        "Failed to refresh the workspace state after the verify-deps-before-run content check",
                    );
                }
            }
            RunDepsStatus::UpToDate
        }
        Err(reason) => outdated(reason.to_string()),
    }
}

/// Read-only twin of [`crate::optimistic_repeat_install::regenerate_wanted_lockfile_if_missing`] for the
/// run gate: pnpm's run-path check never writes `pnpm-lock.yaml` (only
/// the install command restores it from the current lockfile), so a
/// missing wanted lockfile passes exactly when the current lockfile can
/// stand in for it, and the check leaves the workspace untouched.
pub(crate) fn missing_wanted_lockfile_stand_in_ok(
    check: &OptimisticRepeatInstallCheck<'_>,
) -> Result<(), String> {
    if check.lockfile.is_loaded_or_on_disk() || !check.config.lockfile {
        return Ok(());
    }
    match Lockfile::load_current_from_virtual_store_dir(&check.config.virtual_store_dir) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(format!("Cannot find a lockfile in {}", check.workspace_root.display())),
        Err(_) => Err("the current lockfile cannot be loaded".to_string()),
    }
}

/// The `pnpm install` arguments reproducing the dependency groups the
/// workspace state recorded, so the `install` / `prompt` actions rerun
/// the same kind of install the project last had (pnpm's
/// `createInstallArgs`).
pub(crate) fn install_args_from_state(state: &WorkspaceState) -> Vec<String> {
    let settings = &state.settings;
    let mut args = Vec::new();
    let dev = settings.dev.unwrap_or(false);
    let production = settings.production.unwrap_or(false);
    if production && !dev {
        args.push("--prod".to_string());
    } else if dev && !production {
        args.push("--dev".to_string());
    }
    if !settings.optional.unwrap_or(false) {
        args.push("--no-optional".to_string());
    }
    args
}

/// Whether the configuration dependencies recorded by the last install
/// differ from today's config. Both sides read an absent map as empty
/// (pnpm compares `opts.configDependencies ?? {}` against
/// `workspaceState.configDependencies ?? {}`).
pub(crate) fn config_dependencies_drifted(config: &Config, state: &WorkspaceState) -> bool {
    if config.config_dependencies.is_none() && state.config_dependencies.is_none() {
        return false;
    }
    let empty = std::collections::BTreeMap::new();
    config.config_dependencies.as_ref().unwrap_or(&empty)
        != state.config_dependencies.as_ref().unwrap_or(&empty)
}
