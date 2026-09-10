pub(super) mod completion;

use completion::{
    MaterializedProjectScriptsInputs, ReportInstallCompletionInputs, ResolveOnlyCompletionInputs,
    complete_resolve_only, report_install_completion, run_materialized_project_scripts,
};

mod commit;
use commit::{CommitModulesStateInputs, commit_modules_state};

mod selection;
use selection::{
    LinkMaterializedProjectsInputs, MaterializedState, SelectMaterializedStateInputs,
    link_materialized_projects, select_materialized_state,
};

use super::{
    BTreeMap, Catalogs, Config, HashSet, HoistedDependencies, Host, IncludedDependencies,
    InstallError, Lockfile, Materialized, Modules, NodeLinker, PackageManifest, Path, PathBuf,
    ProjectMutation, RebuildOptions, Reporter, WorkspaceInstallSelection, build_workspace_state,
    update_workspace_state,
};
use crate::optimistic_repeat_install::filesystem_now_ms;
use pnpm_store_dir::VerifiedFileIntegrity;

pub(super) struct ApplyMaterializationInputs<'a, 'selection> {
    pub(super) resolve_only: bool,
    pub(super) dry_run: bool,
    pub(super) peer_issues_sink_is_none: bool,
    pub(super) existing_wanted_lockfile: Option<&'a Lockfile>,
    pub(super) materialized: Materialized,
    pub(super) prefix: String,
    pub(super) lockfile: Option<&'a Lockfile>,
    pub(super) requested_importer_ids: Option<&'a HashSet<String>>,
    pub(super) workspace_root: PathBuf,
    pub(super) workspace_manifest_dir: PathBuf,
    pub(super) included: IncludedDependencies,
    pub(super) node_linker: NodeLinker,
    pub(super) current_lockfile: Option<Lockfile>,
    pub(super) real_importer_ids: &'a HashSet<String>,
    pub(super) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(super) filtered_install: bool,
    pub(super) is_inconsistent: bool,
    pub(super) previous_modules_metadata: Option<Modules>,
    pub(super) config: &'static Config,
    pub(super) modules_manifest: Option<pnpm_modules_yaml::ModulesLayout>,
    pub(super) rebuild: Option<RebuildOptions>,
    pub(super) take_frozen_path: bool,
    pub(super) lockfile_synthesized_from_current: bool,
    pub(super) lockfile_was_fast_updated: bool,
    pub(super) save_lockfile: bool,
    pub(super) mutation: ProjectMutation,
    pub(super) manifest_dir: &'a Path,
    pub(super) selection: Option<WorkspaceInstallSelection<'selection>>,
    pub(super) supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    pub(super) catalogs: Catalogs,
    pub(super) catalog_context_present: bool,
    pub(super) verified_file_integrity_baseline: VerifiedFileIntegrity,
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
        fresh_lockfile: inputs.materialized.fresh_lockfile.as_ref(),
        loaded_wanted_lockfile: inputs.lockfile,
        requested_importer_ids: inputs.requested_importer_ids,
        real_importer_ids: inputs.real_importer_ids,
        workspace_root: &inputs.workspace_root,
        included: inputs.included,
        install_skipped: &inputs.materialized.install_skipped,
        node_linker: inputs.node_linker,
        current_lockfile: inputs.current_lockfile.as_ref(),
        is_inconsistent: inputs.is_inconsistent,
        project_manifests: inputs.project_manifests,
    })
}

async fn link_apply_projects<Reporter: self::Reporter + 'static>(
    inputs: &ApplyMaterializationInputs<'_, '_>,
    state: &MaterializedState<'_>,
) -> Result<(), InstallError> {
    let phase_start = std::time::Instant::now();
    link_materialized_projects::<Reporter>(LinkMaterializedProjectsInputs {
        filtered_install: inputs.filtered_install,
        node_linker: inputs.node_linker,
        config: inputs.config,
        current_lockfile: state.current_lockfile.as_ref(),
        wanted_lockfile: state.wanted_lockfile,
        workspace_root: &inputs.workspace_root,
        project_manifests: inputs.project_manifests,
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
        hoisted_dependencies: std::mem::take(&mut materialized.hoisted_dependencies),
        hoisted_locations: std::mem::take(&mut materialized.hoisted_locations),
        injected_deps: std::mem::take(&mut materialized.injected_deps),
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
        prior_modules: inputs.modules_manifest.as_ref(),
        config: inputs.config,
        workspace_root: &inputs.workspace_root,
        materialized_current_lockfile: state.current_lockfile.as_ref(),
        selected_current_lockfile: state.selected_current_lockfile.as_ref(),
        materialized_project_manifests: &state.project_manifests,
        included: inputs.included,
        install_skipped: &inputs.materialized.install_skipped,
        node_linker: inputs.node_linker,
        filtered_install: inputs.filtered_install,
        is_inconsistent: inputs.is_inconsistent,
        previous_modules_metadata: inputs.previous_modules_metadata.as_ref(),
        hoisted_dependencies: metadata.hoisted_dependencies,
        hoisted_locations: metadata.hoisted_locations,
        injected_deps: metadata.injected_deps,
        ignored_builds: &inputs.materialized.ignored_builds,
        deferred_builds: metadata.deferred_builds,
        rebuild: inputs.rebuild.as_ref(),
        take_frozen_path: inputs.take_frozen_path,
        lockfile_synthesized_from_current: inputs.lockfile_synthesized_from_current,
        lockfile_was_fast_updated: inputs.lockfile_was_fast_updated,
        save_lockfile: inputs.save_lockfile,
        loaded_wanted_lockfile: inputs.lockfile,
    })?;
    tracing::info!(target: "pacquet::install::phase", phase = "apply.commit_modules_state", elapsed_ms = phase_start.elapsed().as_millis() as u64, "phase complete");

    run_apply_scripts::<Reporter>(inputs, state)
}

