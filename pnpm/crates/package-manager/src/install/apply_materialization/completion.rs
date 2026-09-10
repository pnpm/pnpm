use super::super::{
    BTreeSet, Catalogs, Config, GlobalLog, HashSet, Host, InstallError, Lockfile, LogEvent,
    LogLevel, NodeLinker, PackageManifest, Path, PathBuf, ProjectMutation, ProjectScriptsInputs,
    RebuildOptions, Reporter, SummaryLog, WorkspaceInstallSelection, drain_settled_projects,
    project_lifecycle_graph, projects_running_own_scripts, run_projects_lifecycle_scripts,
};
use crate::peer_dependency_issues::report_peer_dependency_issues;
use pnpm_store_dir::VerifiedFileIntegrity;
use std::time::Duration;

pub(super) struct ResolveOnlyCompletionInputs<'a> {
    pub(super) resolve_only: bool,
    pub(super) dry_run: bool,
    pub(super) peer_issues_sink_is_none: bool,
    pub(super) existing_wanted_lockfile: Option<&'a Lockfile>,
    pub(super) peer_issue_importer_ids: &'a HashSet<String>,
    pub(super) fresh_lockfile: Option<&'a Lockfile>,
    pub(super) prefix: &'a str,
    pub(super) config: &'static Config,
    pub(super) catalogs: Option<&'a Catalogs>,
    pub(super) workspace_root: &'a Path,
    pub(super) installed_importer_ids: &'a HashSet<String>,
}
pub(super) fn complete_resolve_only<Reporter: self::Reporter>(
    inputs: &ResolveOnlyCompletionInputs<'_>,
) -> Result<bool, InstallError> {
    if !inputs.resolve_only {
        return Ok(false);
    }

    // A sink-driven dry run is a programmatic query, not a CLI preview.
    if inputs.dry_run && inputs.peer_issues_sink_is_none {
        use std::io::Write as _;
        let report =
            crate::lockfile_diff::render_dry_run_report(&crate::lockfile_diff::diff_lockfiles(
                inputs.existing_wanted_lockfile,
                inputs.fresh_lockfile,
                crate::lockfile_diff::ImporterDiffKey::Specifier,
            ));
        let mut stdout = std::io::stdout();
        let _ = writeln!(stdout, "{report}");
        let _ = stdout.flush();
    }
    // A programmatic peer-issue query asks for the issues; it must not
    // also be told about them, nor fail over them.
    if inputs.peer_issues_sink_is_none {
        report_peer_dependency_issues::<Reporter>(
            inputs.fresh_lockfile,
            inputs.peer_issue_importer_ids,
            inputs.installed_importer_ids,
            inputs.workspace_root,
            inputs.config,
            inputs.catalogs,
        )?;
    }
    Reporter::emit(&LogEvent::Summary(SummaryLog {
        level: LogLevel::Debug,
        prefix: inputs.prefix.to_string(),
    }));
    Ok(true)
}
#[derive(Clone, Copy)]
pub(super) struct MaterializedProjectScriptsInputs<'a, 'selection> {
    pub(super) config: &'static Config,
    pub(super) node_linker: NodeLinker,
    pub(super) workspace_root: &'a Path,
    pub(super) rebuild: Option<&'a RebuildOptions>,
    pub(super) mutation: ProjectMutation,
    pub(super) manifest_dir: &'a Path,
    pub(super) selection: Option<&'a WorkspaceInstallSelection<'selection>>,
    pub(super) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(super) materialized_project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(super) materialized_current_lockfile: Option<&'a Lockfile>,
}
pub(super) fn run_materialized_project_scripts<Reporter: self::Reporter>(
    inputs: MaterializedProjectScriptsInputs<'_, '_>,
) -> Result<(), InstallError> {
    let projects_to_run = materialized_script_projects(&inputs);
    if !projects_to_run.is_empty() {
        let project_graph = project_lifecycle_graph(
            &projects_to_run,
            inputs.selection.map(|selection| selection.project_dependencies),
            inputs.workspace_root,
            inputs.materialized_current_lockfile,
        )?;
        if !project_graph.dependencies.is_empty() {
            run_projects_lifecycle_scripts::<Reporter>(
                &project_graph,
                inputs.config,
                inputs.node_linker,
                inputs.workspace_root,
            )?;
        }
        if let Some(rebuild) = inputs.rebuild {
            drain_settled_projects::<Host>(&inputs.config.modules_dir, &rebuild.pending_projects)?;
        }
    }

    Ok(())
}
// Rebuild's pending projects are script candidates even though rebuilding mutates no manifest.
pub(super) fn materialized_script_projects<'a>(
    inputs: &MaterializedProjectScriptsInputs<'a, '_>,
) -> Vec<(PathBuf, &'a PackageManifest)> {
    if inputs.config.ignore_scripts || inputs.config.virtual_store_only {
        Vec::new()
    } else if let Some(rebuild) = inputs.rebuild {
        inputs
            .materialized_project_manifests
            .iter()
            .filter(|(project_dir, _)| {
                let importer_id =
                    pnpm_workspace::importer_id_from_root_dir(inputs.workspace_root, project_dir);
                rebuild.pending_projects.contains(&importer_id)
            })
            .cloned()
            .collect()
    } else {
        projects_running_own_scripts(&ProjectScriptsInputs {
            mutation: inputs.mutation,
            workspace_root: inputs.workspace_root,
            active_project_dir: inputs.manifest_dir,
            selected_dirs: inputs.selection.map(|selection| selection.selected_dirs),
            project_manifests: inputs.project_manifests,
            materialized_project_manifests: inputs.materialized_project_manifests,
        })
    }
}
pub(super) struct ReportInstallCompletionInputs<'a> {
    pub(super) config: &'static Config,
    pub(super) catalogs: Option<&'a Catalogs>,
    pub(super) workspace_root: &'a Path,
    pub(super) workspace_manifest_dir: &'a Path,
    pub(super) prefix: String,
    pub(super) ignored_builds: Vec<String>,
    pub(super) verified_file_integrity_baseline: VerifiedFileIntegrity,
    /// The lockfile this install resolved, or `None` when it skipped
    /// resolution — which is what decides whether peer-dependency
    /// issues are reported at all.
    pub(super) resolved_lockfile: Option<&'a Lockfile>,
    pub(super) peer_issue_importer_ids: &'a HashSet<String>,
    pub(super) installed_importer_ids: &'a HashSet<String>,
}
pub(super) fn report_install_completion<Reporter: self::Reporter>(
    inputs: ReportInstallCompletionInputs<'_>,
) -> Result<(), InstallError> {
    // Reported before the summary and before the ignored-builds
    // failure below, matching where pnpm places the verdict: last in
    // the install, first among the ways it can still fail.
    report_peer_dependency_issues::<Reporter>(
        inputs.resolved_lockfile,
        inputs.peer_issue_importer_ids,
        inputs.installed_importer_ids,
        inputs.workspace_root,
        inputs.config,
        inputs.catalogs,
    )?;
    // `pnpm:summary` closes the install and lets the reporter render
    // the accumulated `pnpm:root` events as a "+N -M" block. Must
    // come after `importing_done`.
    Reporter::emit(&LogEvent::Summary(SummaryLog {
        level: LogLevel::Debug,
        prefix: inputs.prefix,
    }));

    report_verified_file_integrity::<Reporter>(
        VerifiedFileIntegrity::snapshot().since(inputs.verified_file_integrity_baseline),
    );

    // A global install is exempt from the scaffold below: its root is a
    // throwaway per-group directory, and the approval prompt that
    // follows it records the ignored builds against the stable global
    // packages dir instead.
    let is_global_install = inputs
        .config
        .global_pkg_dir
        .as_deref()
        .is_some_and(|global_pkg_dir| inputs.workspace_root.starts_with(global_pkg_dir));
    // Leave the user a line to edit in `pnpm-workspace.yaml` for every
    // build this install blocked, so approving one is an edit rather
    // than recalling the `allowBuilds` shape. Written before the strict
    // failure below, which is the very run whose message it answers.
    // `--ignore-workspace` opts out: the run disowned the workspace
    // manifest, so it must not write to one either.
    if !inputs.ignored_builds.is_empty() && !is_global_install && !inputs.config.ignore_workspace {
        let allow_build_keys: BTreeSet<String> = inputs
            .ignored_builds
            .iter()
            .map(|dep_path| crate::allow_build_key_from_ignored_build(dep_path))
            .collect();
        pnpm_workspace_manifest_writer::scaffold_allow_builds(
            inputs.workspace_manifest_dir,
            allow_build_keys.iter().map(String::as_str),
        )
        .map_err(InstallError::ScaffoldAllowBuilds)?;
    }

    // When `strictDepBuilds` is on (the default), an install that
    // blocked any dependency build script fails with
    // `ERR_PNPM_IGNORED_BUILDS` *after* the artifacts are written, so
    // the package is still added/installed and the user approves the
    // builds and reinstalls.
    if inputs.config.strict_dep_builds && !inputs.ignored_builds.is_empty() {
        return Err(InstallError::IgnoredBuilds { package_names: inputs.ignored_builds });
    }

    Ok(())
}
/// Spending this long re-hashing store files is well past what a
/// healthy store needs, so the install owns up to the time.
pub(super) const VERIFIED_FILE_INTEGRITY_SLOW: Duration = Duration::from_secs(1);
/// Re-hashing this many files says something keeps invalidating the
/// store even when the hashing itself was quick — worth telling the
/// user about before the store grows and the same churn does cost them
/// time.
pub(super) const VERIFIED_FILE_INTEGRITY_MANY: u64 = 1000;
/// Tell the user when store verification re-hashed files: how much time
/// it cost, or failing that, that it happened at all on a scale a
/// healthy store never reaches. The two are separate claims, so they
/// are separate messages, and the timed one wins when both hold — it
/// carries the file count anyway.
///
/// `verified` covers this install alone, and its `duration` is summed
/// across the threads that did the hashing — see
/// [`pnpm_store_dir::VerifiedFileIntegrity`].
///
/// The seconds are rounded to tenths in integer arithmetic rather than
/// by float formatting: pnpm renders the same messages from the same
/// figures and the two have to agree character for character, but
/// Rust's `{:.1}` rounds a tie to even where JavaScript's `toFixed`
/// rounds it up.
pub(in super::super) fn report_verified_file_integrity<Reporter: self::Reporter>(
    verified: VerifiedFileIntegrity,
) {
    let files = verified.files;
    let message = if verified.duration > VERIFIED_FILE_INTEGRITY_SLOW {
        let tenths = (verified.duration.as_millis() + 50) / 100;
        format!("The integrity of {files} files was checked in {}.{}s.", tenths / 10, tenths % 10)
    } else if files > VERIFIED_FILE_INTEGRITY_MANY {
        format!(
            "The integrity of {files} files was checked, because their timestamps changed since the store recorded them. A backup tool, an antivirus scan, or a copied store can cause this.",
        )
    } else {
        return;
    };
    Reporter::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Info, message }));
}
