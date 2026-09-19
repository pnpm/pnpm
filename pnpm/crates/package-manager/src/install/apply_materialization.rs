pub(super) mod completion;

use completion::{
    MaterializedProjectScriptsInputs, ReportInstallCompletionInputs, ResolveOnlyCompletionInputs,
    complete_resolve_only, report_install_completion, run_materialized_project_scripts,
};

mod commit;
use commit::{CommitModulesStateInputs, commit_modules_state};

mod selection;
use selection::{
    LinkMaterializedLockfiles, LinkMaterializedProjectsInputs, MaterializedState,
    SelectMaterializedStateInputs, link_materialized_projects, select_materialized_state,
};

use super::{
    BTreeMap, HoistedDependencies, Host, InstallError, Lockfile, Materialized, Reporter,
    build_workspace_state, update_workspace_state,
};
use crate::optimistic_repeat_install::filesystem_now_ms;

pub(super) struct ApplyMaterializationInputs<'a, 'selection> {
    pub(crate) completion: crate::install::state_options::ApplyCompletionContext,
    pub(crate) mode: crate::install::state_options::CompletionMode,
    pub(crate) prior: crate::install::state_options::ApplyPriorState,
    pub(crate) projects: crate::install::state_options::ApplyProjectSelection<'a>,
    pub(crate) resolution: crate::install::state_options::ApplyResolutionState<'a>,
    pub(crate) scripts: crate::install::state_options::PendingProjectScripts<'a, 'selection>,
    pub(crate) write: crate::install::state_options::LockfileWritePolicy,
    pub(super) materialized: Materialized,
}

pub(super) async fn apply_materialization_result<Reporter: self::Reporter + 'static>(
    inputs: ApplyMaterializationInputs<'_, '_>,
) -> Result<(), InstallError> {
    let phase_start = std::time::Instant::now();
    apply::<Reporter>(inputs).await?;
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "apply_materialization_result",
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        "phase complete",
    );
    Ok(())
}

fn select_apply_state<'a>(inputs: &'a ApplyMaterializationInputs<'_, '_>) -> MaterializedState<'a> {
    select_materialized_state(&SelectMaterializedStateInputs {
        lockfiles: crate::install::state_options::SelectedLockfiles {
            fresh: inputs.materialized.fresh_lockfile.as_ref(),
            wanted: inputs.resolution.loaded,
            current: inputs.prior.lockfile.as_ref(),
        },
        projects: inputs.projects.importers,

        workspace_root: &inputs.projects.workspace_root,
        included: inputs.projects.included,
        install_skipped: &inputs.materialized.install_skipped,
        node_linker: inputs.projects.node_linker,

        is_inconsistent: inputs.prior.is_inconsistent,
    })
}

async fn link_apply_projects<Reporter: self::Reporter + 'static>(
    inputs: &ApplyMaterializationInputs<'_, '_>,
    state: &MaterializedState<'_>,
) -> Result<(), InstallError> {
    let phase_start = std::time::Instant::now();
    link_materialized_projects::<Reporter>(LinkMaterializedProjectsInputs {
        filtered_install: inputs.projects.filtered_install,
        node_linker: inputs.projects.node_linker,
        config: inputs.completion.config,
        lockfiles: LinkMaterializedLockfiles {
            current: state.current_lockfile.as_ref(),
            wanted: state.wanted_lockfile,
        },
        workspace_root: &inputs.projects.workspace_root,
        workspace_packages: inputs.projects.workspace_packages.as_ref(),
        project_manifests: inputs.projects.importers.manifests,
        materialized_project_manifests: &state.project_manifests,
    })
    .await?;
    tracing::info!(target: "pacquet::install::phase", phase = "apply.link_projects", elapsed_ms = phase_start.elapsed().as_millis() as u64, "phase complete");

    Ok(())
}

struct CommittedMetadata {
    hoisted_dependencies: HoistedDependencies,
    hoisted_locations: BTreeMap<String, Vec<String>>,
    injected_deps: BTreeMap<String, Vec<String>>,
    deferred_builds: Vec<String>,
}

