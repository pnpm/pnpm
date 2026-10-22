//! The freshness check the install fast path and the verify-deps-before-run
//! gate share. It first rules out the drift the recorded settings and static
//! inputs show, then decides from the content digests the last validation
//! recorded which projects are still proven installed, checks the rest that
//! the gate covers against the lockfile, and records a passing check.

pub(crate) use drift::{InputDrift, ProjectStatus};

use super::{
    LockfileConflictCheckFailure, OptimisticRepeatInstallCheck, Path, PathBuf, WorkspaceState,
    catalogs_cache_matches,
    current_lockfile::CurrentLockfile,
    current_lockfile_file_has_content,
    deps_status::config_dependencies_drifted,
    direct_dependency_link_dangling, filesystem_now_ms_in_modules,
    first_held_project_missing_modules_dir, first_lockfile_requiring_conflict_safe_install,
    first_setting_drift, held_projects_match_lockfile, local_file_blocks_fast_path,
    pnpmfiles_drift, project_structure_matches,
    relocation::{prove_move, rekeyed_validation_now},
    settle::{
        current_lockfile_unusable_with_non_empty_wanted, regenerate_wanted_lockfile_if_missing,
    },
    validation::{FileDigest, Vouched, trusts_every_file},
};
use std::collections::HashSet;

mod drift;

/// What a gate does differently in [`check_freshness`].
pub(super) struct GatePolicy<'a> {
    /// The gate a failed state write is logged under.
    pub(super) gate: Gate,
    /// Workspace-state setting keys the gate ignores when comparing settings.
    pub(super) ignored_settings: &'a [&'static str],
    /// Whether local file dependencies (`file:` specs, overrides, package
    /// extensions) force a reinstall.
    pub(super) check_local_file_deps: bool,
    /// Whether dangling direct dependency symlinks force a reinstall.
    pub(super) check_dangling_direct_links: bool,
    pub(super) missing_wanted_lockfile: MissingWantedLockfile,
    pub(super) newer_patch: NewerPatch,
    /// Whether the content check applies `dedupePeers`.
    pub(super) dedupe_peers: bool,
    pub(super) recorded_groups: RecordedGroups,
}

pub(super) enum Gate {
    Install,
    Run,
}

/// What a gate does when `pnpm-lock.yaml` is missing and the current lockfile
/// can stand in for it.
pub(super) enum MissingWantedLockfile {
    /// Write `pnpm-lock.yaml` from the current lockfile, as a full install
    /// would.
    Restore,
    /// Leave the project untouched and only require the stand-in to exist.
    RequireStandIn,
}

/// What a gate does with a patch file whose content changed since the last
/// validation. An edit in place keeps the `patchedDependencies` entry, so the
/// settings comparison cannot see it, but it changes the patched output.
pub(super) enum NewerPatch {
    /// Report the tree stale.
    Refuse,
    /// Report it current when every patch still hashes to what the lockfile
    /// records.
    CompareHash,
}

/// The dependency groups a refreshed state records.
pub(super) enum RecordedGroups {
    /// The groups this run uses, which the gate compared with the recorded
    /// ones.
    Current,
    /// The recorded groups, for a gate that ignores group drift and must not
    /// overwrite what the last install recorded.
    Kept,
}

/// The projects a gate holds to being installed. Whatever the gate reports
/// must be something the install it leads to can fix: a filtered install
/// leaves every project outside its selection as it is.
#[derive(Clone, Copy)]
pub(crate) enum Scope<'a> {
    /// Every project, for an unfiltered install.
    Workspace,
    /// The selected projects and the workspace projects they depend on, for a
    /// filtered install or a filtered command.
    Selected(&'a [&'a Path]),
    /// An unfiltered command run from the selected projects. Its install
    /// covers the whole workspace, so a project whose recorded install went
    /// stale is held anywhere. A project never installed is held only where
    /// the selection depends on it, so an install filtered to other projects
    /// does not make every command install again.
    Command(&'a [&'a Path]),
}

