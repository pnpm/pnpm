//! The content phase the install fast path and the verify-deps-before-run
//! gate share. Each gate first rules out the drift its recorded settings and
//! static inputs can show; this phase then decides from the inputs' mtimes,
//! and from their content where one changed, whether the tree is current, and
//! records a passing check.

use super::{
    FileMtime, Host, Lockfile, ManifestFreshness, ManifestStat, OptimisticRepeatInstallCheck,
    WorkspaceState, filesystem_now_ms_in_modules, lockfile_modified_since,
    manifest_drift_reference_ms, modified_manifests_match_lockfile, patches_modified_since,
    relocation::{prove_move, rekeyed_validation_now},
    settle::{
        current_lockfile_unusable_with_non_empty_wanted, regenerate_wanted_lockfile_if_missing,
    },
    stat_manifests, unstatted_manifests, wanted_lockfile_mtime,
};

/// What a gate does differently in the shared content phase.
pub(super) struct GatePolicy {
    pub(super) missing_wanted_lockfile: MissingWantedLockfile,
    /// Whether the content check applies `dedupePeers`.
    pub(super) dedupe_peers: bool,
    pub(super) recorded_groups: RecordedGroups,
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

/// The dependency groups a refreshed state records.
pub(super) enum RecordedGroups {
    /// The groups this run uses, which the gate compared with the recorded
    /// ones.
    Current,
    /// The recorded groups, for a gate that ignores group drift and must not
    /// overwrite what the last install recorded.
    Kept,
}

/// Why the content phase cannot report the tree current. Each gate words it
/// for its own audience.
#[derive(Clone, Copy)]
pub(super) enum Stale {
    /// A manifest could not be stat'd, so freshness cannot be proven.
    Unprovable,
    /// The wanted lockfile has packages but no current lockfile records an
    /// install.
    NothingInstalled,
    /// A tree that moved cannot be reused from where it is now.
    MoveRefused(&'static str),
    /// Neither lockfile exists to check against.
    NoLockfile,
    /// A content check failed.
    Content(&'static str),
}

/// The content phase for `state`, which `moved` marks as re-keyed by
/// [`super::relocation::relocated_state`].
pub(super) fn check_freshness(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    moved: bool,
    policy: &GatePolicy,
) -> Result<(), Stale> {
    let drift = InputDrift::stat(check, state).ok_or(Stale::Unprovable)?;
    if moved {
        return settle_moved_tree(check, state, &drift, policy);
    }
    let modified = drift.modified();
    if modified.is_empty() && !drift.shared_input_modified() {
        require_installed(check)?;
        return settle_missing_wanted_lockfile(check, policy, None);
    }
    // Probed before any content is read, so a file edited while the checks
    // run is newer than the recorded validation. The unchanged tree above
    // records nothing, so it is spared the probe.
    let refresh = drift.refreshes_state(check).then(|| validation_now(check));
    if drift.patches_modified {
        patches_match_lockfile(check).map_err(Stale::Content)?;
    }
    require_installed(check)?;
    let projects_to_check = drift.projects_to_check(modified);
    let loaded_current = if projects_to_check.is_empty() {
        None
    } else {
        modified_manifests_match_lockfile(check, state, &projects_to_check, policy.dedupe_peers)
            .map_err(Stale::Content)?
    };
    let refresh = refresh.filter(|_| drift.lockfile_unchanged(check));
    settle_missing_wanted_lockfile(check, policy, loaded_current)?;
    if let Some(filesystem_now) = refresh {
        record_state(check, state, filesystem_now, policy);
    }
    Ok(())
}

/// A single project whose wanted lockfile has packages must have a current
/// lockfile recording their install.
fn require_installed(check: &OptimisticRepeatInstallCheck<'_>) -> Result<(), Stale> {
    match current_lockfile_unusable_with_non_empty_wanted(check) {
        Ok(true) => Err(Stale::NothingInstalled),
        Ok(false) => Ok(()),
        Err(reason) => Err(Stale::Content(reason)),
    }
}

/// Every input the content phase compares against the last validation, with
/// the mtimes it read them at.
///
/// A passing check refreshes `lastValidatedTimestamp` wherever the next run
/// measures an input against it: always in a workspace, and in a single
/// project once its lockfile or a patch changed. A single project's manifest
/// drift is measured against the lockfile's mtime instead
/// ([#14890](https://github.com/pnpm/pnpm/issues/14890)), so a manifest change
/// alone needs no refresh there.
pub(crate) struct InputDrift<'a> {
    pub(super) stats: Vec<ManifestStat<'a>>,
    /// A lockfile-only change, such as a `git checkout` of just
    /// `pnpm-lock.yaml`, leaves every manifest untouched but still
    /// invalidates the install. A missing lockfile reports `false` here and is
    /// left to the existence and stand-in checks.
    pub(super) lockfile_modified: bool,
    /// Whether a configured patch file is newer than the last validation.
    patches_modified: bool,
    /// Per [`manifest_drift_reference_ms`].
    manifest_reference_ms: i64,
    /// The wanted lockfile's mtime when [`Self::stat`] ran.
    lockfile_mtime: Option<FileMtime>,
}

impl<'a> InputDrift<'a> {
    /// `None` when a manifest cannot be stat'd, which leaves freshness
    /// unprovable. In-memory manifests ([`ManifestFreshness::Content`]) are
    /// not stat'd: each one counts as possibly modified, so the content
    /// re-check covers them all.
    pub(crate) fn stat(
        check: &OptimisticRepeatInstallCheck<'a>,
        state: &WorkspaceState,
    ) -> Option<Self> {
        let lockfile_mtime = wanted_lockfile_mtime(check.workspace_root, check.config);
        let stats = match check.manifests.freshness {
            ManifestFreshness::Mtime => stat_manifests(check.project_manifests)?,
            ManifestFreshness::Content => unstatted_manifests(check.project_manifests),
        };
        Some(Self {
            stats,
            lockfile_modified: lockfile_mtime.is_some_and(|mtime| {
                lockfile_modified_since(mtime, state.last_validated_timestamp)
            }),
            patches_modified: patches_modified_since(
                check.workspace_root,
                check.config,
                state.last_validated_timestamp,
            ),
            manifest_reference_ms: manifest_drift_reference_ms(
                check.config,
                check.is_workspace_install,
                state.last_validated_timestamp,
                lockfile_mtime,
            ),
            lockfile_mtime,
        })
    }