fn take_committed_metadata(materialized: &mut Materialized) -> CommittedMetadata {
    CommittedMetadata {
        hoisted_dependencies: std::mem::take(&mut materialized.hoisted.dependencies),
        hoisted_locations: std::mem::take(&mut materialized.hoisted.locations),
        injected_deps: std::mem::take(&mut materialized.hoisted.injected_deps),
        deferred_builds: std::mem::take(&mut materialized.deferred_builds),
    }
}

fn commit_apply_state<Reporter: self::Reporter>(
    inputs: &ApplyMaterializationInputs<'_, '_>,
    state: &MaterializedState<'_>,
    metadata: CommittedMetadata,
) -> Result<(), InstallError> {
    let phase_start = std::time::Instant::now();
    commit_modules_state(CommitModulesStateInputs {
        builds: crate::install::state_options::CommittedBuildState {
            ignored_builds: &inputs.materialized.ignored_builds,
            deferred_builds: metadata.deferred_builds,
            rebuild: inputs.scripts.rebuild.as_ref(),
        },
        tree: crate::install::state_options::ModulesTreeContext {
            config: inputs.completion.config,
            workspace_root: &inputs.projects.workspace_root,
            node_linker: inputs.projects.node_linker,
            included: inputs.projects.included,
        },
        hoisted: pnpm_deps_restorer::InstalledHoistedState {
            dependencies: metadata.hoisted_dependencies,
            locations: metadata.hoisted_locations,
            injected_deps: metadata.injected_deps,
        },
        lockfiles: crate::install::state_options::CommittedLockfiles {
            materialized: state.current_lockfile.as_ref(),
            selected: state.selected_current_lockfile.as_ref(),
            wanted: inputs.resolution.loaded,
        },
        materialized: crate::install::state_options::CommittedProjects {
            manifests: &state.project_manifests,
            skipped: &inputs.materialized.install_skipped,
            frozen: inputs.resolution.frozen,
        },
        prior: crate::install::state_options::PriorModulesState {
            layout: inputs.prior.layout.as_ref(),
            metadata: inputs.prior.metadata.as_ref(),
            is_inconsistent: inputs.prior.is_inconsistent,
            filtered_install: inputs.projects.filtered_install,
        },
        write: inputs.write,
    })?;
    tracing::info!(target: "pacquet::install::phase", phase = "apply.commit_modules_state", elapsed_ms = phase_start.elapsed().as_millis() as u64, "phase complete");

    run_apply_scripts::<Reporter>(inputs, state)
}

fn run_apply_scripts<Reporter: self::Reporter>(
    inputs: &ApplyMaterializationInputs<'_, '_>,
    state: &MaterializedState<'_>,
) -> Result<(), InstallError> {
    run_materialized_project_scripts::<Reporter>(MaterializedProjectScriptsInputs {
        request: crate::install::state_options::ProjectScriptSelection {
            mutation: inputs.scripts.mutation,
            manifest_dir: inputs.scripts.manifest_dir,
            workspace: inputs.scripts.selection.as_ref(),
            rebuild: inputs.scripts.rebuild.as_ref(),
        },
        config: inputs.completion.config,
        node_linker: inputs.projects.node_linker,
        workspace_root: &inputs.projects.workspace_root,

        project_manifests: inputs.projects.importers.manifests,
        materialized_project_manifests: &state.project_manifests,
        materialized_current_lockfile: state.current_lockfile.as_ref(),
    })?;

    Ok(())
}

