//! A check on manifest stand-ins never writes: what it would write, the
//! workspace state or the wanted lockfile, is derived from the manifest
//! files. It defers to the check on the files instead.

use super::{OptimisticRepeatInstallCheck, Stale};

/// `Err` when the check runs on stand-ins and the step it reached `writes`.
pub(super) fn refuse(check: &OptimisticRepeatInstallCheck<'_>, writes: bool) -> Result<(), Stale> {
    if writes && check.project_manifests.has_stand_ins() {
        return Err(Stale::Content("refreshing the workspace state needs the manifest files"));
    }
    Ok(())
}
