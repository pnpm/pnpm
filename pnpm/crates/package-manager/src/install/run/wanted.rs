use super::{
    super::{
        Arc,
        FastUpdateLockfileOptions,
        FreshnessScope,
        InstallError,
        Lockfile,
        PackageManifest,
        Path,
        PathBuf,
        Reporter,
        check_lockfile_freshness,
        lockfile_freshness::LockfileFreshnessInputs,
        prune_merged_branch_lockfile,
        try_fast_update_lockfile,
    },
    InstallView,
    RunMode,
    lockfile_load::Loaded,
    manifest_freshness_inputs,
    workspace::{
        InstallScope,
        InstallWorkspace,
    },
};
use pnpm_config::Config;

/// The wanted lockfile as the run resolves against it: the loaded one, or
/// what synthesis, the branch fold and the fast update made of it. Each
/// later layer stands in for the ones below when it applied.
pub(super) struct WantedLockfile<'a> {
    /// The on-disk `pnpm-lock.yaml`, the dry-run diff's baseline.
    pub(super) loaded: Option<&'a Lockfile>,
    synthesized: Option<Lockfile>,
    merged_branch: Option<Lockfile>,
    fast_updated: Option<Lockfile>,
}
impl WantedLockfile<'_> {
    pub(super) fn get(&self) -> Option<&Lockfile> {
        self.fast_updated
            .as_ref()
            .or(self.merged_branch.as_ref())
            .or(self.synthesized.as_ref())
            .or(self.loaded)
    }

    pub(super) fn synthesized_from_current(&self) -> bool {
        self.synthesized.is_some()
    }

    /// The loader's `Arc` handle, forwarded only while the loaded document
    /// still is the wanted lockfile: synthesis, the branch fold and the
    /// fast update each replace it, and a consumer must never seed from
    /// a superseded document.
    pub(super) fn loader_handle(&self, shared: Option<Arc<Lockfile>>) -> Option<Arc<Lockfile>> {
        shared.filter(|shared| {
            self.get()
                .is_some_and(|lockfile| std::ptr::eq(lockfile, Arc::as_ptr(shared)))
        })
    }

    pub(super) fn was_fast_updated(&self) -> bool {
        self.fast_updated.is_some()
    }
}
/// The wanted lockfile settled, with the manifests the freshness gates
/// compare it against.
pub(super) struct Lockfiles<'a> {
    pub(super) wanted: WantedLockfile<'a>,
    pub(super) manifest_freshness_inputs: Vec<(String, &'a PackageManifest)>,
}
impl<'a> Lockfiles<'a> {
    fn new(
        loaded: Option<&'a Lockfile>,
        workspace_root: &Path,
        project_manifests: &[(PathBuf, &'a PackageManifest)],
        selection: Option<&crate::WorkspaceInstallSelection<'_>>,
    ) -> Self {
        Self {
            wanted: WantedLockfile {
                loaded,
                synthesized: None,
                merged_branch: None,
                fast_updated: None,
            },
            manifest_freshness_inputs: manifest_freshness_inputs(
                workspace_root,
                project_manifests,
                selection,
            ),
        }
    }
}
pub(super) async fn settle_wanted_lockfile<'a: 'w, 'w, Reporter: self::Reporter + 'static>(
    install: InstallView<'a>,
    mode: &RunMode,
    workspace: &InstallWorkspace<'_>,
    scope: &InstallScope<'_>,
    loaded: &Loaded<'a>,
    project_manifests: &'w [(PathBuf, &'w PackageManifest)],
    selection: Option<&crate::WorkspaceInstallSelection<'_>>,
) -> Result<Lockfiles<'w>, InstallError> {
    let mut lockfiles = Lockfiles::new(
        loaded.wanted.lockfile,
        &workspace.dirs.workspace_root,
        project_manifests,
        selection,
    );
    lockfiles.wanted.synthesized = synthesize_wanted(
        install,
        mode,
        workspace,
        scope,
        loaded,
        &lockfiles.manifest_freshness_inputs,
    )
    .await;
    reconcile_branch_lockfile(&mut lockfiles, loaded, install.context.config);
    if may_fast_update_with_hooks(install, mode, loaded, lockfiles.wanted.get()).await? {
        lockfiles.wanted.fast_updated =
            try_fast_update_lockfile::<Reporter>(FastUpdateLockfileOptions {
                lockfile: lockfiles.wanted.get(),
                project_manifests,
                freshness: LockfileFreshnessInputs {
                    lockfile_dir: &workspace.dirs.workspace_root,
                    manifests: &lockfiles.manifest_freshness_inputs,
                    workspace_packages: workspace.workspace_packages.as_ref(),
                    config: install.context.config,
                    catalogs: &workspace.catalogs,
                    pnpmfile_hook: loaded.pnpmfile_hook.as_ref(),
                    scope: FreshnessScope {
                        ignore_manifest_check: install.lockfile_policy.ignore_manifest_check,
                        prune_stale_importers: scope.prune_stale_importers,
                        allow_missing_dependency_free_importers: true,
                    },
                },
            })
            .await;
    }
    Ok(lockfiles)
}
// Every subsequent freshness check and write must use the reconciled branch fold.
pub(super) fn reconcile_branch_lockfile(
    lockfiles: &mut Lockfiles<'_>,
    loaded: &Loaded<'_>,
    config: &Config,
) {
    lockfiles.wanted.merged_branch = loaded.wanted.pre_merge_importers
        .zip(lockfiles.wanted.get())
        .and_then(|(pre_merge_importers, lockfile)| {
            prune_merged_branch_lockfile(
                lockfile,
                pre_merge_importers,
                &lockfiles.manifest_freshness_inputs,
                config.auto_install_peers,
            )
        });
}
// A current snapshot that still satisfies manifests can regenerate an absent wanted lockfile.
pub(super) async fn synthesize_wanted(
    install: InstallView<'_>,
    mode: &RunMode,
    workspace: &InstallWorkspace<'_>,
    scope: &InstallScope<'_>,
    loaded: &Loaded<'_>,
    manifest_freshness_inputs: &[(String, &PackageManifest)],
) -> Option<Lockfile> {
    synthesize_lockfile_from_current(
        loaded.current.as_ref(),
        SynthesizeScope {
            lockfile_is_absent: loaded.wanted.lockfile.is_none(),
            frozen_lockfile: install.lockfile_policy.frozen,
            prefer_frozen_lockfile: mode.prefer_frozen_lockfile,
            freshness: LockfileFreshnessInputs {
                lockfile_dir: &workspace.dirs.workspace_root,
                manifests: manifest_freshness_inputs,
                workspace_packages: workspace.workspace_packages.as_ref(),
                config: install.context.config,
                catalogs: &workspace.catalogs,
                pnpmfile_hook: loaded.pnpmfile_hook.as_ref(),
                scope: FreshnessScope {
                    ignore_manifest_check: install.lockfile_policy.ignore_manifest_check,
                    prune_stale_importers: scope.prune_stale_importers,
                    allow_missing_dependency_free_importers: true,
                },
            },
        },
    )
    .await
}
/// What decides whether the current lockfile may stand in for a missing
/// `pnpm-lock.yaml`.
pub(super) struct SynthesizeScope<'a> {
    lockfile_is_absent: bool,
    frozen_lockfile: bool,
    prefer_frozen_lockfile: bool,
    freshness: LockfileFreshnessInputs<'a, 'a>,
}
/// Synthesize the wanted lockfile from `<virtual_store_dir>/lock.yaml` when
/// `pnpm-lock.yaml` is absent and the materialized snapshot still satisfies
/// the manifest. The install then skips resolution and regenerates
/// `pnpm-lock.yaml` from the synthesized object.
pub(super) async fn synthesize_lockfile_from_current(
    current_lockfile: Option<&Lockfile>,
    scope: SynthesizeScope<'_>,
) -> Option<Lockfile> {
    let current = current_lockfile?;
    if !scope.lockfile_is_absent || scope.frozen_lockfile || !scope.prefer_frozen_lockfile {
        return None;
    }
    check_lockfile_freshness(current, &scope.freshness).await.ok().map(|()| current.clone())
}

