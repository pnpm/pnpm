mod up_to_date;
use up_to_date::{FrozenTreeUpToDate, UpToDateInstall, frozen_tree_up_to_date, report_up_to_date};

mod purge;
use purge::{
    ExcludedGroupPrune, InconsistentModulesDir, prune_excluded_direct_deps,
    purge_inconsistent_modules_dir,
};

use super::{
    Arc, Catalogs, Config, HashSet, Host, IncludedDependencies, InstallError, Lockfile, Modules,
    NodeLinker, PackageManifest, Path, PathBuf, RebuildOptions, Reporter, ResolutionVerifier,
    modules_layout_consistent_with,
};

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

pub(super) fn prior_hoisted_dependencies(
    previous_modules_metadata: Option<&Modules>,
) -> Option<&super::HoistedDependencies> {
    previous_modules_metadata.map(|modules| &modules.hoisted_dependencies)
}

pub(super) fn prior_hoisted_locations(
    previous_modules_metadata: Option<&Modules>,
) -> Option<&pnpm_deps_restorer::HoistedLocations> {
    previous_modules_metadata.and_then(|modules| modules.hoisted_locations.as_ref())
}

/// Returns `Ok(None)` after completing an up-to-date install; the caller must return successfully
/// without materializing.
pub(super) async fn prepare_modules_state<'install, Reporter: self::Reporter + 'static>(
    inputs: PrepareModulesStateInputs<'_, 'install>,
) -> Result<Option<PreparedModulesState<'install>>, InstallError> {
    let old_modules =
        read_old_modules(inputs.resolve_only, inputs.take_frozen_path, inputs.config)?;
    let modules_manifest = old_modules.as_ref();
    let previous_modules_metadata = read_previous_modules_metadata(
        inputs.resolve_only,
        inputs.filtered_install,
        inputs.config,
    )?;
    let is_inconsistent =
        modules_layout_drifted(modules_manifest, inputs.config, inputs.node_linker);

    prepare_modules_layout(&inputs, modules_manifest, is_inconsistent)?;

    let up_to_date = frozen_tree_inputs(&inputs, modules_manifest);
    if let Some((wanted_lockfile, modules)) = frozen_tree_up_to_date(&up_to_date) {
        report_prepared_up_to_date::<Reporter>(inputs, wanted_lockfile, modules).await?;
        return Ok(None);
    }

    Ok(Some(PreparedModulesState {
        old_modules,
        previous_modules_metadata,
        is_inconsistent,
        lockfile_verification_override: inputs.lockfile_verification_override,
    }))
}

fn prepare_modules_layout(
    inputs: &PrepareModulesStateInputs<'_, '_>,
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
    is_inconsistent: bool,
) -> Result<(), InstallError> {
    if !inputs.resolve_only && is_inconsistent {
        purge_inconsistent_modules_dir(&InconsistentModulesDir {
            config: inputs.config,
            workspace_root: inputs.workspace_root,
            modules_manifest,
            installs_only: inputs.installs_only,
            filtered_install: inputs.filtered_install,
        })?;
    }

    prune_excluded_direct_deps(&ExcludedGroupPrune {
        resolve_only: inputs.resolve_only,
        is_inconsistent,
        filtered_install: inputs.filtered_install,
        config: inputs.config,
        workspace_root: inputs.workspace_root,
        included: inputs.included,
        modules_manifest,
        current_lockfile: inputs.current_lockfile,
        requested_importer_ids: inputs.requested_importer_ids,
    })?;

    Ok(())
}

async fn report_prepared_up_to_date<Reporter: self::Reporter + 'static>(
    inputs: PrepareModulesStateInputs<'_, '_>,
    wanted_lockfile: &Lockfile,
    modules: &pnpm_modules_yaml::ModulesLayout,
) -> Result<(), InstallError> {
    report_up_to_date::<Reporter>(UpToDateInstall {
        config: inputs.config,
        workspace_root: inputs.workspace_root,
        node_linker: inputs.node_linker,
        included: inputs.included,
        wanted_lockfile,
        modules,
        supported_architectures: inputs.supported_architectures,
        catalogs: inputs.catalogs,
        project_manifests: inputs.project_manifests,
        filtered_install: inputs.filtered_install,
        prefix: inputs.prefix,
        resolution_verifiers: inputs.resolution_verifiers,
        derived_lockfile_path: inputs.derived_lockfile_path,
        lockfile_verification_override: inputs.lockfile_verification_override,
        lockfile_synthesized_from_current: inputs.lockfile_synthesized_from_current,
        lockfile_was_fast_updated: inputs.lockfile_was_fast_updated,
        save_lockfile: inputs.save_lockfile,
    })
    .await
}

