//! What a gate does when `pnpm-lock.yaml` is missing, per its
//! [`MissingWantedLockfile`] policy.

use super::{
    CurrentLockfile, GatePolicy, MissingWantedLockfile, OptimisticRepeatInstallCheck, Stale,
    stand_in_writes,
};
use crate::optimistic_repeat_install::settle::regenerate_wanted_lockfile_if_missing;

/// Apply the gate's [`MissingWantedLockfile`] policy, returning whether it
/// wrote the wanted lockfile.
pub(super) fn settle_missing_wanted_lockfile(
    check: &OptimisticRepeatInstallCheck<'_>,
    policy: &GatePolicy<'_>,
    current: &CurrentLockfile<'_>,
) -> Result<bool, Stale> {
    match policy.missing_wanted_lockfile {
        MissingWantedLockfile::Restore => {
            stand_in_writes::refuse(
                check,
                check.config.lockfile && !check.lockfile.is_loaded_or_on_disk(),
            )?;
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