async fn apply<Reporter: self::Reporter + 'static>(
    mut inputs: ApplyMaterializationInputs<'_, '_>,
) -> Result<(), InstallError> {
    let peer_catalogs =
        inputs.completion.catalog_context_present.then_some(&inputs.completion.catalogs);
    // What this run installed: a `--filter`ed install acts only on its
    // selection, every other one on the whole workspace. The lockfile
    // can hold more — importers a filtered run left alone, or ones
    // `pruneLockfileImporters` has yet to drop.
    let installed_importer_ids =
        inputs.projects.importers.requested_ids.unwrap_or(inputs.projects.importers.real_ids);
    tracing::info!(target: "pacquet::install", "Complete all");

    if complete_resolve_only::<Reporter>(&ResolveOnlyCompletionInputs {
        mode: crate::install::state_options::CompletionMode {
            resolve_only: inputs.mode.resolve_only,
            dry_run: inputs.mode.dry_run,
            peer_issues_sink_is_none: inputs.mode.peer_issues_sink_is_none,
        },
        peers: crate::install::state_options::PeerIssueLockfiles {
            wanted: inputs.resolution.existing_wanted,
            fresh: inputs.materialized.fresh_lockfile.as_ref(),
            importer_ids: &inputs.materialized.peer_issue_importer_ids,
        },

        prefix: &inputs.completion.prefix,
        config: inputs.completion.config,
        catalogs: peer_catalogs,
        workspace_root: &inputs.projects.workspace_root,
        installed_importer_ids,
    })? {
        return Ok(());
    }

    let metadata = take_committed_metadata(&mut inputs.materialized);
    let state = select_apply_state(&inputs);

    link_apply_projects::<Reporter>(&inputs, &state).await?;

    commit_apply_state::<Reporter>(&inputs, &state, metadata)?;

    let MaterializedState {
        selected_current_lockfile,
        current_lockfile,
        ..
    } = state;
    finish_apply::<Reporter>(inputs, selected_current_lockfile, current_lockfile)
}

fn finish_apply<Reporter: self::Reporter>(
    mut inputs: ApplyMaterializationInputs<'_, '_>,
    selected_current_lockfile: Option<Lockfile>,
    current_lockfile: Option<Lockfile>,
) -> Result<(), InstallError> {
    let peer_catalogs =
        inputs.completion.catalog_context_present.then_some(&inputs.completion.catalogs);
    let installed_importer_ids =
        inputs.projects.importers.requested_ids.unwrap_or(inputs.projects.importers.real_ids);
    // Nothing below reads the materialized lockfiles, and each holds a
    // workspace-scale importer map.
    pnpm_fs::background_drop((
        selected_current_lockfile,
        current_lockfile,
        inputs.prior.lockfile.take(),
    ));

    // Refreshing the root here would hide stale bins in unselected projects
    // from the next install.
    if !(inputs.prior.tree_moved && inputs.projects.filtered_install) {
        write_applied_workspace_state(&inputs)?;
    }

    let completion = report_install_completion::<Reporter>(ReportInstallCompletionInputs {
        workspace: crate::install::state_options::CompletionWorkspace {
            config: inputs.completion.config,
            catalogs: peer_catalogs,
            workspace_root: &inputs.projects.workspace_root,
            workspace_manifest_dir: &inputs.completion.workspace_manifest_dir,
        },

        prefix: inputs.completion.prefix,
        ignored_builds: inputs.materialized.ignored_builds,
        verified_file_integrity_baseline: inputs.completion.verified_file_integrity_baseline,
        resolved_lockfile: inputs.materialized.fresh_lockfile.as_ref(),
        peer_issue_importer_ids: &inputs.materialized.peer_issue_importer_ids,
        installed_importer_ids,
    });
    pnpm_fs::background_drop(inputs.materialized.fresh_lockfile);
    completion
}

// Publish workspace freshness only after modules.yaml and the current lockfile are committed.
fn write_applied_workspace_state(
    inputs: &ApplyMaterializationInputs<'_, '_>,
) -> Result<(), InstallError> {
    let phase_start = std::time::Instant::now();
    // Write `node_modules/.pnpm-workspace-state-v1.json`.
    // pnpm's `verifyDepsBeforeRun` gate bails to "outdated" the
    // moment this file is missing, forcing `pnpm install` to rerun.
    // Writing it after both the `.modules.yaml` and the current
    // lockfile succeed keeps the file pointing at a fully committed
    // install.
    update_workspace_state(
        &inputs.projects.workspace_root,
        &build_workspace_state::<Host>(
            &inputs.projects.workspace_root,
            inputs.completion.config,
            inputs.projects.node_linker,
            inputs.projects.included,
            inputs.projects.supported_architectures.as_ref(),
            &inputs.completion.catalogs,
            inputs.projects.importers.manifests,
            inputs.projects.filtered_install,
            filesystem_now_ms(&inputs.projects.workspace_root),
        ),
    )
    .map_err(InstallError::WriteWorkspaceState)?;
    tracing::info!(target: "pacquet::install::phase", phase = "apply.workspace_state", elapsed_ms = phase_start.elapsed().as_millis() as u64, "phase complete");

    Ok(())
}
