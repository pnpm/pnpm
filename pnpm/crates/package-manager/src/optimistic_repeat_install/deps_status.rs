//! The pre-run dependency-status check behind `verifyDepsBeforeRun`.

use super::{
    Config, LockfileConflictCheckFailure, NodeLinker, OptimisticRepeatInstallCheck, Path, Scope,
    WorkspaceState,
    freshness::{
        Gate, GatePolicy, MissingWantedLockfile, NewerPatch, RecordedGroups, Stale, check_freshness,
    },
    relocation::relocated_state,
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
/// drift (scripts always run with the default groups), and reports drift
/// with pnpm's user-facing issue wording instead of a diagnostic-only
/// reason.
/// `state` arrives from the caller, which already had to load it to
/// decide whether a check is possible at all (a missing state is
/// "Cannot check whether dependencies are outdated").
///
/// `selected_project_dirs` are the project directories the gated command
/// selected. A filtered command is held only to the selected projects and the
/// workspace projects they depend on: the install it spawns is filtered too,
/// and can only bring those up to date.
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
    let config = check.config;
    let scope = if config.filter.is_empty() && config.filter_prod.is_empty() {
        Scope::Command(selected_project_dirs)
    } else {
        Scope::Selected(selected_project_dirs)
    };
    let mut ignored_settings = vec!["dev", "optional", "production"];
    if install_would_run_frozen(check) {
        ignored_settings.push("autoDedupe");
    }
    let policy = GatePolicy {
        gate: Gate::Run,
        ignored_settings: &ignored_settings,
        check_local_file_deps: false,
        check_dangling_direct_links: false,
        missing_wanted_lockfile: MissingWantedLockfile::RequireStandIn,
        newer_patch: NewerPatch::CompareHash,
        // The TypeScript run/exec handler does not forward `dedupePeers`
        // into `checkDepsStatus`, so its pre-run lockfile check uses the
        // false default even when the workspace setting is true.
        dedupe_peers: false,
        recorded_groups: RecordedGroups::Kept,
    };
    match check_freshness(check, state, moved, &policy, scope) {
        Ok(()) => RunDepsStatus::UpToDate,
        Err(stale) => outdated(stale_issue(check, stale)),
    }
}

/// pnpm's issue wording for a [`Stale`] verdict.
fn stale_issue(check: &OptimisticRepeatInstallCheck<'_>, stale: Stale) -> String {
    match stale {
        Stale::UnrecordedFilteredState | Stale::Unprovable => {
            "Cannot check whether dependencies are outdated".to_string()
        }
        Stale::LockfileConflict { lockfile_path, failure } => {
            lockfile_conflict_issue(check.workspace_root, &lockfile_path, failure)
        }
        Stale::LocalFileDep(reason) | Stale::Content(reason) => reason.to_string(),
        Stale::SettingChanged(setting) => {
            format!("The value of the {setting} setting has changed")
        }
        Stale::ConfigDependencies => "Configuration dependencies are not up to date".to_string(),
        Stale::CatalogsCache => "Catalogs cache outdated".to_string(),
        Stale::ProjectStructure | Stale::MoveRefused(_) => WORKSPACE_STRUCTURE_CHANGED.to_string(),
        Stale::MissingModulesDir(id) => {
            format!("Workspace package {id} has dependencies but does not have a modules directory")
        }
        Stale::DanglingDirectLink => {
            "a direct dependency link points to a missing target".to_string()
        }
        Stale::MissingBranchLockfile | Stale::NoLockfile => {
            format!("Cannot find a lockfile in {}", check.workspace_root.display())
        }
        Stale::Pnpmfile(issue) => issue,
        Stale::ProjectNotInstalled(id) => {
            format!("Workspace package {id} has dependencies but was not installed")
        }
        Stale::NothingInstalled => {
            "The lockfile requires dependencies but none were installed".to_string()
        }
    }
}

fn lockfile_conflict_issue(
    workspace_root: &Path,
    lockfile_path: &Path,
    failure: LockfileConflictCheckFailure,
) -> String {
    let lockfile_dir = lockfile_path
        .parent()
        .unwrap_or(workspace_root)
        .display();
    match failure {
        LockfileConflictCheckFailure::MergeConflict => {
            format!("The lockfile in {lockfile_dir} has merge conflicts")
        }
        LockfileConflictCheckFailure::Unsafe => {
            format!("The lockfile in {lockfile_dir} cannot be checked for merge conflicts")
        }
    }
}

const WORKSPACE_STRUCTURE_CHANGED: &str = "The workspace structure has changed since last install";

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
