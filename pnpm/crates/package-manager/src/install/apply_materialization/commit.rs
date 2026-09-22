use super::super::{
    Config, Host, InstallError, InstallWithFreshLockfileError, Lockfile, Modules, NodeLinker,
    PackageManifest, Path, PathBuf, SystemTime, build_modules_manifest, current_contains_dep_path,
    merge_filtered_modules_metadata, merge_pending_builds, project_requires_lifecycle_scripts,
    write_modules_manifest,
};

pub(super) struct CommitModulesStateInputs<'a> {
    pub(crate) builds: crate::install::state_options::CommittedBuildState<'a>,
    pub(crate) tree: crate::install::state_options::ModulesTreeContext<'a>,
    pub(crate) hoisted: pnpm_deps_restorer::InstalledHoistedState,
    pub(crate) lockfiles: crate::install::state_options::CommittedLockfiles<'a>,
    pub(crate) materialized: crate::install::state_options::CommittedProjects<'a>,
    pub(crate) prior: crate::install::state_options::PriorModulesState<'a>,
    pub(crate) write: crate::install::state_options::LockfileWritePolicy,
    pub(crate) force_prune: bool,
}
pub(super) fn commit_modules_state(
    mut inputs: CommitModulesStateInputs<'_>,
) -> Result<(), InstallError> {
    let (pruned_at, pending_builds, allow_build_policy) =
        prepare_committed_build_state(&mut inputs)?;

    // Rebuild reads hoisted locations from `.modules.yaml` and reports
    // `MISSING_HOISTED_LOCATIONS` if an install fails to persist them here.
    let mut next_modules = build_modules_manifest(
        inputs.tree.config,
        inputs.tree.node_linker,
        inputs.tree.included,
        std::mem::take(&mut inputs.hoisted.dependencies),
        std::mem::take(&mut inputs.hoisted.locations),
        std::mem::take(&mut inputs.hoisted.injected_deps),
        inputs.materialized.skipped,
        inputs.builds.ignored_builds,
        pending_builds,
        pruned_at,
    );
    merge_committed_modules_metadata(&inputs, &mut next_modules, allow_build_policy.as_ref());
    let phase_start = std::time::Instant::now();
    write_modules_manifest::<Host>(&inputs.tree.config.modules_dir, next_modules)
        .map_err(InstallError::WriteModules)?;
    tracing::info!(target: "pacquet::install::phase", phase = "apply.modules_yaml", elapsed_ms = phase_start.elapsed().as_millis() as u64, "phase complete");

    let phase_start = std::time::Instant::now();
    save_current_lockfile(inputs.tree.config, inputs.lockfiles.materialized)?;
    tracing::info!(target: "pacquet::install::phase", phase = "apply.current_lockfile", elapsed_ms = phase_start.elapsed().as_millis() as u64, "phase complete");
    // Regenerate `pnpm-lock.yaml` from the synthesized snapshot when
    // the wanted lockfile was reconstructed from
    // `<virtual_store_dir>/lock.yaml`. The no-op short-circuit above
    // handles the common case; this branch covers the rare path where
    // `.modules.yaml` was wiped or inconsistent and the frozen install
    // had to relink.
    if inputs.materialized.frozen {
        save_relinked_wanted_lockfile(&RelinkedLockfileSave {
            config: inputs.tree.config,
            workspace_root: inputs.tree.workspace_root,
            lockfile_synthesized_from_current: inputs.write.synthesized_from_current,
            lockfile_was_fast_updated: inputs.write.fast_updated,
            save_lockfile: inputs.write.save,
            loaded_wanted_lockfile: inputs.lockfiles.wanted,
        })?;
    }

    Ok(())
}
pub(super) fn prepare_committed_build_state(
    inputs: &mut CommitModulesStateInputs<'_>,
) -> Result<(String, Vec<String>, Option<crate::AllowBuildPolicy>), InstallError> {
    let now = SystemTime::now();
    let phase_start = std::time::Instant::now();
    let did_prune = sweep_virtual_store(
        inputs.tree.config,
        inputs.prior.layout,
        inputs.lockfiles.materialized,
        inputs.materialized.skipped,
        inputs.force_prune,
        now,
    );
    tracing::info!(target: "pacquet::install::phase", phase = "apply.prune", elapsed_ms = phase_start.elapsed().as_millis() as u64, "phase complete");
    let pruned_at = pruned_at(inputs.prior.layout, did_prune, now);
    // The build phase settles a dependency only when it actually
    // rebuilt it, so a `pnpm rebuild --pending` that the policy still
    // blocks (`allowBuilds: None`/`false`) leaves the debt in place.
    // Reuse the same policy `BuildModules` ran under; on a rebuild a
    // selected, approved dependency always runs (force-rebuild
    // bypasses the side-effects cache gate), so policy approval is a
    // faithful stand-in for "was rebuilt".
    let allow_build_policy = (inputs.builds.rebuild.is_some()
        || (inputs.prior.layout.is_some() && inputs.lockfiles.materialized.is_some()))
    .then(|| crate::AllowBuildPolicy::from_config(inputs.tree.config))
    .transpose()
    .map_err(InstallWithFreshLockfileError::AllowBuildsPolicy)
    .map_err(InstallError::WithFreshLockfile)?;
    let pending_builds = merge_pending_builds(
        inputs.prior.layout.map_or(&[][..], |modules| modules.pending_builds.as_slice()),
        deferred_projects(
            inputs.tree.config,
            inputs.materialized.manifests,
            inputs.tree.workspace_root,
        )
        .into_iter()
        .flatten()
        .chain(std::mem::take(&mut inputs.builds.deferred_builds)),
        inputs.lockfiles.materialized,
        inputs.builds.rebuild,
        allow_build_policy.as_ref(),
    );

    Ok((pruned_at, pending_builds, allow_build_policy))
}
pub(super) fn merge_committed_modules_metadata(
    inputs: &CommitModulesStateInputs<'_>,
    next_modules: &mut Modules,
    allow_build_policy: Option<&crate::AllowBuildPolicy>,
) {
    if let (Some(previous), Some(current), Some(policy)) =
        (inputs.prior.layout, inputs.lockfiles.materialized, allow_build_policy)
    {
        retain_current_ignored_builds(next_modules, previous, current, policy);
    }
    if inputs.prior.filtered_install
        && !matches!(inputs.tree.node_linker, NodeLinker::Hoisted)
        && !inputs.prior.is_inconsistent
        && let (Some(previous), Some(current), Some(selected)) =
            (inputs.prior.metadata, inputs.lockfiles.materialized, inputs.lockfiles.selected)
    {
        merge_filtered_modules_metadata(next_modules, previous, current, selected);
    }
}
/// Stamp `prunedAt` only when the sweep ran (or there was no prior
/// `.modules.yaml`); otherwise preserve the recorded timestamp so
/// the throttle keeps counting from the last real prune.
pub(super) fn pruned_at(
    prior_modules: Option<&pnpm_modules_yaml::ModulesLayout>,
    did_prune: bool,
    now: SystemTime,
) -> String {
    match (prior_modules, did_prune) {
        (Some(prior), false) => prior.pruned_at.clone(),
        _ => httpdate::fmt_http_date(now),
    }
}
/// The projects whose own install scripts `--ignore-scripts`
/// skipped are owed a build just like the dependencies the build
/// phase deferred, and are recorded by importer id.
pub(super) fn deferred_projects(
    config: &Config,
    materialized_project_manifests: &[(PathBuf, &PackageManifest)],
    workspace_root: &Path,
) -> Option<Vec<String>> {
    config.ignore_scripts.then(|| {
        materialized_project_manifests
            .iter()
            .filter(|(project_dir, manifest)| {
                project_requires_lifecycle_scripts(project_dir, manifest)
            })
            .map(|(project_dir, _)| {
                pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir)
            })
            .collect()
    })
}
/// Write `<virtual_store_dir>/lock.yaml`. Captures what was
/// actually materialized so the next install can diff each
/// snapshot against it and skip the unchanged
/// slots. Persist *after* `write_modules_manifest` succeeds so
/// a manifest failure can't leave a fresh current-lockfile
/// pointing at incomplete install state — the next frozen
/// reinstall would otherwise diff against a graph that never
/// finished committing.
///
/// A filtered isolated/PnP install merges its newly materialized
/// closure into compatible prior current state, while a hoisted
/// install records the full shared graph it materialized. This
/// keeps the file aligned with physical state without discarding
/// unselected slots that remain on disk.
/// Filter the wanted lockfile down to the snapshots that
/// were actually materialized: dep maps the user excluded
/// (`--no-optional`, `--no-dev`) plus snapshots the
/// install-time skip set transiently dropped (a fetch
/// failure, `--no-optional`-only entries). The next install
/// diffs against this filtered shape so dropped snapshots
/// aren't mistaken for already-done work.
pub(super) fn save_current_lockfile(
    config: &Config,
    materialized_current_lockfile: Option<&Lockfile>,
) -> Result<(), InstallError> {
    let Some(lockfile) = materialized_current_lockfile else { return Ok(()) };
    lockfile
        .save_current_to_virtual_store_dir(&config.virtual_store_dir)
        .map_err(InstallError::SaveCurrentLockfile)
}
/// Sweep the virtual store of everything the install no longer needs, and
/// report whether the sweep actually ran (enumerated the store) rather than
/// just being allowed by the throttle. It does not run when there is no
/// lockfile to derive the needed set from (`materialized_current_lockfile` is
/// `None`), when the target is refused as unsafe, or when enumeration failed.
/// `prunedAt` must not advance on a run where nothing was swept, or the next
/// real sweep is throttled off for `modulesCacheMaxAge`.
pub(super) fn sweep_virtual_store(
    config: &Config,
    prior_modules: Option<&pnpm_modules_yaml::ModulesLayout>,
    materialized_current_lockfile: Option<&Lockfile>,
    install_skipped: &crate::SkippedSnapshots,
    force: bool,
    now: SystemTime,
) -> bool {
    if !virtual_store_prune_is_due(config, prior_modules, force, now) {
        return false;
    }
    let Some(wanted) = materialized_current_lockfile else {
        return false;
    };
    let Some(prune_dir) = confined_virtual_store_prune_target(config) else {
        return false;
    };
    crate::prune_virtual_store::prune_virtual_store(
        &prune_dir,
        wanted.snapshots.iter().flat_map(|snapshots| snapshots.keys()),
        install_skipped,
        config.virtual_store_dir_max_length as usize,
    )
    .is_some()
}