    pub(crate) fn modified(&self) -> Vec<&ManifestStat<'a>> {
        self.stats
            .iter()
            .filter(|stat| stat.possibly_modified_since(self.manifest_reference_ms))
            .collect()
    }

    /// Whether an input every project shares changed.
    fn shared_input_modified(&self) -> bool {
        self.lockfile_modified || self.patches_modified
    }

    /// Whether a passing check refreshes `lastValidatedTimestamp`, per the
    /// rule on [`InputDrift`].
    fn refreshes_state(&self, check: &OptimisticRepeatInstallCheck<'_>) -> bool {
        check.is_workspace_install || self.shared_input_modified()
    }

    /// The projects the content check covers: every one when the lockfile
    /// itself changed, else the modified ones.
    fn projects_to_check<'s>(
        &'s self,
        modified: Vec<&'s ManifestStat<'a>>,
    ) -> Vec<&'s ManifestStat<'a>> {
        if self.lockfile_modified { self.stats.iter().collect() } else { modified }
    }

    /// Whether the wanted lockfile still has the mtime [`Self::stat`] saw.
    /// A content check that passed validated the lockfile it read, so a
    /// lockfile rewritten since must not have its mtime recorded as
    /// validated.
    pub(crate) fn lockfile_unchanged(&self, check: &OptimisticRepeatInstallCheck<'_>) -> bool {
        let mtime_ns = |mtime: Option<FileMtime>| mtime.map(|mtime| mtime.ns);
        mtime_ns(self.lockfile_mtime)
            == mtime_ns(wanted_lockfile_mtime(check.workspace_root, check.config))
    }
}

