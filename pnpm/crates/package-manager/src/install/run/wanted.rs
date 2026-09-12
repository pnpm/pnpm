use super::{
    super::{
        Arc, FastUpdateLockfileOptions, FreshnessScope, InstallError, Lockfile, PackageManifest,
        Path, PathBuf, Reporter, check_lockfile_freshness, prune_merged_branch_lockfile,
        try_fast_update_lockfile,
    },
    InstallView, RunMode,
    lockfile_load::Loaded,
    manifest_freshness_inputs,
    workspace::{InstallScope, InstallWorkspace},
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
            self.get().is_some_and(|lockfile| std::ptr::eq(lockfile, Arc::as_ptr(shared)))
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
    let mut lockfiles =
        Lockfiles::new(loaded.lockfile, &workspace.workspace_root, project_manifests, selection);
    lockfiles.wanted.synthesized = synthesize_wanted(
        install,
        mode,
        workspace,
        scope,
        loaded,
        &lockfiles.manifest_freshness_inputs,
    )
    .await;
    reconcile_branch_lockfile(&mut lockfiles, loaded, install.config);
    if may_fast_update_lockfile(
        install.frozen_lockfile,
        install.dry_run,
        mode.prefer_frozen_lockfile,
        install.mutation,
    ) {
        lockfiles.wanted.fast_updated =
            try_fast_update_lockfile::<Reporter>(FastUpdateLockfileOptions {
                lockfile: lockfiles.wanted.get(),
                lockfile_dir: &workspace.workspace_root,
                manifests: &lockfiles.manifest_freshness_inputs,
                project_manifests,
                config: install.config,
                catalogs: &workspace.catalogs,
                pnpmfile_hook: loaded.pnpmfile_hook.as_ref(),
                ignore_manifest_check: install.ignore_manifest_check,
                prune_stale_importers: scope.prune_stale_importers,
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
    lockfiles.wanted.merged_branch = loaded
        .pre_merge_importers
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
            lockfile_is_absent: loaded.lockfile.is_none(),
            frozen_lockfile: install.frozen_lockfile,
            prefer_frozen_lockfile: mode.prefer_frozen_lockfile,
            workspace_root: &workspace.workspace_root,
            manifest_freshness_inputs,
            config: install.config,
            catalogs: &workspace.catalogs,
            pnpmfile_hook: loaded.pnpmfile_hook.as_ref(),
            ignore_manifest_check: install.ignore_manifest_check,
            prune_stale_importers: scope.prune_stale_importers,
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
    workspace_root: &'a Path,
    manifest_freshness_inputs: &'a [(String, &'a PackageManifest)],
    config: &'a Config,
    catalogs: &'a super::super::Catalogs,
    pnpmfile_hook: Option<&'a Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    ignore_manifest_check: bool,
    prune_stale_importers: bool,
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
    check_lockfile_freshness(
        current,
        scope.workspace_root,
        scope.manifest_freshness_inputs,
        scope.config,
        scope.catalogs,
        scope.pnpmfile_hook,
        FreshnessScope {
            ignore_manifest_check: scope.ignore_manifest_check,
            allow_missing_dependency_free_importers: true,
            prune_stale_importers: scope.prune_stale_importers,
        },
    )
    .await
    .ok()
    .map(|()| current.clone())
}
pub(super) fn may_fast_update_lockfile(
    frozen_lockfile: bool,
    dry_run: bool,
    prefer_frozen_lockfile: bool,
    mutation: crate::ProjectMutation,
) -> bool {
    !frozen_lockfile && !dry_run && prefer_frozen_lockfile && mutation.may_fast_update_lockfile()
}
