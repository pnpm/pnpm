use super::super::{
    Host, InstallError, LogEvent, LogLevel, OptimisticRepeatInstallCheck,
    OptimisticRepeatInstallDecision, PnpmLog, Reporter, SummaryLog, UpdateSeedPolicy,
    check_optimistic_repeat_install, gvs_build_marker_present,
    gvs_build_markers_may_require_recovery, unapproved_recorded_ignored_builds,
};
use pnpm_config::Config;
use std::path::Path;

/// Everything the optimistic repeat-install short-circuit consults.
pub(super) struct UpToDateCheck<'a> {
    pub(super) workspace: OptimisticRepeatInstallCheck<'a>,
    pub(super) mutation: crate::ProjectMutation,
    pub(super) update_seed_policy: &'a UpdateSeedPolicy,
    pub(super) frozen_lockfile: bool,
    /// `--lockfile-only` or `--dry-run`: nothing is linked, so the project is
    /// not registered in the store.
    pub(super) resolve_only: bool,
    pub(super) disable_optimistic_repeat_install: bool,
    pub(super) effective_node_version: Option<&'a str>,
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
/// likewise read as up to date and skip the registry re-resolution. A
/// `--frozen-lockfile` install never short-circuits: it always goes through
/// the dispatch so a `NoLockfile` or `OutdatedLockfile` error still fires
/// when the lockfile is missing or stale. It only skips the projects' own
/// lifecycle scripts when nothing changed.
///
/// A `--filter` narrowing does not disqualify the run: the check validates the
/// whole workspace (`project_manifests` covers every project even when only a
/// subset is selected), and it refuses a workspace state a filtered install
/// wrote, so "nothing changed" still means every selected project is
/// materialized.
pub(super) fn repeat_install_verdict(
    check: &UpToDateCheck<'_>,
) -> Result<RepeatInstallVerdict, InstallError> {
    if !check.resolve_only {
        register_workspace_in_store(check.workspace.config, check.workspace.workspace_root)?;
    }
    let eligible = check.mutation.is_full_install()
        && matches!(check.update_seed_policy, UpdateSeedPolicy::KeepAll)
        && !check.workspace.config.force
        && !check.disable_optimistic_repeat_install;
    if !eligible {
        return Ok(RepeatInstallVerdict::Changed);
    }
    if let OptimisticRepeatInstallDecision::Skipped { reason } =
        check_optimistic_repeat_install(&check.workspace)
    {
        tracing::debug!(
            target: "pacquet::install",
            reason,
            "repeat-install fast path skipped; running the full install",
        );
        return Ok(RepeatInstallVerdict::Changed);
    }
    if check.frozen_lockfile {
        return Ok(RepeatInstallVerdict::UnchangedFrozen);
    }
    Ok(if build_state_allows_short_circuit(check)? {
        RepeatInstallVerdict::Unchanged
    } else {
        RepeatInstallVerdict::Changed
    })
}
/// What [`repeat_install_verdict`] found.
#[derive(Debug, Clone, Copy)]
pub(super) enum RepeatInstallVerdict {
    /// The install runs in full.
    Changed,
    /// The install short-circuits with "Already up to date".
    Unchanged,
    /// A `--frozen-lockfile` install that changed nothing. It still runs, but
    /// the projects do not run their own lifecycle scripts.
    UnchangedFrozen,
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

/// Whether an embedder's in-memory hooks differ from the ones the wanted
/// lockfile records: a different `pnpmfileChecksum`, or an untracked
/// `readPackage` hook that appeared or went away. The repeat-install check
/// detects a changed pnpmfile by its mtime, which in-memory hooks do not
/// have. An unchanged untracked hook does not count, as an unchanged
/// pnpmfile does not. An unreadable lockfile or hook counts as changed. A
/// missing lockfile leaves nothing to compare.
async fn pnpmfile_hook_override_changed(
    hooks: Option<std::sync::Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    lockfile: pnpm_lockfile::MaybeLazyLockfile<'_>,
) -> bool {
    let Some(hooks) = hooks else { return false };
    let (recorded_checksum, recorded_untracked) = match lockfile.get() {
        Ok(Some(lockfile)) => {
            (lockfile.pnpmfile_checksum.clone(), lockfile.untracked_pnpmfile_read_package_hook())
        }
        Ok(None) => return false,
        Err(_) => return true,
    };
    recorded_checksum != hooks.calculate_pnpmfile_checksum().await
        || hooks
            .untracked_read_package_hook()
            .await
            .map_or(true, |current| current != recorded_untracked)
}

/// Reports a run the repeat-install fast path finished.
pub(super) fn report_already_up_to_date<Reporter: self::Reporter>(
    prefix: String,
) -> super::InstallRunOutcome {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: "Already up to date".to_string(),
        prefix: prefix.clone(),
    }));
    Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));
    super::InstallRunOutcome::AlreadyUpToDate
}

