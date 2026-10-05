//! The pre-run dependency-status check behind `verifyDepsBeforeRun`.

use super::{
    Config, Host, Lockfile, LockfileConflictCheckFailure, ManifestStat, NodeLinker,
    OptimisticRepeatInstallCheck, Path, WorkspaceState, catalogs_cache_matches,
    current_lockfile_file_has_content, current_lockfile_unusable_with_non_empty_wanted,
    filesystem_now_ms_in_modules, first_lockfile_requiring_conflict_safe_install,
    first_project_missing_modules_dir, first_selected_project_missing_from_current_lockfile,
    first_selected_project_missing_modules_dir, first_setting_drift,
    modified_manifests_match_lockfile, patches_modified_since, pnpmfiles_drift,
    project_structure_matches,
    relocation::{prove_move, rekeyed_validation_now, relocated_state},
    wanted_lockfile_file_has_content,
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
///
/// `selected_project_dirs` are the project directories the gated command
/// selected. A state that records a filtered install exempts the projects
/// that install did not select from the modules-directory requirement, but
/// the selected ones are still held to it; an empty selection keeps that
/// exemption for every project.
#[must_use]
pub fn check_deps_status_before_run(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    selected_project_dirs: &[&Path],
) -> RunDepsStatus {
    let install_args = install_args_from_state(state);
    let outdated =
        |issue: String| RunDepsStatus::Outdated { issue, install_args: install_args.clone() };

    if check.layout.node_linker == NodeLinker::Pnp {
        return RunDepsStatus::SkippedPnp;
    }
    let relocated = relocated_state(state, check.workspace_root, check.project_manifests);
    let moved = relocated.is_some();
    let state = relocated.as_ref().unwrap_or(state);
    if let Some(issue) = first_static_drift(check, state, moved, selected_project_dirs) {
        return outdated(issue);
    }

    let Some(drift) = super::ManifestDrift::stat(check, state) else {
        return outdated("Cannot check whether dependencies are outdated".to_string());
    };
    if moved {
        return moved_tree_status(check, state, &drift, &outdated);
    }
    let modified = drift.modified();
    if let Some(status) =
        early_content_verdict(check, &modified, drift.lockfile_modified, &outdated)
    {
        return status;
    }

    let projects_to_check = drift.projects_to_check(modified);
    let filesystem_now = check.is_workspace_install
        .then(|| {
            filesystem_now_ms_in_modules(&check.config.workspace_state_modules_dir(
                check.workspace_root,
            ))
        })
        .flatten();
    // The TypeScript run/exec handler does not forward `dedupePeers`
    // into `checkDepsStatus`, so its pre-run lockfile check uses the
    // false default even when the workspace setting is true.
    match modified_manifests_match_lockfile(check, state, &projects_to_check, false) {
        Ok(_) => match settle_content_check(check, state, filesystem_now) {
            Ok(()) => RunDepsStatus::UpToDate,
            Err(reason) => outdated(reason),
        },
        Err(reason) => outdated(reason.to_string()),
    }
}

/// The gate's verdict on a tree whose `state` was re-keyed by
/// [`relocated_state`]: a tree [`prove_move`] refuses reports the structure
/// change an unrecognized move would.
fn moved_tree_status(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    drift: &super::ManifestDrift<'_>,
    outdated: &impl Fn(String) -> RunDepsStatus,
) -> RunDepsStatus {
    let filesystem_now = rekeyed_validation_now(check, state, drift);
    if prove_move(check, drift).is_err() {
        return outdated(WORKSPACE_STRUCTURE_CHANGED.to_string());
    }
    record_content_check_state(check, state, filesystem_now);
    RunDepsStatus::UpToDate
}

const WORKSPACE_STRUCTURE_CHANGED: &str = "The workspace structure has changed since last install";

/// The first drift the gate can decide before stat-ing any manifest.
fn first_static_drift(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    moved: bool,
    selected_project_dirs: &[&Path],
) -> Option<String> {
    first_lockfile_or_setting_drift(check, state)
        .or_else(|| first_workspace_drift(check, state, moved, selected_project_dirs))
}

fn first_lockfile_or_setting_drift(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
) -> Option<String> {
    let &OptimisticRepeatInstallCheck {
        config,
        catalogs,
        layout:
            crate::RepeatInstallLayout {
                node_linker,
                included,
                supported_architectures,
                ..
            },
        ..
    } = check;
    if let Some(reason) = lockfile_conflict_drift(check, state.last_validated_timestamp) {
        return Some(reason);
    }
    let mut ignored_workspace_state_settings = vec!["dev", "optional", "production"];
    if install_would_run_frozen(check) {
        ignored_workspace_state_settings.push("autoDedupe");
    }
    if let Some(setting) = first_setting_drift(
        state,
        config,
        node_linker,
        included,
        supported_architectures,
        &ignored_workspace_state_settings,
    ) {
        return Some(format!("The value of the {setting} setting has changed"));
    }
    if config_dependencies_drifted(config, state) {
        return Some("Configuration dependencies are not up to date".to_string());
    }
    if !catalogs_cache_matches(state.settings.catalogs.as_ref(), catalogs) {
        return Some("Catalogs cache outdated".to_string());
    }
    None
}

/// Whether the `pnpm install` this gate spawns would run with a frozen
/// lockfile, the way `InstallArgs::resolve_frozen_lockfile` decides it: on CI
/// an install whose lockfile is present and non-empty is frozen unless
/// `preferFrozenLockfile` is off, and an absent or empty lockfile re-resolves
/// either way. A frozen install never re-resolves, so it cannot record the
/// dedupe baseline a pending `autoDedupe` setting asks for: the gate would
/// spawn an install before every script and never settle
/// ([pnpm/pnpm#16374](https://github.com/pnpm/pnpm/issues/16374)).
fn install_would_run_frozen(check: &OptimisticRepeatInstallCheck<'_>) -> bool {
    let config = check.config;
    config.frozen_lockfile.unwrap_or(config.ci && config.prefer_frozen_lockfile)
        && wanted_lockfile_file_has_content(check.workspace_root, config)
}

/// A `moved` tree leaves its patches to the content proof, as the install
/// fast path does.
fn first_workspace_drift(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    moved: bool,
    selected_project_dirs: &[&Path],
) -> Option<String> {
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        project_manifests,
        is_workspace_install,
        ..
    } = check;
    if !project_structure_matches(state, project_manifests) {
        return Some(WORKSPACE_STRUCTURE_CHANGED.to_string());
    }
    if let Some(issue) = first_uninstalled_project(check, state, selected_project_dirs) {
        return Some(issue);
    }
    if !is_workspace_install
        && !workspace_root.join(config.wanted_lockfile_name()).exists()
        && !current_lockfile_file_has_content(&config.install_state_dir)
    {
        return Some(format!("Cannot find a lockfile in {}", workspace_root.display()));
    }
    if !moved && patches_modified_since(workspace_root, config, state.last_validated_timestamp) {
        return Some("Patches were modified".to_string());
    }
    pnpmfiles_drift(workspace_root, config, &state.pnpmfiles, state.last_validated_timestamp)
}