fn run_apply_scripts<Reporter: self::Reporter>(
    inputs: &ApplyMaterializationInputs<'_, '_>,
    state: &MaterializedState<'_>,
) -> Result<(), InstallError> {
    run_materialized_project_scripts::<Reporter>(MaterializedProjectScriptsInputs {
        config: inputs.config,
        node_linker: inputs.node_linker,
        workspace_root: &inputs.workspace_root,
        rebuild: inputs.rebuild.as_ref(),
        mutation: inputs.mutation,
        manifest_dir: inputs.manifest_dir,
        selection: inputs.selection.as_ref(),
        project_manifests: inputs.project_manifests,
        materialized_project_manifests: &state.project_manifests,
        materialized_current_lockfile: state.current_lockfile.as_ref(),
    })?;

    Ok(())
}

async fn apply<Reporter: self::Reporter + 'static>(
    mut inputs: ApplyMaterializationInputs<'_, '_>,
) -> Result<(), InstallError> {
    let peer_catalogs = inputs.catalog_context_present.then_some(&inputs.catalogs);
    // What this run installed: a `--filter`ed install acts only on its
    // selection, every other one on the whole workspace. The lockfile
    // can hold more — importers a filtered run left alone, or ones
    // `pruneLockfileImporters` has yet to drop.
    let installed_importer_ids = inputs.requested_importer_ids.unwrap_or(inputs.real_importer_ids);
    tracing::info!(target: "pacquet::install", "Complete all");

    if complete_resolve_only::<Reporter>(&ResolveOnlyCompletionInputs {
        resolve_only: inputs.resolve_only,
        dry_run: inputs.dry_run,
        peer_issues_sink_is_none: inputs.peer_issues_sink_is_none,
        existing_wanted_lockfile: inputs.existing_wanted_lockfile,
        peer_issue_importer_ids: &inputs.materialized.peer_issue_importer_ids,
        fresh_lockfile: inputs.materialized.fresh_lockfile.as_ref(),
        prefix: &inputs.prefix,
        config: inputs.config,
        catalogs: peer_catalogs,
        workspace_root: &inputs.workspace_root,
        installed_importer_ids,
    })? {
        return Ok(());
    }

    let metadata = take_committed_metadata(&mut inputs.materialized);
    let state = select_apply_state(&inputs);

    link_apply_projects::<Reporter>(&inputs, &state).await?;

    commit_apply_state::<Reporter>(&inputs, &state, metadata)?;

    let MaterializedState { selected_current_lockfile, current_lockfile, .. } = state;
    finish_apply::<Reporter>(inputs, selected_current_lockfile, current_lockfile)
}

fn finish_apply<Reporter: self::Reporter>(
    mut inputs: ApplyMaterializationInputs<'_, '_>,
    selected_current_lockfile: Option<Lockfile>,
    current_lockfile: Option<Lockfile>,
) -> Result<(), InstallError> {
    let peer_catalogs = inputs.catalog_context_present.then_some(&inputs.catalogs);
    let installed_importer_ids = inputs.requested_importer_ids.unwrap_or(inputs.real_importer_ids);
    // Nothing below reads the materialized lockfiles, and each holds a
    // workspace-scale importer map.
    pnpm_fs::background_drop((
        selected_current_lockfile,
        current_lockfile,
        inputs.current_lockfile.take(),
    ));

    write_applied_workspace_state(&inputs)?;

    let completion = report_install_completion::<Reporter>(ReportInstallCompletionInputs {
        config: inputs.config,
        catalogs: peer_catalogs,
        workspace_root: &inputs.workspace_root,
        workspace_manifest_dir: &inputs.workspace_manifest_dir,
        prefix: inputs.prefix,
        ignored_builds: inputs.materialized.ignored_builds,
        verified_file_integrity_baseline: inputs.verified_file_integrity_baseline,
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
        &inputs.workspace_root,
        &build_workspace_state::<Host>(
            &inputs.workspace_root,
            inputs.config,
            inputs.node_linker,
            inputs.included,
            inputs.supported_architectures.as_ref(),
            &inputs.catalogs,
            inputs.project_manifests,
            inputs.filtered_install,
            filesystem_now_ms(&inputs.workspace_root),
        ),
    )
    .map_err(InstallError::WriteWorkspaceState)?;
    tracing::info!(target: "pacquet::install::phase", phase = "apply.workspace_state", elapsed_ms = phase_start.elapsed().as_millis() as u64, "phase complete");

    Ok(())
}
