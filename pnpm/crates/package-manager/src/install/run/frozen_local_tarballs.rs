use std::{collections::HashSet, path::Path};

use super::{super::map_frozen_lockfile_error, InstallError, dispatch::Settled};

async fn detect_host(
    settled: &Settled<'_, '_>,
    lockfile: &pnpm_lockfile::Lockfile,
) -> Option<pnpm_deps_restorer::InstallabilityHost> {
    let needs_check = !settled.install.context.config.force
        && match (lockfile.snapshots.as_ref(), lockfile.packages.as_ref()) {
            (Some(snaps), Some(pkgs)) if !snaps.is_empty() => {
                pnpm_deps_restorer::any_installability_constraint(snaps, pkgs)
            }
            _ => false,
        };
    pnpm_deps_restorer::materialization_plan::detect_installability_host(
        needs_check,
        settled.install.context.config.engine_strict,
        settled.mode.effective_node_version.clone(),
        settled.owned.projects.supported_architectures.as_ref(),
    )
    .await
}

fn read_skipped_seed(modules_dir: &Path) -> pnpm_deps_restorer::SkippedSnapshots {
    pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(modules_dir)
        .ok()
        .flatten()
        .map(|modules| pnpm_deps_restorer::SkippedSnapshots::from_strings(modules.skipped.iter()))
        .unwrap_or_default()
}

async fn compute_frozen_skip_set(
    settled: &Settled<'_, '_>,
    lockfile: &pnpm_lockfile::Lockfile,
    importer_ids: &HashSet<String>,
) -> Result<pnpm_deps_restorer::SkippedSnapshots, InstallError> {
    let host = detect_host(settled, lockfile).await;
    let seed = if settled.install.context.config.force {
        pnpm_deps_restorer::SkippedSnapshots::default()
    } else {
        read_skipped_seed(&settled.install.context.config.modules_dir)
    };
    let workspace = &settled.projects.workspace;
    pnpm_deps_restorer::materialization_plan::compute_skip_set::<pnpm_reporter::SilentReporter>(
        pnpm_deps_restorer::materialization_plan::SkipSetInputs {
            closure: pnpm_deps_restorer::SkipSetClosure {
                lockfile,
                root: &workspace.dirs.workspace_root,
                importer_ids,
                included: settled.mode.included,
            },
            entries: pnpm_lockfile::LockfileEntries {
                packages: lockfile.packages.as_ref(),
                snapshots: lockfile.snapshots.as_ref(),
            },
            requester: &workspace.prefix,
            importers: &lockfile.importers,
            installability_host: host.as_ref(),
            seed,
            exclude_optional: !settled.mode.included.optional_dependencies,
            skip_runtimes: settled.install.execution.skip_runtimes,
        },
    )
    .map_err(pnpm_deps_restorer::InstallFrozenLockfileError::Installability)
    .map_err(map_frozen_lockfile_error)
}

pub(super) async fn verify_frozen_tarballs(settled: Settled<'_, '_>) -> Result<(), InstallError> {
    let lockfile =
        settled.lockfiles.wanted.get().expect("frozen dispatch verified lockfile is present");
    let requested = settled.projects.scope.importers.requested_importer_ids
        .as_ref()
        .or_else(|| {
            (!settled.install.lockfile_policy.ignore_manifest_check).then_some(
                &settled.projects.scope.importers.real_importer_ids,
            )
        });
    let importer_ids = crate::install::materialize::initial_materialization_ids(
        lockfile,
        requested,
        settled.install.execution.node_linker,
    );
    let skipped = compute_frozen_skip_set(&settled, lockfile, &importer_ids).await?;

    crate::optimistic_repeat_install::verify_frozen_local_tarballs(
        &crate::optimistic_repeat_install::FrozenLocalTarballCheck {
            workspace_root: &settled.projects.workspace.dirs.workspace_root,
            importer_ids: &importer_ids,
            included: settled.mode.included,
            lockfile,
            skipped: &skipped,
        },
    )
    .map_err(InstallError::LocalTarballIntegrity)
}
