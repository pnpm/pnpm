use super::{
    Arc, Catalogs, Config, HashSet, Host, IncludedDependencies, InstallError, Lockfile, LogEvent,
    LogLevel, Modules, NodeLinker, PackageManifest, Path, PathBuf, PnpmLog, RebuildOptions,
    Reporter, ResolutionVerifier, Stage, StageLog, SummaryLog, SystemTime, build_workspace_state,
    check_modules_settings_diff, frozen_tree_intact, gvs_build_marker_present,
    has_newly_allowed_ignored_builds, has_revoked_allowed_builds, map_frozen_lockfile_error,
    modules_consistent_with, modules_layout_consistent_with, unapproved_recorded_ignored_builds,
    update_workspace_state, verify_lockfile_eagerly,
};
use crate::optimistic_repeat_install::filesystem_now_ms;

pub(super) struct PrepareModulesStateInputs<'a, 'install> {
    pub(super) resolve_only: bool,
    pub(super) take_frozen_path: bool,
    pub(super) config: &'static Config,
    pub(super) filtered_install: bool,
    pub(super) installs_only: bool,
    pub(super) workspace_root: &'a Path,
    pub(super) included: IncludedDependencies,
    pub(super) current_lockfile: Option<&'a Lockfile>,
    pub(super) requested_importer_ids: Option<&'a HashSet<String>>,
    pub(super) node_linker: NodeLinker,
    pub(super) disable_optimistic_repeat_install: bool,
    pub(super) lockfile: Option<&'a Lockfile>,
    pub(super) supported_architectures:
        Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    pub(super) rebuild: Option<&'a RebuildOptions>,
    pub(super) resolution_verifiers: &'a [Arc<dyn ResolutionVerifier>],
    pub(super) derived_lockfile_path: Option<&'a Path>,
    pub(super) lockfile_verification_override:
        Option<super::LockfileVerificationOverride<'install>>,
    pub(super) lockfile_synthesized_from_current: bool,
    pub(super) lockfile_was_fast_updated: bool,
    pub(super) save_lockfile: bool,
    pub(super) catalogs: &'a Catalogs,
    pub(super) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(super) effective_node_version: Option<&'a str>,
    pub(super) prefix: &'a str,
}

pub(super) struct PreparedModulesState<'install> {
    pub(super) old_modules: Option<pnpm_modules_yaml::ModulesLayout>,
    pub(super) previous_modules_metadata: Option<Modules>,
    pub(super) is_inconsistent: bool,
    pub(super) lockfile_verification_override:
        Option<super::LockfileVerificationOverride<'install>>,
}

