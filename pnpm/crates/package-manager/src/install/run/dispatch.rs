use super::{
    super::{
        Arc, ContextLog, FreshnessCheckError, FreshnessScope, InstallError, InstallRunOptions,
        Lockfile, LogEvent, LogLevel, PackageManifest, Path, PathBuf, PrepareModulesStateInputs,
        PreparedModulesState, Reporter, Stage, StageLog, SummaryLog, check_lockfile_freshness,
        lockfile_freshness::LockfileFreshnessInputs, map_frozen_lockfile_error,
        prepare_modules_state, verify_lockfile_eagerly,
    },
    InstallOwned, InstallView, RunMode, Verification,
    lockfile_load::Loaded,
    manifests::{DevPreinstallScope, run_dev_preinstall_hook},
    reject_frozen_with_update_checksums,
    wanted::Lockfiles,
    workspace::{InstallScope, InstallWorkspace},
};
use pnpm_config::Config;

/// Everything the run has settled before it dispatches.
#[derive(Clone, Copy)]
pub(super) struct Settled<'r, 'a> {
    pub(super) install: InstallView<'a>,
    pub(super) owned: &'r InstallOwned,
    pub(super) mode: &'r RunMode,
    pub(super) loaded: &'r Loaded<'a>,
    pub(super) lockfiles: &'r Lockfiles<'a>,
    pub(super) verification: &'r Verification,
    pub(super) projects: SettledProjects<'r, 'a>,
}

#[derive(Clone, Copy)]
pub(super) struct SettledProjects<'r, 'a> {
    pub(super) workspace: &'r InstallWorkspace<'a>,
    pub(super) scope: &'r InstallScope<'a>,
    pub(super) project_manifests: &'r [(PathBuf, &'a PackageManifest)],
}