/// Register the workspace root in the store's project registry, once per
/// install, with or without the global virtual store. The repeat-install
/// fast paths call it before they short-circuit, so a project installed
/// unregistered still gets an entry. Store prune walks the workspace's
/// `node_modules` to find every installed package, so one entry per
/// workspace is enough. A frozen store is read-only, but a global virtual
/// store install still registers, best-effort, because prune removes the
/// slots of an unregistered project.
///
/// Best-effort: a registry write failure shouldn't fail the install, so it is
/// surfaced as `tracing::warn!` instead. Loaded installs register the
/// directory holding their store manifest, which prune reads, and require
/// durable registration because their blobs have no project hardlinks to
/// protect them.
/// A frozen store is externally managed and must not receive registry writes.
pub(crate) fn register_workspace_in_store(
    config: &Config,
    workspace_root: &Path,
) -> Result<(), InstallError> {
    if config.frozen_store && config.node_linker == pnpm_config::NodeLinker::Loaded {
        return Ok(());
    }
    if config.node_linker == pnpm_config::NodeLinker::Loaded {
        let loader_dir = config.store_loader_dir(workspace_root);
        std::fs::create_dir_all(&loader_dir)
            .map_err(|error| InstallError::CreateStoreLoaderDir {
                dir: loader_dir.clone(),
                error,
            })?;
        return pnpm_store_dir::register_loaded_project(&config.store_dir, &loader_dir)
            .map_err(InstallError::RegisterLoadedProject);
    }
    if config.frozen_store && !config.enable_global_virtual_store {
        return Ok(());
    }
    // Create the store root before calling `register_project` so its
    // `path_contains` guard can canonicalize the path instead of falling
    // through to a literal comparison that wrongly matches against
    // `<workspace>/../pacquet-store/v11`-shaped relative store paths
    // (resolved-on-disk: outside the workspace; lexical: starts with the
    // workspace prefix).
    if let Err(error) = std::fs::create_dir_all(pnpm_store_dir::StoreDir::root(&config.store_dir)) {
        tracing::warn!(
            target: "pacquet::install",
            ?error,
            "Failed to ensure store root exists before project registry write; install continues",
        );
    }
    if let Err(error) = pnpm_store_dir::register_project(&config.store_dir, workspace_root) {
        tracing::warn!(
            target: "pacquet::install",
            ?error,
            "Failed to register workspace root in the store project registry; install continues",
        );
    }
    Ok(())
}

impl super::RunExecution<'_> {
    /// The repeat-install check for this run. An embedder's changed in-memory
    /// hooks count as a change the check cannot see.
    pub(super) async fn repeat_install_verdict(
        &mut self,
        scope: &super::InstallScope<'_>,
    ) -> Result<RepeatInstallVerdict, InstallError> {
        let embedder_hooks = self.owned.projects.pnpmfile_hook_override.clone();
        self.check_custom_fetcher_reuse().await?;
        let verdict =
            scope.repeat_install_verdict(self.install, &self.owned, &self.mode, &self.workspace)?;
        let changed = match verdict {
            RepeatInstallVerdict::Changed => true,
            RepeatInstallVerdict::Unchanged | RepeatInstallVerdict::UnchangedFrozen => false,
        };
        if changed
            || self.malformed_declaration_blocks_repeat_install(scope)
            || pnpmfile_hook_override_changed(embedder_hooks, self.install.context.lockfile).await
        {
            return Ok(RepeatInstallVerdict::Changed);
        }
        Ok(verdict)
    }
}