fn virtual_store_prune_is_due(
    config: &Config,
    prior_modules: Option<&pnpm_modules_yaml::ModulesLayout>,
    force: bool,
    now: SystemTime,
) -> bool {
    let effective_virtual_store_dir = config.effective_virtual_store_dir();
    // Decide "this is the global store" from the resolved paths, not
    // the `enableGlobalVirtualStore` flag alone: the global store is
    // shared across projects, so a config that points `virtualStoreDir`
    // at it must not be pruned even when the flag is off.
    let is_global_virtual_store = crate::prune_virtual_store::same_dir(
        effective_virtual_store_dir,
        &config.global_virtual_store_dir,
    );
    !is_global_virtual_store
        && (force
            || crate::prune_virtual_store::should_prune_virtual_store(
                false,
                prior_modules.map(|modules| modules.pruned_at.as_str()),
                config.modules_cache_max_age,
                now,
            ))
}

fn confined_virtual_store_prune_target(config: &Config) -> Option<PathBuf> {
    let effective_virtual_store_dir = config.effective_virtual_store_dir();
    // Sweep the canonicalized prune target returned by the containment
    // check, never the raw configured path: deleting from the validated path
    // closes the time-of-check/time-of-use gap a symlink swap would
    // otherwise open.
    let Some(prune_dir) = crate::prune_virtual_store::prune_target_within_modules(
        effective_virtual_store_dir,
        &config.modules_dir,
    ) else {
        // A wanted lockfile exists but the store path is unsafe
        // (escapes node_modules); refuse the destructive sweep.
        tracing::warn!(
            virtual_store_dir = %effective_virtual_store_dir.display(),
            modules_dir = %config.modules_dir.display(),
            "skipping virtual-store prune: the virtual store is not inside node_modules",
        );
        return None;
    };
    Some(prune_dir)
}
/// What decides whether a relinking frozen install rewrites `pnpm-lock.yaml`.
pub(super) struct RelinkedLockfileSave<'a> {
    config: &'a Config,
    workspace_root: &'a Path,
    lockfile_synthesized_from_current: bool,
    lockfile_was_fast_updated: bool,
    save_lockfile: bool,
    loaded_wanted_lockfile: Option<&'a Lockfile>,
}
/// Regenerate `pnpm-lock.yaml` from the synthesized snapshot when the wanted
/// lockfile was reconstructed from `<virtual_store_dir>/lock.yaml`. The
/// no-op short-circuit in the caller handles the common case; this covers the
/// rare path where `.modules.yaml` was wiped or inconsistent and the frozen
/// install had to relink.
pub(super) fn save_relinked_wanted_lockfile(
    save: &RelinkedLockfileSave<'_>,
) -> Result<(), InstallError> {
    let config = save.config;
    if (save.lockfile_synthesized_from_current
        || save.lockfile_was_fast_updated
        || config.merge_git_branch_lockfiles)
        && config.lockfile
        && save.save_lockfile
        && let Some(updated) = save.loaded_wanted_lockfile
    {
        updated
            .save_to_path(&save.workspace_root.join(config.wanted_lockfile_name()))
            .map_err(InstallError::SaveWantedLockfile)?;
    }
    Ok(())
}
pub(super) fn retain_current_ignored_builds(
    next: &mut Modules,
    previous: &pnpm_modules_yaml::ModulesLayout,
    current: &Lockfile,
    allow_build_policy: &crate::AllowBuildPolicy,
) {
    let Some(previous_ignored) = previous.ignored_builds.as_ref() else { return };
    for dep_path in previous_ignored {
        if current_contains_dep_path(current, dep_path.as_str())
            && allow_build_policy.check(dep_path.as_str()).is_none()
        {
            next.ignored_builds.get_or_insert_default().insert(dep_path.clone());
        }
    }
}
