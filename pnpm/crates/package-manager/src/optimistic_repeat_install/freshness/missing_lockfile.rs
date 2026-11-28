//! What a gate does when `pnpm-lock.yaml` is missing, per its
//! [`MissingWantedLockfile`] policy.

use super::{
    CurrentLockfile,
    GatePolicy,
    MissingWantedLockfile,
    OptimisticRepeatInstallCheck,
    Stale,
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
        MissingWantedLockfile::RequireWantedLockfile => {
            require_wanted_lockfile(check).map(|()| false)
        }
    }
}

/// A missing wanted lockfile has to reach an install. Nothing is written by a
/// check that refuses it.
fn require_wanted_lockfile(check: &OptimisticRepeatInstallCheck<'_>) -> Result<(), Stale> {
    if check.config.lockfile && !check.lockfile.is_loaded_or_on_disk() {
        return Err(Stale::NoLockfile);
    }
    Ok(())
}