/// The phase for a tree whose state was re-keyed to where it is now: it is
/// current once [`prove_move`] holds, and the state is recorded at its new
/// location.
fn settle_moved_tree(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    drift: &InputDrift<'_>,
    policy: &GatePolicy,
) -> Result<(), Stale> {
    let filesystem_now = rekeyed_validation_now(check, state, drift);
    prove_move(check, drift).map_err(Stale::MoveRefused)?;
    settle_missing_wanted_lockfile(check, policy, None)?;
    record_state(check, state, filesystem_now, policy);
    Ok(())
}

pub(super) fn validation_now(check: &OptimisticRepeatInstallCheck<'_>) -> Option<i64> {
    filesystem_now_ms_in_modules(&check.config.workspace_state_modules_dir(check.workspace_root))
}

/// Apply the gate's [`MissingWantedLockfile`] policy. `loaded_current` is the
/// current lockfile when the content check already loaded it.
fn settle_missing_wanted_lockfile(
    check: &OptimisticRepeatInstallCheck<'_>,
    policy: &GatePolicy,
    loaded_current: Option<Lockfile>,
) -> Result<(), Stale> {
    match policy.missing_wanted_lockfile {
        MissingWantedLockfile::Restore => {
            regenerate_wanted_lockfile_if_missing(check, loaded_current).map_err(Stale::Content)
        }
        MissingWantedLockfile::RequireStandIn => missing_wanted_lockfile_stand_in_ok(check),
    }
}

/// The current lockfile can stand in for a missing wanted one. Nothing is
/// written.
fn missing_wanted_lockfile_stand_in_ok(
    check: &OptimisticRepeatInstallCheck<'_>,
) -> Result<(), Stale> {
    if check.lockfile.is_loaded_or_on_disk() || !check.config.lockfile {
        return Ok(());
    }
    match Lockfile::load_current_from_install_state_dir(&check.config.install_state_dir) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(Stale::NoLockfile),
        Err(_) => Err(Stale::Content("the current lockfile cannot be loaded")),
    }
}

/// Whether the configured patch files still hash to what the lockfile
/// records. A missing lockfile passes here and is left to the
/// [`MissingWantedLockfile`] policy.
fn patches_match_lockfile(check: &OptimisticRepeatInstallCheck<'_>) -> Result<(), &'static str> {
    const PATCHES_MODIFIED: &str = "Patches were modified";
    let hashes = check.config.patched_dependency_hashes().map_err(|_| PATCHES_MODIFIED)?;
    let mut current = None;
    let lockfile = match check.lockfile.get() {
        Ok(Some(wanted)) => wanted,
        Ok(None) => {
            match Lockfile::load_current_from_install_state_dir(&check.config.install_state_dir) {
                Ok(Some(loaded)) => &*current.insert(loaded),
                Ok(None) => return Ok(()),
                Err(_) => return Err("the current lockfile cannot be loaded"),
            }
        }
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

/// Record a passing check at `filesystem_now`. A failed write only costs the
/// next run a repeat of the check.
fn record_state(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    filesystem_now: Option<i64>,
    policy: &GatePolicy,
) {
    let new_state = refreshed_state(check, state, filesystem_now, policy);
    let modules_dir = check.config.workspace_state_modules_dir(check.workspace_root);
    if let Err(error) =
        pnpm_workspace_state::update_workspace_state_in_modules(&modules_dir, &new_state)
    {
        tracing::warn!(
            target: "pacquet::install",
            ?error,
            "Failed to refresh the workspace state after the dependency content check",
        );
    }
}

/// `state` validated again at `filesystem_now`. Nothing was materialized, so
/// the previous run's `filtered_install` carries forward: clearing it would
/// claim every importer is materialized when a filtered install left the
/// unselected ones untouched.
fn refreshed_state(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    filesystem_now: Option<i64>,
    policy: &GatePolicy,
) -> WorkspaceState {
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
    if matches!(policy.recorded_groups, RecordedGroups::Kept) {
        new_state.settings.dev = state.settings.dev;
        new_state.settings.optional = state.settings.optional;
        new_state.settings.production = state.settings.production;
    }
    new_state
}
