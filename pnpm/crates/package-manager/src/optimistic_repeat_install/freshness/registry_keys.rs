//! Whether a recorded registry-key verification still covers the lockfile.

use super::{
    super::{OptimisticRepeatInstallCheck, WorkspaceState},
    GatePolicy, InputDrift, Stale,
};

/// A recorded registry-key verification covers the lockfile it was made for,
/// and a passing check carries it forward. A changed lockfile with outdated
/// keys sends an install on to re-key them. A gate that ignores
/// `registryPrefixes` passes, and gets `state` without the verification to
/// record.
pub(super) fn state_unverified_by_changed_lockfile(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    drift: &InputDrift<'_>,
    policy: &GatePolicy<'_>,
) -> Result<Option<WorkspaceState>, Stale> {
    if !drift.lockfile_changed || state.settings.registry_prefixes.is_none() {
        return Ok(None);
    }
    let lockfile = check.lockfile
        .get()
        .map_err(|_| Stale::Content("the wanted lockfile cannot be read or parsed"))?;
    let keys_current = lockfile.is_none_or(|lockfile| {
        crate::install::registry_keys_current(check.config, Some(lockfile))
    });
    if keys_current {
        return Ok(None);
    }
    if !policy.ignored_settings.contains(&"registryPrefixes") {
        return Err(Stale::Content("the changed lockfile records outdated registry keys"));
    }
    let mut unverified = state.clone();
    unverified.settings.registry_prefixes = None;
    Ok(Some(unverified))
}