/// The issue for the first project the gated command needs installed that
/// is not.
///
/// A filtered install legitimately leaves the projects it did not select
/// uninstalled, so a state that records one exempts them. The projects the
/// gated command selected are still held to it: without that, a filtered run
/// or exec could select a project the filtered install never materialized and
/// run it without its dependencies
/// (<https://github.com/pnpm/pnpm/issues/11865>). Their modules directories
/// alone do not prove that, so they must also be importers of the current
/// lockfile.
fn first_uninstalled_project(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    selected_project_dirs: &[&Path],
) -> Option<String> {
    let missing_modules_dir = |id: String| {
        format!("Workspace package {id} has dependencies but does not have a modules directory")
    };
    if !state.filtered_install {
        return first_project_missing_modules_dir(check, state).map(missing_modules_dir);
    }
    first_selected_project_missing_modules_dir(check, state, selected_project_dirs)
        .map(missing_modules_dir)
        .or_else(|| {
            first_selected_project_missing_from_current_lockfile(check, selected_project_dirs)
                .map(|id| format!("Workspace package {id} has dependencies but was not installed"))
        })
}

/// The verdict the gate can already reach from what the current lockfile
/// records and whether anything was modified at all.
fn early_content_verdict(
    check: &OptimisticRepeatInstallCheck<'_>,
    modified: &[&ManifestStat<'_>],
    lockfile_modified: bool,
    outdated: &impl Fn(String) -> RunDepsStatus,
) -> Option<RunDepsStatus> {
    match current_lockfile_unusable_with_non_empty_wanted(check) {
        Ok(true) => {
            return Some(outdated(
                "The lockfile requires dependencies but none were installed".to_string(),
            ));
        }
        Ok(false) => {}
        Err(reason) => return Some(outdated(reason.to_string())),
    }
    if modified.is_empty() && !lockfile_modified {
        return Some(match missing_wanted_lockfile_stand_in_ok(check) {
            Ok(()) => RunDepsStatus::UpToDate,
            Err(reason) => outdated(reason),
        });
    }
    None
}

/// Settle the passing content check for a tree in place. A single project
/// uses the lockfile mtimes and leaves its workspace state unchanged.
fn settle_content_check(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    filesystem_now: Option<i64>,
) -> Result<(), String> {
    missing_wanted_lockfile_stand_in_ok(check)?;
    if check.is_workspace_install {
        record_content_check_state(check, state, filesystem_now);
    }
    Ok(())
}

/// Record a passing content check so the next run can use the refreshed
/// timestamp and project locations.
fn record_content_check_state(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    filesystem_now: Option<i64>,
) {
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        project_manifests,
        catalogs,
        layout:
            crate::RepeatInstallLayout {
                node_linker,
                included,
                supported_architectures,
                ..
            },
        ..
    } = check;
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
    new_state.settings.auto_dedupe =
        crate::install::recorded_auto_dedupe(config, state.settings.auto_dedupe);
    // The gate ignored `dev`/`optional`/`production` drift above;
    // writing today's (default-group) values here would clobber what
    // the last real install recorded and flip its next repeat-install
    // check into "drift".
    new_state.settings.dev = state.settings.dev;
    new_state.settings.optional = state.settings.optional;
    new_state.settings.production = state.settings.production;
    refresh_content_check_state(check, &new_state);
}

