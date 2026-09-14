mod up_to_date;
use up_to_date::{FrozenTreeUpToDate, UpToDateInstall, frozen_tree_up_to_date, report_up_to_date};

mod purge;
use purge::{
    ExcludedGroupPrune, InconsistentModulesDir, prune_excluded_direct_deps,
    purge_inconsistent_modules_dir,
};

use super::{
    Config, Host, InstallError, Lockfile, Modules, NodeLinker, Reporter,
    modules_layout_consistent_with,
};

pub(super) struct PrepareModulesStateInputs<'a, 'install> {
    pub(crate) tree: crate::install::state_options::ModulesTreeContext<'a>,
    pub(crate) lockfiles: crate::install::state_options::PreparedLockfiles<'a>,
    pub(crate) projects: crate::install::state_options::InstallProjectMetadata<'a>,
    pub(crate) repeat: crate::install::state_options::RepeatInstallPolicy<'a>,
    pub(crate) verification:
        crate::install::state_options::LockfileVerificationInputs<'a, 'install>,
    pub(crate) write: crate::install::state_options::LockfileWritePolicy,
    pub(super) resolve_only: bool,
    pub(super) installs_only: bool,
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
        read_old_modules(inputs.resolve_only, inputs.repeat.frozen, inputs.tree.config)?;
    let modules_manifest = old_modules.as_ref();
    let previous_modules_metadata = read_previous_modules_metadata(
        inputs.resolve_only,
        inputs.repeat.filtered,
        inputs.tree.config,
    )?;
    let is_inconsistent =
        modules_layout_drifted(modules_manifest, inputs.tree.config, inputs.tree.node_linker);

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
        lockfile_verification_override: inputs.verification.override_check,
    }))
}

fn prepare_modules_layout(
    inputs: &PrepareModulesStateInputs<'_, '_>,
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
    is_inconsistent: bool,
) -> Result<(), InstallError> {
    if !inputs.resolve_only && is_inconsistent {
        purge_inconsistent_modules_dir(&InconsistentModulesDir {
            config: inputs.tree.config,
            workspace_root: inputs.tree.workspace_root,
            modules_manifest,
            installs_only: inputs.installs_only,
            filtered_install: inputs.repeat.filtered,
        })?;
    }

    prune_excluded_direct_deps(&ExcludedGroupPrune {
        eligibility: crate::install::state_options::PruneEligibility {
            resolve_only: inputs.resolve_only,
            is_inconsistent,
            filtered_install: inputs.repeat.filtered,
        },

        config: inputs.tree.config,
        workspace_root: inputs.tree.workspace_root,
        included: inputs.tree.included,
        modules_manifest,
        current_lockfile: inputs.lockfiles.current,
        requested_importer_ids: inputs.lockfiles.importer_ids,
    })?;

    Ok(())
}

async fn report_prepared_up_to_date<Reporter: self::Reporter + 'static>(
    inputs: PrepareModulesStateInputs<'_, '_>,
    wanted_lockfile: &Lockfile,
    modules: &pnpm_modules_yaml::ModulesLayout,
) -> Result<(), InstallError> {
    report_up_to_date::<Reporter>(UpToDateInstall {
        tree: crate::install::state_options::ModulesTreeContext {
            config: inputs.tree.config,
            workspace_root: inputs.tree.workspace_root,
            node_linker: inputs.tree.node_linker,
            included: inputs.tree.included,
        },
        projects: crate::install::state_options::InstallProjectMetadata {
            catalogs: inputs.projects.catalogs,
            manifests: inputs.projects.manifests,
            prefix: inputs.projects.prefix,
        },
        verification: crate::install::state_options::LockfileVerificationInputs {
            verifiers: inputs.verification.verifiers,
            path: inputs.verification.path,
            override_check: inputs.verification.override_check,
        },
        write: crate::install::state_options::LockfileWritePolicy {
            synthesized_from_current: inputs.write.synthesized_from_current,
            fast_updated: inputs.write.fast_updated,
            save: inputs.write.save,
        },

        wanted_lockfile,
        modules,
        supported_architectures: inputs.repeat.supported_architectures,

        filtered_install: inputs.repeat.filtered,
    })
    .await
}

fn frozen_tree_inputs<'a>(
    inputs: &PrepareModulesStateInputs<'a, '_>,
    modules_manifest: Option<&'a pnpm_modules_yaml::ModulesLayout>,
) -> FrozenTreeUpToDate<'a> {
    FrozenTreeUpToDate {
        tree: crate::install::state_options::ModulesTreeContext {
            config: inputs.tree.config,
            workspace_root: inputs.tree.workspace_root,
            node_linker: inputs.tree.node_linker,
            included: inputs.tree.included,
        },
        repeat: crate::install::state_options::RepeatInstallPolicy {
            frozen: inputs.repeat.frozen,
            filtered: inputs.repeat.filtered,
            disable_optimistic_check: inputs.repeat.disable_optimistic_check,
            supported_architectures: inputs.repeat.supported_architectures,
            rebuild: inputs.repeat.rebuild,
            effective_node_version: inputs.repeat.effective_node_version,
        },

        lockfile: inputs.lockfiles.wanted,
        current_lockfile: inputs.lockfiles.current,
        modules_manifest,
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
        return config.modules_dir
            .join(pnpm_modules_yaml::MODULES_FILENAME)
            .try_exists()
            .unwrap_or(true);
    };
    !modules_layout_consistent_with(modules, config, node_linker)
}
