use super::super::{
    Host, InstallError, LogEvent, LogLevel, OptimisticRepeatInstallCheck,
    OptimisticRepeatInstallDecision, PnpmLog, Reporter, UpdateSeedPolicy,
    check_optimistic_repeat_install, gvs_build_marker_present,
    gvs_build_markers_may_require_recovery, unapproved_recorded_ignored_builds,
};

/// Everything the optimistic repeat-install short-circuit consults.
pub(super) struct UpToDateCheck<'a> {
    pub(super) workspace: OptimisticRepeatInstallCheck<'a>,
    pub(super) mutation: crate::ProjectMutation,
    pub(super) update_seed_policy: &'a UpdateSeedPolicy,
    pub(super) frozen_lockfile: bool,
    pub(super) disable_optimistic_repeat_install: bool,
    pub(super) effective_node_version: Option<&'a str>,
    pub(super) prefix: &'a str,
}
/// Whether nothing has changed since the previous successful install
/// (settings, workspace structure, manifest mtimes), so the whole pipeline can
/// be skipped and pnpm's "Already up to date" log emitted. The fast path runs
/// before any of the install setup (no lockfile reads, no verifier fan-out, no
/// `getContext`).
///
/// Only a full `pacquet install` may short-circuit. `add` and `remove` mutate
/// the manifest in memory and persist it after this run returns, so the
/// on-disk mtimes the check reads still describe the pre-mutation project —
/// without this gate a fresh workspace state would read as "nothing changed →
/// already up to date" and the mutation would never be resolved or
/// materialized. `pacquet update` is excluded through its seed policy: a
/// compatible bump leaves the manifest byte-identical, which the check would
/// likewise read as up to date and skip the registry re-resolution. Disabled
/// under `--frozen-lockfile`: an explicit headless install should always go
/// through the dispatch so a `NoLockfile` or `OutdatedLockfile` error still
/// fires when the lockfile is missing or stale.
///
/// A `--filter` narrowing does not disqualify the run: the check validates the
/// whole workspace (`project_manifests` covers every project even when only a
/// subset is selected), and it refuses a workspace state a filtered install
/// wrote, so "nothing changed" still means every selected project is
/// materialized.
pub(super) fn install_is_already_up_to_date<Reporter: self::Reporter>(
    check: &UpToDateCheck<'_>,
) -> Result<bool, InstallError> {
    let decided = check.mutation.is_full_install()
        && matches!(check.update_seed_policy, UpdateSeedPolicy::KeepAll)
        && !check.frozen_lockfile
        && !check.workspace.config.force
        && !check.disable_optimistic_repeat_install
        && check_optimistic_repeat_install(&check.workspace)
            == OptimisticRepeatInstallDecision::UpToDate;
    if !decided {
        return Ok(false);
    }
    if !build_state_allows_short_circuit(check)? {
        return Ok(false);
    }
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: "Already up to date".to_string(),
        prefix: check.prefix.to_string(),
    }));
    Ok(true)
}
/// Whether the recorded build state lets the fast path stand.
///
/// A build marker lives in the shared slot, outside every project-state input
/// the repeat check reads. And `strictDepBuilds` stays enforced across reruns:
/// an install that already recorded unapproved ignored builds must keep
/// failing until they are approved, not exit 0 via the fast path. An
/// `allowBuilds` change that newly permits one is already caught by
/// `settings_match` (the policy is part of the workspace state), which reports
/// drift and skips this branch, so the full install runs and rebuilds it.
///
/// A corrupt / unreadable `.modules.yaml` can't prove there are no recorded
/// ignored builds, so under strict mode the fast path is refused rather than
/// short-circuiting on a swallowed read error.
pub(super) fn build_state_allows_short_circuit(
    check: &UpToDateCheck<'_>,
) -> Result<bool, InstallError> {
    if gvs_build_markers_may_require_recovery(check.workspace.config) {
        match check.workspace.lockfile.get() {
            Ok(Some(wanted)) => {
                if gvs_build_marker_present(
                    wanted,
                    check.workspace.config,
                    check.workspace.workspace_root,
                    check.effective_node_version,
                ) {
                    return Ok(false);
                }
            }
            Ok(None) => {}
            Err(_) => return Ok(false),
        }
    }
    if !check.workspace.config.strict_dep_builds {
        return Ok(true);
    }
    match pnpm_modules_yaml::read_modules_layout::<Host>(&check.workspace.config.modules_dir) {
        Ok(Some(modules)) => {
            match unapproved_recorded_ignored_builds(&modules, check.workspace.config) {
                Ok(Some(package_names)) => Err(InstallError::IgnoredBuilds { package_names }),
                Ok(None) => Ok(true),
                // Unreadable state or a malformed `allowBuilds`: can't trust the
                // fast path, run the full install.
                Err(_) => Ok(false),
            }
        }
        Ok(None) => Ok(true),
        Err(_) => Ok(false),
    }
}