/// Read-only twin of [`crate::optimistic_repeat_install::regenerate_wanted_lockfile_if_missing`](crate::optimistic_repeat_install::settle::regenerate_wanted_lockfile_if_missing) for the
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
    match Lockfile::load_current_from_install_state_dir(&check.config.install_state_dir) {
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

fn lockfile_conflict_drift(
    check: &OptimisticRepeatInstallCheck<'_>,
    timestamp: i64,
) -> Option<String> {
    if let Some((lockfile_path, failure)) =
        first_lockfile_requiring_conflict_safe_install(check, timestamp)
    {
        let lockfile_dir = lockfile_path
            .parent()
            .unwrap_or(check.workspace_root)
            .display();
        return Some(match failure {
            LockfileConflictCheckFailure::MergeConflict => {
                format!("The lockfile in {lockfile_dir} has merge conflicts")
            }
            LockfileConflictCheckFailure::Unsafe => {
                format!("The lockfile in {lockfile_dir} cannot be checked for merge conflicts")
            }
        });
    }
    None
}

fn refresh_content_check_state(
    check: &OptimisticRepeatInstallCheck<'_>,
    new_state: &WorkspaceState,
) {
    let modules_dir = check.config.workspace_state_modules_dir(check.workspace_root);
    if let Err(error) =
        pnpm_workspace_state::update_workspace_state_in_modules(&modules_dir, new_state)
    {
        tracing::warn!(
            target: "pacquet::run",
            ?error,
            "Failed to refresh the workspace state after the verify-deps-before-run content check",
        );
    }
}
