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
) -> Result<(pnpm_deps_restorer::SkippedSnapshots, crate::GroupSelection), InstallError> {
    let host = pnpm_deps_restorer::materialization_plan::with_locked_runtime_node(
        detect_host(settled, lockfile).await.as_ref(),
        settled.install.context.config,
        &lockfile.importers,
    );
    let groups = crate::GroupSelection::classify(
        lockfile,
        settled.mode.included,
        settled.install.context.config.peer_edge_options(),
    );
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
                groups: &groups,
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
    .map(|skipped| (skipped, groups))
    .map_err(pnpm_deps_restorer::InstallFrozenLockfileError::Installability)
    .map_err(map_frozen_lockfile_error)
}

pub(super) async fn verify_frozen_tarballs(settled: Settled<'_, '_>) -> Result<(), InstallError> {
    let lockfile =
        settled.lockfiles.wanted.get().expect("frozen dispatch verified lockfile is present");
    if !has_local_tarball(lockfile) {
        return Ok(());
    }
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
    let (skipped, groups) = compute_frozen_skip_set(&settled, lockfile, &importer_ids).await?;

    let targets = crate::optimistic_repeat_install::frozen_local_tarballs_to_verify(
        &crate::optimistic_repeat_install::FrozenLocalTarballCheck {
            workspace_root: &settled.projects.workspace.dirs.workspace_root,
            importer_ids: &importer_ids,
            groups: &groups,
            lockfile,
            skipped: &skipped,
        },
    );

    verify_integrity(targets).await
}

/// Whether any package resolves to a tarball on the local filesystem, the
/// only kind [`verify_frozen_tarballs`] checks.
fn has_local_tarball(lockfile: &pnpm_lockfile::Lockfile) -> bool {
    lockfile.packages
        .iter()
        .flat_map(|packages| packages.values())
        .any(|package| {
            matches!(
                &package.resolution,
                pnpm_lockfile::LockfileResolution::Tarball(resolution)
                    if pnpm_lockfile::is_local_tarball_path(&resolution.tarball),
            )
        })
}

async fn verify_integrity(
    targets: Vec<(std::path::PathBuf, ssri::Integrity)>,
) -> Result<(), InstallError> {
    if targets.is_empty() {
        return Ok(());
    }
    tokio::task::spawn_blocking(move || {
        use rayon::prelude::*;
        targets
            .into_par_iter()
            .try_for_each(|(path, integrity)| {
                pnpm_tarball::verify_local_file_integrity(&path, &integrity)
            })
    })
    .await
    .expect("verify frozen local tarballs task panicked")
    .map_err(InstallError::LocalTarballIntegrity)
}