/// Why [`check_freshness`] cannot report the tree current. Each gate words it
/// for its own audience.
pub(crate) enum Stale {
    /// A filtered state without validation records cannot prove an unfiltered
    /// workspace install current.
    UnrecordedFilteredState,
    /// A changed lockfile contains merge conflict markers or is not a safe
    /// regular file.
    LockfileConflict { lockfile_path: PathBuf, failure: LockfileConflictCheckFailure },
    /// A local file dependency's contents may have changed.
    LocalFileDep(&'static str),
    /// A recorded workspace-state setting differs from the current config,
    /// named by its camelCase workspace-state key.
    SettingChanged(&'static str),
    /// Recorded `configDependencies` differ from the current config.
    ConfigDependencies,
    /// Recorded `catalogs` differ from the current catalogs.
    CatalogsCache,
    /// The workspace project list or a project's name/version changed.
    ProjectStructure,
    /// A held project with dependencies lacks its modules directory, named by
    /// its id.
    MissingModulesDir(String),
    /// A direct dependency link in a project's modules directory is dangling.
    DanglingDirectLink,
    /// `useGitBranchLockfile` is enabled and the branch lockfile is absent.
    MissingBranchLockfile,
    /// A pnpmfile was added, removed, or modified, in pnpm's issue wording.
    Pnpmfile(String),
    /// An input could not be read, so freshness cannot be proven.
    Unprovable,
    /// The wanted lockfile has packages or importer dependencies, but no
    /// current lockfile records an install.
    NothingInstalled,
    /// The current lockfile does not list a project with dependencies, named
    /// by its id.
    ProjectNotInstalled(String),
    /// A tree that moved cannot be reused from where it is now.
    MoveRefused(&'static str),
    /// Neither lockfile exists to check against.
    NoLockfile,
    /// A content check failed.
    Content(&'static str),
}

/// The freshness check for `state`, which `moved` marks as re-keyed by
/// [`super::relocation::relocated_state`].
pub(super) fn check_freshness(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    moved: bool,
    policy: &GatePolicy<'_>,
    scope: Scope<'_>,
) -> Result<(), Stale> {
    check_static_freshness(check, state, policy, scope)?;
    let drift = InputDrift::compute(check, state);
    if moved {
        return settle_moved_tree(check, state, &drift, policy);
    }
    if matches!(drift.lockfile, FileDigest::Unreadable) {
        return Err(Stale::Unprovable);
    }
    match current_lockfile_unusable_with_non_empty_wanted(check) {
        Ok(true) => return Err(Stale::NothingInstalled),
        Ok(false) => {}
        Err(reason) => return Err(Stale::Content(reason)),
    }
    let held = drift.held(check, scope);
    if held.is_empty() && !drift.patches_modified {
        return settle_proven_tree(check, state, &drift, policy, scope);
    }
    // Probed before any content is read, so a file edited while the checks
    // run is newer than the recorded validation.
    let filesystem_now = validation_now(check);
    if drift.patches_modified {
        check_newer_patches(check, policy, &drift.current)?;
    }
    if !held.is_empty() {
        held_projects_match_lockfile(check, state, &held, &drift.current, policy.dedupe_peers)?;
    }
    settle_missing_wanted_lockfile(check, policy, &drift.current)?;
    if drift.lockfile_unchanged() {
        let vouched = drift.proven_dirs(&held);
        record_state_at(check, state, &vouched, policy, filesystem_now);
    }
    Ok(())
}

/// The static drift checks that run before [`InputDrift::compute`] stats any
/// manifest.
fn check_static_freshness(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    policy: &GatePolicy<'_>,
    scope: Scope<'_>,
) -> Result<(), Stale> {
    // A filtered state without validation records names no installed
    // project, and the current lockfile lists the projects that install did
    // not select as if it had installed them.
    if state.filtered_install && state.validation.is_none() && matches!(scope, Scope::Workspace) {
        return Err(Stale::UnrecordedFilteredState);
    }
    if let Some((lockfile_path, failure)) =
        first_lockfile_requiring_conflict_safe_install(check, state.last_validated_timestamp)
    {
        return Err(Stale::LockfileConflict { lockfile_path, failure });
    }
    if policy.check_local_file_deps
        && let Some(reason) = local_file_blocks_fast_path(check)
    {
        return Err(Stale::LocalFileDep(reason));
    }
    check_settings_and_workspace_freshness(check, state, policy, scope)?;
    check_lockfile_inputs_freshness(check, state)
}

fn check_settings_and_workspace_freshness(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    policy: &GatePolicy<'_>,
    scope: Scope<'_>,
) -> Result<(), Stale> {
    let &OptimisticRepeatInstallCheck {
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
    if let Some(setting) = first_setting_drift(
        state,
        config,
        node_linker,
        included,
        supported_architectures,
        policy.ignored_settings,
    ) {
        return Err(Stale::SettingChanged(setting));
    }
    if config_dependencies_drifted(config, state) {
        return Err(Stale::ConfigDependencies);
    }
    if !catalogs_cache_matches(state.settings.catalogs.as_ref(), catalogs) {
        return Err(Stale::CatalogsCache);
    }
    if !project_structure_matches(state, project_manifests) {
        return Err(Stale::ProjectStructure);
    }
    if let Some(id) = first_held_project_missing_modules_dir(check, state, scope) {
        return Err(Stale::MissingModulesDir(id));
    }
    if policy.check_dangling_direct_links && direct_dependency_link_dangling(check) {
        return Err(Stale::DanglingDirectLink);
    }
    Ok(())
}

/// The lockfile and resolution inputs beside the manifests: a missing
/// lockfile the current one may not stand in for, or an edited pnpmfile. An
/// edited patch is left to the content phase's [`NewerPatch`] policy.
fn check_lockfile_inputs_freshness(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
) -> Result<(), Stale> {
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        is_workspace_install,
        ..
    } = check;
    if config.lockfile
        && config.use_git_branch_lockfile
        && !workspace_root.join(config.wanted_lockfile_name()).exists()
    {
        return Err(Stale::MissingBranchLockfile);
    }
    if !is_workspace_install
        && !workspace_root.join(config.wanted_lockfile_name()).exists()
        && !current_lockfile_file_has_content(&config.install_state_dir)
    {
        return Err(Stale::NoLockfile);
    }
    if let Some(issue) = pnpmfiles_drift(workspace_root, config, state) {
        return Err(Stale::Pnpmfile(issue));
    }
    Ok(())
}

/// The phase for a tree whose every held project is still proven installed.
/// It records the inputs as they are now when the last validation could not
/// stand for them, which spares the next run reading them again.
fn settle_proven_tree(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    drift: &InputDrift<'_>,
    policy: &GatePolicy<'_>,
    scope: Scope<'_>,
) -> Result<(), Stale> {
    // A full install reads a changed lockfile even when no project records
    // it.
    if drift.lockfile_changed && matches!(scope, Scope::Workspace) {
        check.lockfile
            .get()
            .map_err(|_| Stale::Content("the wanted lockfile cannot be read or parsed"))?;
    }
    let restored = settle_missing_wanted_lockfile(check, policy, &drift.current)?;
    let vouched = drift.proven_dirs(&[]);
    if drift.lockfile_changed || restored {
        record_state(check, state, &vouched, policy);
    } else if drift.rehashed {
        // A file changed in the same clock tick as this record is read again
        // next time anyway, so writing it would only repeat the write then.
        let new_state = refreshed_state(check, state, &vouched, validation_now(check), policy);
        if new_state.validation.as_ref().is_some_and(trusts_every_file) {
            write_state(check, &new_state, policy);
        }
    }
    Ok(())
}

/// Apply the gate's [`NewerPatch`] policy to a patch whose content changed.
fn check_newer_patches(
    check: &OptimisticRepeatInstallCheck<'_>,
    policy: &GatePolicy<'_>,
    current: &CurrentLockfile<'_>,
) -> Result<(), Stale> {
    match policy.newer_patch {
        NewerPatch::Refuse => Err(Stale::Content("a patch file changed since the last validation")),
        NewerPatch::CompareHash => patches_match_lockfile(check, current).map_err(Stale::Content),
    }
}

/// The phase for a tree whose state was re-keyed to where it is now: it is
/// current once [`prove_move`] holds, and the state is recorded at its new
/// location.
fn settle_moved_tree(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    drift: &InputDrift<'_>,
    policy: &GatePolicy<'_>,
) -> Result<(), Stale> {
    let filesystem_now = rekeyed_validation_now(check, state, drift);
    prove_move(check, drift).map_err(Stale::MoveRefused)?;
    settle_missing_wanted_lockfile(check, policy, &drift.current)?;
    let every_project = drift.stats
        .iter()
        .map(|stat| stat.root_dir.to_path_buf())
        .collect();
    record_state_at(check, state, &every_project, policy, filesystem_now);
    Ok(())
}

pub(super) fn validation_now(check: &OptimisticRepeatInstallCheck<'_>) -> Option<i64> {
    filesystem_now_ms_in_modules(&check.config.workspace_state_modules_dir(check.workspace_root))
}

/// Apply the gate's [`MissingWantedLockfile`] policy, returning whether it
/// wrote the wanted lockfile.
fn settle_missing_wanted_lockfile(
    check: &OptimisticRepeatInstallCheck<'_>,
    policy: &GatePolicy<'_>,
    current: &CurrentLockfile<'_>,
) -> Result<bool, Stale> {
    match policy.missing_wanted_lockfile {
        MissingWantedLockfile::Restore => {
            regenerate_wanted_lockfile_if_missing(check, current).map_err(Stale::Content)
        }
        MissingWantedLockfile::RequireStandIn => {
            missing_wanted_lockfile_stand_in_ok(check, current).map(|()| false)
        }
    }
}

/// The current lockfile can stand in for a missing wanted one. Nothing is
/// written.
fn missing_wanted_lockfile_stand_in_ok(
    check: &OptimisticRepeatInstallCheck<'_>,
    current: &CurrentLockfile<'_>,
) -> Result<(), Stale> {
    if check.lockfile.is_loaded_or_on_disk() || !check.config.lockfile {
        return Ok(());
    }
    if check.config.use_git_branch_lockfile {
        return Err(Stale::MissingBranchLockfile);
    }
    match current.get() {
        Some(Some(_)) => Ok(()),
        Some(None) => Err(Stale::NoLockfile),
        None => Err(Stale::Content("the current lockfile cannot be loaded")),
    }
}

/// Whether the configured patch files still hash to what the lockfile
/// records. A missing lockfile passes here and is left to the
/// [`MissingWantedLockfile`] policy.
fn patches_match_lockfile(
    check: &OptimisticRepeatInstallCheck<'_>,
    current: &CurrentLockfile<'_>,
) -> Result<(), &'static str> {
    const PATCHES_MODIFIED: &str = "Patches were modified";
    let hashes = check.config.patched_dependency_hashes().map_err(|_| PATCHES_MODIFIED)?;
    let lockfile = match check.lockfile.get() {
        Ok(Some(wanted)) => wanted,
        Ok(None) => match current.get() {
            Some(Some(current)) => current,
            Some(None) => return Ok(()),
            None => return Err("the current lockfile cannot be loaded"),
        },
        Err(_) => return Err("the wanted lockfile cannot be read or parsed"),
    };
    let empty = std::collections::BTreeMap::new();
    if lockfile.patched_dependencies.as_ref().unwrap_or(&empty) == hashes.as_ref().unwrap_or(&empty)
    {
        Ok(())
    } else {
        Err(PATCHES_MODIFIED)
    }
}

/// [`record_state_at`] at the filesystem's current time.
fn record_state(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    vouched: &HashSet<PathBuf>,
    policy: &GatePolicy<'_>,
) {
    record_state_at(check, state, vouched, policy, validation_now(check));
}

/// Record a passing check at `filesystem_now`, vouching for the `vouched`
/// projects.
fn record_state_at(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    vouched: &HashSet<PathBuf>,
    policy: &GatePolicy<'_>,
    filesystem_now: Option<i64>,
) {
    let new_state = refreshed_state(check, state, vouched, filesystem_now, policy);
    write_state(check, &new_state, policy);
}

/// Write `new_state`. A failed write only costs the next run a repeat of the
/// check.
fn write_state(
    check: &OptimisticRepeatInstallCheck<'_>,
    new_state: &WorkspaceState,
    policy: &GatePolicy<'_>,
) {
    let modules_dir = check.config.workspace_state_modules_dir(check.workspace_root);
    let Err(error) =
        pnpm_workspace_state::update_workspace_state_in_modules(&modules_dir, new_state)
    else {
        return;
    };
    // `tracing` takes the target as a literal.
    match policy.gate {
        Gate::Install => tracing::warn!(
            target: "pacquet::install",
            ?error,
            "Failed to refresh the workspace state after the repeat-install content check",
        ),
        Gate::Run => tracing::warn!(
            target: "pacquet::run",
            ?error,
            "Failed to refresh the workspace state after the verify-deps-before-run content check",
        ),
    }
}

/// `state` validated again at `filesystem_now`. Nothing was materialized, so
/// only the `vouched` projects are proven; the others keep their records.
fn refreshed_state(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    vouched: &HashSet<PathBuf>,
    filesystem_now: Option<i64>,
    policy: &GatePolicy<'_>,
) -> WorkspaceState {
    crate::install::recorded_workspace_state(&crate::install::StateWrite {
        workspace_root: check.workspace_root,
        config: check.config,
        layout: check.layout,
        catalogs: check.catalogs,
        project_manifests: check.project_manifests,
        filesystem_now_ms: filesystem_now,
        carried: crate::install::CarriedSettings {
            established_auto_dedupe: state.settings.auto_dedupe,
            kept_groups: matches!(policy.recorded_groups, RecordedGroups::Kept).then_some(
                &state.settings,
            ),
        },
        vouched: &Vouched::Projects { dirs: vouched, previous: Some(state) },
    })
}