/// Whether the fast update may run for this install. It cannot preserve
/// subtrees when an unchecksummed `readPackage` hook may have changed them.
async fn may_fast_update_with_hooks(
    install: InstallView<'_>,
    mode: &RunMode,
    loaded: &Loaded<'_>,
    lockfile: Option<&Lockfile>,
) -> Result<bool, InstallError> {
    if !may_fast_update_lockfile(install, mode.prefer_frozen_lockfile, loaded.wanted.had_conflicts)
    {
        return Ok(false);
    }
    let Some(lockfile) = lockfile else { return Ok(false) };
    let current = pnpm_hooks::untracked_read_package_hook(loaded.pnpmfile_hook.as_ref())
        .await
        .map_err(InstallError::ReadPackageHook)?;
    Ok(!crate::install::untracked_read_package_hook_may_have_changed(
        lockfile.untracked_pnpmfile_read_package_hook(),
        current,
    ))
}

/// A lockfile whose Git conflict markers were merged away has to be
/// written back, and only a resolution writes it, so the fast update is
/// off the table for it — as it is for every other input the update
/// cannot account for.
pub(super) fn may_fast_update_lockfile(
    install: InstallView<'_>,
    prefer_frozen_lockfile: bool,
    lockfile_had_conflicts: bool,
) -> bool {
    !lockfile_had_conflicts
        && !install.lockfile_policy.frozen
        && !install.execution.dry_run
        && prefer_frozen_lockfile
        && install.execution.mutation.may_fast_update_lockfile()
}