/// Which path the install takes, and the modules state it starts from.
pub(super) struct Dispatched<'install> {
    pub(super) take_frozen_path: bool,
    pub(super) modules: PreparedModulesState<'install>,
}
/// Announce the install, run `pnpm:devPreinstall`, and decide between the
/// frozen and the fresh path. Returns `None` when the run is complete: a
/// `--lockfile-only` frozen install, or a modules state found up to date.
/// Consumes the lockfile verification override off `options`.
pub(super) async fn dispatch<'install, Reporter: self::Reporter + 'static>(
    settled: Settled<'_, '_>,
    options: &mut InstallRunOptions<'install, '_>,
) -> Result<Option<Dispatched<'install>>, InstallError> {
    let Settled { install, mode, lockfiles, .. } = settled;
    announce_import::<Reporter>(settled, options.rebuild.as_ref())?;
    // Dispatch priority, following the CLI + `preferFrozenLockfile`
    // semantics:
    //
    // 1. `--frozen-lockfile` flag → frozen path. Lockfile must exist
    //    and the freshness check (settings + per-importer specifier
    //    match) must pass, otherwise fail.
    //
    // 2. No flag, lockfile present, `prefer_frozen_lockfile == true`,
    //    and the freshness check passes → frozen path (same code as
    //    state 1). The `preferFrozenLockfile` fast path: when the
    //    lockfile matches the manifest, the install silently goes
    //    headless instead of re-resolving against the registry.
    //
    // 3. No flag, lockfile present, but either `prefer_frozen_lockfile`
    //    is off or the freshness check fails → fresh-resolve path,
    //    seeded from the existing lockfile so unrelated entries keep
    //    their pins (the `update: false` resolver mode).
    //
    // 4. No lockfile → fresh-resolve path with no seed, writes a
    //    brand-new `pnpm-lock.yaml`.
    //
    reject_frozen_with_update_checksums(
        install.lockfile_policy.update_checksums,
        install.lockfile_policy.frozen,
    )?;
    // Compute the dispatch decision once. `take_frozen_path` is true
    // for both state 1 (--frozen-lockfile) and state 2 (auto-frozen
    // via prefer-frozen-lockfile). The freshness check fires for both
    // — fatal for state 1, fall-through for state 2.
    //
    // `--dry-run` always takes the fresh-resolve path: it must compute
    // the would-be lockfile to diff against the existing one, and the
    // frozen freshness gate would otherwise abort on a stale lockfile
    // instead of reporting the change.
    let take_frozen_path = decide_frozen_path(&FrozenDispatch {
        dry_run: install.execution.dry_run,
        frozen_lockfile: install.lockfile_policy.frozen,
        lockfile_had_conflicts: settled.loaded.wanted.had_conflicts,
        update_checksums: install.lockfile_policy.update_checksums,
        prefer_frozen_lockfile: mode.prefer_frozen_lockfile,
        lockfile: lockfiles.wanted.get(),
        lockfile_synthesized_from_current: lockfiles.wanted.synthesized_from_current(),
        freshness: settled.freshness_inputs(),
    })
    .await?;

    if take_frozen_path && mode.lockfile_only {
        finish_dispatched_lockfile::<Reporter>(
            settled,
            options.lockfile_verification_override.take(),
        )
        .await?;
        return Ok(None);
    }

    prepare_dispatched_modules::<Reporter>(settled, options, take_frozen_path).await
}
pub(super) async fn finish_dispatched_lockfile<Reporter: self::Reporter + 'static>(
    settled: Settled<'_, '_>,
    lockfile_verification_override: Option<super::super::LockfileVerificationOverride<'_>>,
) -> Result<(), InstallError> {
    let Settled {
        install,
        lockfiles,
        verification,
        projects: SettledProjects { workspace, .. },
        ..
    } = settled;
    let lockfile = lockfiles.wanted.get().expect("frozen dispatch verified lockfile is present");
    finish_frozen_lockfile_only::<Reporter>(
        lockfile,
        install.context.config,
        LockfileOnlyFrozen {
            workspace_root: &workspace.dirs.workspace_root,
            prefix: &workspace.prefix,
            resolution_verifiers: &verification.resolution_verifiers,
            derived_lockfile_path: verification.derived_lockfile_path.as_deref(),
            lockfile_verification_override,
        },
    )
    .await?;
    Reporter::emit(&LogEvent::Summary(SummaryLog {
        level: LogLevel::Debug,
        prefix: workspace.prefix.clone(),
    }));
    Ok(())
}
pub(super) async fn prepare_dispatched_modules<'install, Reporter: self::Reporter + 'static>(
    settled: Settled<'_, '_>,
    options: &mut InstallRunOptions<'install, '_>,
    take_frozen_path: bool,
) -> Result<Option<Dispatched<'install>>, InstallError> {
    let Settled {
        install,
        owned,
        mode,
        loaded,
        lockfiles,
        verification,
        projects: SettledProjects { workspace, scope, project_manifests },
    } = settled;
    let verification = verification.take_inputs(options);
    Ok(prepare_modules_state::<Reporter>(PrepareModulesStateInputs {
        tree: settled.modules_tree(),
        lockfiles: crate::install::state_options::PreparedLockfiles {
            wanted: lockfiles.wanted.get(),
            current: loaded.current.as_ref(),
            importer_ids: scope.importers.requested_importer_ids.as_ref(),
        },
        projects: crate::install::state_options::InstallProjectMetadata {
            catalogs: &workspace.catalogs,
            manifests: project_manifests,
            prefix: &workspace.prefix,
            workspace_packages: workspace.workspace_packages.as_ref(),
        },
        repeat: crate::install::state_options::RepeatInstallPolicy {
            frozen: take_frozen_path,
            filtered: scope.importers.filtered_install,
            disable_optimistic_check: install.lockfile_policy.disable_optimistic_repeat,
            supported_architectures: owned.projects.supported_architectures.as_ref(),
            rebuild: options.rebuild.as_ref(),
            effective_node_version: mode.effective_node_version.as_deref(),
        },
        verification,
        write: lockfiles.write_policy(options.save_lockfile),
        resolve_only: mode.resolve_only,

        installs_only: install.execution.installs_only,
    })
    .await?
    .map(|modules| Dispatched { take_frozen_path, modules }))
}
pub(super) fn announce_import<Reporter: self::Reporter>(
    settled: Settled<'_, '_>,
    rebuild: Option<&crate::RebuildOptions>,
) -> Result<(), InstallError> {
    let Settled {
        install,
        mode,
        loaded,
        projects: SettledProjects { workspace, project_manifests, .. },
        ..
    } = settled;
    // `@pnpm/cli.default-reporter` renders these fields in the install header;
    // `currentLockfileExists` flips after the virtual-store lockfile is written.
    Reporter::emit(&LogEvent::Context(ContextLog {
        level: LogLevel::Debug,
        current_lockfile_exists: loaded.current.is_some(),
        store_dir: install.context.config.store_dir.display().to_string(),
        virtual_store_dir: install.context.config
            .effective_virtual_store_dir()
            .to_string_lossy()
            .into_owned(),
    }));
    // `pnpm:devPreinstall` runs ahead of everything the install does
    // with the lockfile — including the frozen path's freshness
    // check — because what it prepares is an input to resolution and
    // linking. What skips it:
    //
    // - `resolve_only`, which materializes nothing for the hook to
    //   prepare. pnpm reaches the same outcome by having
    //   `--lockfile-only` (and `--dry-run`, which sets it) imply
    //   `ignoreScripts`.
    // - A rebuild, which resolves and links nothing.
    // - `ignore_manifest_check`, which covers `pacquet fetch` (pnpm's
    //   `ignorePackageManifest`, installing from the lockfile alone)
    //   and the TypeScript CLI delegating a frozen materialization,
    //   which already ran the hook before handing the install over.
    // - [`DEV_PREINSTALL_ALREADY_RAN_ENV`], the delegating CLI's
    //   marker for the one path that carries no flag of its own.
    run_dev_preinstall_hook::<Reporter>(&DevPreinstallScope {
        config: install.context.config,
        workspace_root: &workspace.dirs.workspace_root,
        project_manifests,
        resolve_only: mode.resolve_only,
        ignore_manifest_check: install.lockfile_policy.ignore_manifest_check,
        rebuild,
    })?;
    Reporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: workspace.prefix.clone(),
        stage: Stage::ImportingStarted,
    }));
    tracing::info!(target: "pacquet::install", "Start all");
    Ok(())
}
/// What the frozen-vs-fresh dispatch decides on.
pub(super) struct FrozenDispatch<'a> {
    dry_run: bool,
    frozen_lockfile: bool,
    lockfile_had_conflicts: bool,
    update_checksums: bool,
    prefer_frozen_lockfile: bool,
    lockfile: Option<&'a Lockfile>,
    lockfile_synthesized_from_current: bool,
    freshness: LockfileFreshnessInputs<'a, 'a>,
}
/// `take_frozen_path` is true for both state 1 (`--frozen-lockfile`) and state
/// 2 (auto-frozen via `preferFrozenLockfile`). The freshness check fires for
/// both — fatal for state 1, fall-through for state 2.
///
/// `--dry-run` always takes the fresh-resolve path: it must compute the
/// would-be lockfile to diff against the existing one, and the frozen
/// freshness gate would otherwise abort on a stale lockfile instead of
/// reporting the change.
pub(super) async fn decide_frozen_path(
    dispatch: &FrozenDispatch<'_>,
) -> Result<bool, InstallError> {
    if dispatch.dry_run {
        return Ok(false);
    }
    if dispatch.frozen_lockfile {
        let Some(lockfile) = dispatch.lockfile else {
            return Err(InstallError::NoLockfile);
        };
        // Run the freshness gates; on failure surface a fatal InstallError via
        // `FreshnessCheckError`'s `From` impl. The check is run for its side
        // effect (the typed outcome) — the borrowed lockfile / manifests are
        // consumed again inside the frozen branch below.
        //
        // pnpm's importer-set gate sits in the auto-frozen branch of
        // `isFrozenInstallPossible`, which an explicit `--frozen-lockfile`
        // short-circuits past, so a removed project does not fail the install
        // there.
        let freshness = LockfileFreshnessInputs {
            scope: FreshnessScope {
                allow_missing_dependency_free_importers: false,
                prune_stale_importers: false,
                ..dispatch.freshness.scope
            },
            ..dispatch.freshness
        };
        check_lockfile_freshness(lockfile, &freshness).await.map_err(InstallError::from)?;
        return Ok(true);
    }
    // The wanted lockfile was only usable because its Git conflict markers
    // were merged away in memory; `pnpm-lock.yaml` still holds them. Only
    // the fresh-resolve path writes the merge back, so an install that was
    // not explicitly told to keep the lockfile frozen takes it.
    if dispatch.lockfile_had_conflicts {
        return Ok(false);
    }
    if dispatch.update_checksums {
        return Ok(false);
    }
    let Some(lockfile) = dispatch.lockfile else { return Ok(false) };
    // Auto-frozen via `preferFrozenLockfile`. Skip when the user opted out
    // (`--no-prefer-frozen-lockfile` / `preferFrozenLockfile: false`).
    if !dispatch.prefer_frozen_lockfile {
        return Ok(false);
    }
    auto_frozen_path(dispatch, lockfile).await
}
/// Consult the freshness gate for an auto-frozen install. A `Stale` /
/// `NoImporter` outcome routes to the fresh-resolve path; a malformed
/// `pnpm.overrides` is a user-config error that surfaces regardless of
/// dispatch.
pub(super) async fn auto_frozen_path(
    dispatch: &FrozenDispatch<'_>,
    lockfile: &Lockfile,
) -> Result<bool, InstallError> {
    match check_lockfile_freshness(lockfile, &dispatch.freshness).await {
        // Even an up-to-date lockfile may not go frozen: a custom resolver's
        // `shouldRefreshResolution` can force the fresh-resolve path. The
        // hook's verdict blocks the frozen install. A lockfile synthesized
        // from the current snapshot skips the check (it only gates on a
        // non-empty wanted lockfile). A throwing hook aborts the install.
        Ok(()) => {
            // An unchecksummed `readPackage` hook can change dependency
            // manifests without changing the regular freshness inputs.
            if !dispatch.freshness.config.ignore_pnpmfile {
                let current =
                    pnpm_hooks::untracked_read_package_hook(dispatch.freshness.pnpmfile_hook)
                        .await
                        .map_err(InstallError::ReadPackageHook)?;
                if crate::install::untracked_read_package_hook_may_have_changed(
                    lockfile.untracked_pnpmfile_read_package_hook(),
                    current,
                ) {
                    return Ok(false);
                }
            }
            Ok(dispatch.lockfile_synthesized_from_current
                || dispatch.freshness.config.ignore_pnpmfile
                || !crate::check_custom_resolver_force_resolve::force_resolve_from_pnpmfile(
                    lockfile,
                    dispatch.freshness.pnpmfile_hook.map(std::convert::AsRef::as_ref),
                )
                .await
                .map_err(InstallError::CustomResolverForceResolve)?)
        }
        Err(error @ (FreshnessCheckError::Stale(_) | FreshnessCheckError::NoImporter { .. })) => {
            tracing::info!(
                target: "pacquet::install",
                reason = %error,
                "lockfile not usable as-is; falling through to a fresh resolve",
            );
            Ok(false)
        }
        Err(
            error @ (FreshnessCheckError::InvalidOverrides(_)
            | FreshnessCheckError::CalcPatchHashes(_)),
        ) => Err(error.into()),
    }
}
/// What a frozen `--lockfile-only` run still has to verify and write.
pub(super) struct LockfileOnlyFrozen<'a, 'install> {
    workspace_root: &'a Path,
    prefix: &'a str,
    resolution_verifiers: &'a [Arc<dyn super::super::ResolutionVerifier>],
    derived_lockfile_path: Option<&'a Path>,
    lockfile_verification_override: Option<super::super::LockfileVerificationOverride<'install>>,
}
/// This path materializes nothing, so there's no fetch to overlap; verify
/// eagerly to keep the gate before the early return.
pub(super) async fn finish_frozen_lockfile_only<Reporter: self::Reporter + 'static>(
    lockfile: &Lockfile,
    config: &Config,
    finish: LockfileOnlyFrozen<'_, '_>,
) -> Result<(), InstallError> {
    if let Some(lockfile_verification_override) = finish.lockfile_verification_override {
        lockfile_verification_override.await.map_err(map_frozen_lockfile_error)?;
    } else {
        verify_lockfile_eagerly::<Reporter>(
            lockfile,
            finish.resolution_verifiers,
            finish.derived_lockfile_path,
            &config.cache_dir,
        )
        .await?;
    }
    if config.lockfile {
        lockfile
            .save_to_path(&finish.workspace_root.join(config.wanted_lockfile_name()))
            .map_err(InstallError::SaveWantedLockfile)?;
    }
    Reporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: finish.prefix.to_string(),
        stage: Stage::ImportingDone,
    }));
    Ok(())
}

impl Verification {
    fn take_inputs<'r, 'install>(
        &'r self,
        options: &mut InstallRunOptions<'install, '_>,
    ) -> crate::install::state_options::LockfileVerificationInputs<'r, 'install> {
        crate::install::state_options::LockfileVerificationInputs {
            verifiers: &self.resolution_verifiers,
            path: self.derived_lockfile_path.as_deref(),
            override_check: options.lockfile_verification_override.take(),
        }
    }
}

impl<'r> Settled<'r, '_> {
    fn modules_tree(self) -> crate::install::state_options::ModulesTreeContext<'r> {
        crate::install::state_options::ModulesTreeContext {
            config: self.install.context.config,
            workspace_root: &self.projects.workspace.dirs.workspace_root,
            node_linker: self.install.execution.node_linker,
            included: self.mode.included,
        }
    }

    fn freshness_inputs(self) -> LockfileFreshnessInputs<'r, 'r> {
        let Self {
            install,
            loaded,
            lockfiles,
            projects: SettledProjects { workspace, scope, .. },
            ..
        } = self;
        LockfileFreshnessInputs {
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
        }
    }
}