/// Returns `Ok(None)` after completing an up-to-date install; the caller must return successfully
/// without materializing.
pub(super) async fn prepare_modules_state<'install, Reporter: self::Reporter + 'static>(
    inputs: PrepareModulesStateInputs<'_, 'install>,
) -> Result<Option<PreparedModulesState<'install>>, InstallError> {
    let PrepareModulesStateInputs {
        resolve_only,
        take_frozen_path,
        config,
        filtered_install,
        installs_only,
        workspace_root,
        included,
        current_lockfile,
        requested_importer_ids,
        node_linker,
        disable_optimistic_repeat_install,
        lockfile,
        supported_architectures,
        rebuild,
        resolution_verifiers,
        derived_lockfile_path,
        lockfile_verification_override,
        lockfile_synthesized_from_current,
        lockfile_was_fast_updated,
        save_lockfile,
        catalogs,
        project_manifests,
        effective_node_version,
        prefix,
    } = inputs;
    // A no-op still refreshes workspace state so `verifyDepsBeforeRun`
    // does not treat the materialized tree as stale.
    // An unreadable state file fails the install rather than reading as
    // layout drift: the drift path purges `node_modules` — the entries
    // the user keeps there included — and relinks the whole tree, and it
    // would do so on every run, because the manifest it rewrites is no
    // more readable than the one it replaced. The TypeScript CLI's
    // `readModulesManifest` rethrows everything but `ENOENT` for the same
    // reason.
    let old_modules = if !resolve_only || take_frozen_path {
        pnpm_modules_yaml::read_modules_layout::<Host>(&config.modules_dir)
            .map_err(InstallError::ReadModules)?
    } else {
        None
    };
    let modules_manifest = old_modules.as_ref();
    // A filtered install rewrites `.modules.yaml` from the selected
    // projects' state merged over the previous file's, so losing the
    // previous contents would drop every unselected project's entries.
    // A file whose layout parses while some later field does not would
    // otherwise merge against `None` and silently prune those entries, so
    // that case fails instead.
    let previous_modules_metadata = if resolve_only {
        None
    } else {
        match pnpm_modules_yaml::read_modules_manifest::<Host>(&config.modules_dir) {
            Ok(modules) => modules,
            // A filtered install merges the unselected importers'
            // entries out of this file, so it cannot proceed
            // without it; an unfiltered install only loses the
            // orphan hoist-link cleanup.
            Err(error) if filtered_install => return Err(InstallError::ReadModules(error)),
            Err(error) => {
                tracing::warn!(
                    target: "pacquet::install",
                    ?error,
                    "failed to fully parse .modules.yaml; skipping orphan hoist-link cleanup",
                );
                None
            }
        }
    };
    // The purge keys off *layout* drift only, not `included`: an
    // included (`--prod`<->full) change is handled by relinking, so it
    // must not wipe the user's `node_modules` contents. See
    // [`modules_layout_consistent_with`].
    let is_inconsistent = match &modules_manifest {
        Some(modules) => !modules_layout_consistent_with(modules, config, node_linker),
        // Treat existence-check errors conservatively as inconsistent.
        None => config
            .modules_dir
            .join(pnpm_modules_yaml::MODULES_FILENAME)
            .try_exists()
            .unwrap_or(true),
    };

    if !resolve_only && is_inconsistent {
        // A plain install may recreate the drifted modules dir;
        // `add` / `remove` must surface the drift instead
        // (upstream `validateModules` with `forceNewModules =
        // installsOnly`).
        if !installs_only && let Some(modules) = modules_manifest {
            check_modules_settings_diff(modules, config)?;
        }
        let (is_safe, target_dir) = if config.modules_dir.exists() {
            match (
                std::fs::canonicalize(&config.modules_dir),
                std::fs::canonicalize(workspace_root),
            ) {
                (Ok(modules_canon), Ok(root_canon)) => {
                    (is_safe_modules_purge_target(&modules_canon, &root_canon), Some(modules_canon))
                }
                _ => (false, None),
            }
        } else {
            (true, None)
        };
        if is_safe {
            if let Some(target) = target_dir {
                match std::fs::read_dir(&target) {
                    Ok(entries) => {
                        for entry_res in entries {
                            let entry = match entry_res {
                                Ok(e) => e,
                                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                                    continue;
                                }
                                Err(err) => return Err(InstallError::RemoveModulesDir(err)),
                            };
                            let file_name = entry.file_name();
                            let file_name_str = file_name.to_string_lossy();

                            let is_hidden = file_name_str.starts_with('.');
                            let is_pnpm_hidden = file_name_str == ".bin"
                                || file_name_str == ".modules.yaml"
                                || config
                                    .virtual_store_dir
                                    .file_name()
                                    .is_some_and(|n| n == file_name_str.as_ref())
                                || modules_manifest.as_ref().is_some_and(|manifest| {
                                    let mut old_vs =
                                        std::path::PathBuf::from(&manifest.virtual_store_dir);
                                    if old_vs.is_relative() {
                                        old_vs = config.modules_dir.join(old_vs);
                                    }
                                    old_vs.starts_with(&config.modules_dir)
                                        && old_vs
                                            .file_name()
                                            .is_some_and(|n| n == file_name_str.as_ref())
                                });

                            if is_hidden && !is_pnpm_hidden {
                                continue;
                            }

                            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                                #[cfg(windows)]
                                let is_removed = pnpm_fs::remove_symlink_dir(&entry.path()).is_ok();
                                #[cfg(not(windows))]
                                let is_removed = false;

                                if !is_removed
                                    && let Err(err) = std::fs::remove_dir_all(entry.path())
                                    && err.kind() != std::io::ErrorKind::NotFound
                                {
                                    return Err(InstallError::RemoveModulesDir(err));
                                }
                            } else if let Err(err) = std::fs::remove_file(entry.path())
                                && err.kind() != std::io::ErrorKind::NotFound
                            {
                                return Err(InstallError::RemoveModulesDir(err));
                            }
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => return Err(InstallError::RemoveModulesDir(err)),
                }
            }
        } else {
            if filtered_install {
                return Err(InstallError::UnsafeFilteredModulesDir {
                    modules_dir: config.modules_dir.clone(),
                    workspace_root: workspace_root.to_path_buf(),
                });
            }
            tracing::warn!(
                ?config.modules_dir,
                "refusing to remove inconsistent modules directory outside the project root",
            );
        }
    }

    // Remove direct links from dependency groups excluded by this
    // run. Unfiltered installs can use the global `included` value
    // recorded in `.modules.yaml`; filtered installs may retain
    // importers materialized with different group sets, so they
    // conservatively prune every excluded group from only the
    // selected workspace-link closure.
    if !resolve_only
        && !is_inconsistent
        && let Some(modules) = modules_manifest
        && let Some(current) = current_lockfile.as_ref()
        && (filtered_install || modules.included != included)
    {
        let selected_prune_importer_ids = requested_importer_ids.as_ref().map(|requested| {
            crate::materialization_closure(
                current,
                workspace_root,
                requested,
                included,
                &crate::SkippedSnapshots::new(),
            )
            .importer_ids
        });
        let previously_included = if filtered_install {
            IncludedDependencies {
                dependencies: true,
                dev_dependencies: true,
                optional_dependencies: true,
            }
        } else {
            modules.included
        };
        crate::prune_direct_deps_excluded_by_groups(
            current,
            previously_included,
            included,
            workspace_root,
            config,
            selected_prune_importer_ids.as_ref(),
        )
        .map_err(InstallError::PruneDirectDeps)?;
    }

    let modules_cache_prune_due = modules_manifest.as_ref().is_some_and(|modules| {
        crate::prune_virtual_store::should_prune_virtual_store(
            crate::prune_virtual_store::same_dir(
                config.effective_virtual_store_dir(),
                &config.global_virtual_store_dir,
            ),
            Some(modules.pruned_at.as_str()),
            config.modules_cache_max_age,
            SystemTime::now(),
        )
    });

    if take_frozen_path
            && !filtered_install
            && !disable_optimistic_repeat_install
            // `--force` reinstalls everything, so an up-to-date tree
            // must not short-circuit the materialization.
            && !config.force
            && let Some(wanted_lockfile) = lockfile
            && let Some(current) = current_lockfile
            && wanted_lockfile == current
            // A `file:` dependency resolves to a directory whose
            // contents can change with nothing in the lockfile or
            // `.modules.yaml` moving, so an equal-lockfile tree is not
            // evidence that its slot is current. pnpm's `file:` is a
            // copy taken at install time, not a symlink, so the copy
            // has to be retaken; the TypeScript CLI has no gate at this
            // level at all and instead forces every directory dep
            // through materialization in `lockfileToDepGraph`.
            && !has_directory_snapshot(wanted_lockfile)
            && let Some(modules) = modules_manifest.as_ref()
            && modules_consistent_with(modules, config, node_linker, included)
            // A `supportedArchitectures` change alters the skip set
            // without touching the lockfile or `.modules.yaml`, so the
            // unchanged-layout premise doesn't hold and the platform
            // packages must be re-evaluated.
            && crate::optimistic_repeat_install::recorded_supported_architectures_match(
                workspace_root,
                supported_architectures,
            )
            // An `allowBuilds` change that now permits a previously-ignored
            // build must rebuild it, even though the lockfile and layout are
            // unchanged.
            && !has_newly_allowed_ignored_builds(modules, config)
            // The mirror image: an approval the user has since withdrawn
            // must be re-evaluated, or a strict install would exit 0 on a
            // package it is no longer allowed to build.
            && !has_revoked_allowed_builds(modules, config)
            // A build marker lives in the shared slot, outside every
            // project-state input checked above. Let materialization inspect
            // buildable and patched GVS slots instead of declaring the local
            // tree complete from importer links alone.
            && !gvs_build_marker_present(
                wanted_lockfile,
                config,
                workspace_root,
                effective_node_version,
            )
            // An explicit `pacquet rebuild` always re-runs the build phase,
            // so it never short-circuits here.
            && rebuild.is_none()
            && !modules_cache_prune_due
            && frozen_tree_intact(wanted_lockfile, modules, config, workspace_root, node_linker)
    {
        // The full frozen path runs the offline structural
        // name gate before any materialization; the up-to-date
        // early return must not skip it (the resolution-verifier
        // fan-out below is policy-gated and can be empty).
        pnpm_lockfile_verification::verify_lockfile_dependency_names(wanted_lockfile)
            .map_err(InstallError::LockfileVerification)?;
        // Nothing to materialize means no fetch to overlap; verify
        // eagerly before the up-to-date early return.
        if let Some(lockfile_verification_override) = lockfile_verification_override {
            lockfile_verification_override.await.map_err(map_frozen_lockfile_error)?;
        } else {
            verify_lockfile_eagerly::<Reporter>(
                wanted_lockfile,
                resolution_verifiers,
                derived_lockfile_path,
                &config.cache_dir,
            )
            .await?;
        }
        // Keep `strictDepBuilds` enforced on the up-to-date path: a
        // rerun after an `ERR_PNPM_IGNORED_BUILDS` failure must not
        // exit 0 just because the lockfile and layout are unchanged.
        // Checked after verification (a tampered lockfile fails first)
        // and before the "up to date" log so the command doesn't
        // claim success.
        // `Err` (malformed `allowBuilds`) is unreachable here — the
        // `has_newly_allowed_ignored_builds` guard above returns `true`
        // on the same `from_config` error and skips this block — so a
        // bad policy is surfaced by the full install instead.
        if config.strict_dep_builds
            && let Ok(Some(package_names)) = unapproved_recorded_ignored_builds(modules, config)
        {
            return Err(InstallError::IgnoredBuilds { package_names });
        }
        Reporter::emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Info,
            message: "Lockfile is up to date, resolution step is skipped".to_string(),
            prefix: prefix.to_string(),
        }));
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: prefix.to_string(),
            stage: Stage::ImportingDone,
        }));
        // A merge produced a lockfile that no file on disk holds, so it
        // has to be written back even when nothing else changed.
        if (lockfile_synthesized_from_current
            || lockfile_was_fast_updated
            || config.merge_git_branch_lockfiles)
            && config.lockfile
            && save_lockfile
        {
            wanted_lockfile
                .save_to_path(&workspace_root.join(config.wanted_lockfile_name()))
                .map_err(InstallError::SaveWantedLockfile)?;
        }
        update_workspace_state(
            workspace_root,
            &build_workspace_state::<Host>(
                workspace_root,
                config,
                node_linker,
                included,
                supported_architectures,
                catalogs,
                project_manifests,
                filtered_install,
                filesystem_now_ms(workspace_root),
            ),
        )
        .map_err(InstallError::WriteWorkspaceState)?;
        Reporter::emit(&LogEvent::Summary(SummaryLog {
            level: LogLevel::Debug,
            prefix: prefix.to_string(),
        }));
        return Ok(None);
    }

    Ok(Some(PreparedModulesState {
        old_modules,
        previous_modules_metadata,
        is_inconsistent,
        lockfile_verification_override,
    }))
}

/// Whether any package in the lockfile resolves to a local directory.
///
/// Such a package's source is mutable between installs, so its
/// materialized copy can go stale while every install-state artifact
/// still says the tree is current.
fn has_directory_snapshot(lockfile: &Lockfile) -> bool {
    lockfile.packages.iter().flat_map(|packages| packages.values()).any(|metadata| {
        matches!(metadata.resolution, pnpm_lockfile::LockfileResolution::Directory(_))
    })
}

fn is_safe_modules_purge_target(modules_dir: &Path, workspace_root: &Path) -> bool {
    modules_dir != workspace_root && modules_dir.starts_with(workspace_root)
}

#[cfg(test)]
mod tests;