fn frozen_tree_inputs<'a>(
    inputs: &PrepareModulesStateInputs<'a, '_>,
    modules_manifest: Option<&'a pnpm_modules_yaml::ModulesLayout>,
) -> FrozenTreeUpToDate<'a> {
    FrozenTreeUpToDate {
        take_frozen_path: inputs.take_frozen_path,
        filtered_install: inputs.filtered_install,
        disable_optimistic_repeat_install: inputs.disable_optimistic_repeat_install,
        config: inputs.config,
        workspace_root: inputs.workspace_root,
        node_linker: inputs.node_linker,
        included: inputs.included,
        lockfile: inputs.lockfile,
        current_lockfile: inputs.current_lockfile,
        modules_manifest,
        supported_architectures: inputs.supported_architectures,
        rebuild: inputs.rebuild,
        effective_node_version: inputs.effective_node_version,
    }
}

#[cfg(test)]
mod tests;

/// A no-op still refreshes workspace state so `verifyDepsBeforeRun` does not
/// treat the materialized tree as stale.
///
/// An unreadable state file fails the install rather than reading as layout
/// drift: the drift path purges `node_modules` — the entries the user keeps
/// there included — and relinks the whole tree, and it would do so on every
/// run, because the manifest it rewrites is no more readable than the one it
/// replaced. The TypeScript CLI's `readModulesManifest` rethrows everything
/// but `ENOENT` for the same reason.
fn read_old_modules(
    resolve_only: bool,
    take_frozen_path: bool,
    config: &Config,
) -> Result<Option<pnpm_modules_yaml::ModulesLayout>, InstallError> {
    if resolve_only && !take_frozen_path {
        return Ok(None);
    }
    pnpm_modules_yaml::read_modules_layout::<Host>(&config.modules_dir)
        .map_err(InstallError::ReadModules)
}

/// A filtered install rewrites `.modules.yaml` from the selected projects'
/// state merged over the previous file's, so losing the previous contents
/// would drop every unselected project's entries. A file whose layout parses
/// while some later field does not would otherwise merge against `None` and
/// silently prune those entries, so that case fails instead.
fn read_previous_modules_metadata(
    resolve_only: bool,
    filtered_install: bool,
    config: &Config,
) -> Result<Option<Modules>, InstallError> {
    if resolve_only {
        return Ok(None);
    }
    match pnpm_modules_yaml::read_modules_manifest::<Host>(&config.modules_dir) {
        Ok(modules) => Ok(modules),
        // A filtered install merges the unselected importers' entries out of
        // this file, so it cannot proceed without it; an unfiltered install
        // only loses the orphan hoist-link cleanup.
        Err(error) if filtered_install => Err(InstallError::ReadModules(error)),
        Err(error) => {
            tracing::warn!(
                target: "pacquet::install",
                ?error,
                "failed to fully parse .modules.yaml; skipping orphan hoist-link cleanup",
            );
            Ok(None)
        }
    }
}

/// The purge keys off *layout* drift only, not `included`: an included
/// (`--prod` <-> full) change is handled by relinking, so it must not wipe the
/// user's `node_modules` contents. See [`modules_layout_consistent_with`].
fn modules_layout_drifted(
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
    config: &Config,
    node_linker: NodeLinker,
) -> bool {
    let Some(modules) = modules_manifest else {
        // Treat existence-check errors conservatively as inconsistent.
        return config
            .modules_dir
            .join(pnpm_modules_yaml::MODULES_FILENAME)
            .try_exists()
            .unwrap_or(true);
    };
    !modules_layout_consistent_with(modules, config, node_linker)
}
