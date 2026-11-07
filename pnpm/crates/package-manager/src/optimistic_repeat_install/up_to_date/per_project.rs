//! The up-to-date fast path for a workspace whose projects keep their own
//! lockfiles: each project is decided from the manifest record in its own
//! workspace state, with the configuration anchored to it.

use super::{
    Catalogs, Config, CoveredProjects, FastPathContext, FastPathProjects, Path, PathBuf,
    UpToDateFastPathCheck, UpToDateWorkspace, WorkspaceState, check_fast_path_repeat_install,
    configured_or_discovered_workspace_dir, fast_path_workspace_context, lazy_wanted_lockfile,
    load_fast_path_state, register_fast_path_workspace,
};
use crate::install_state::validation::{dedicated_bin_projects, manifest_stand_ins};
use pnpm_workspace::{Project, ProjectManifestPaths};
use rayon::prelude::*;
use std::collections::HashMap;

/// [`super::install_already_up_to_date_from_records`] for a workspace with
/// `sharedWorkspaceLockfile: false`. Every project the workspace lists is
/// checked against its own state, as the install's per-project runs would
/// check it, and the projects are registered in the store once all of them
/// are up to date.
///
/// `Err` as soon as a project cannot be decided from its record: it has no
/// state or no usable record, its check fails, or its declaration is
/// malformed. Also `Err` while `packageConfigs` is set or the global virtual
/// store is on, whose per-project settings and build markers the records do
/// not carry, and while patches are configured.
pub fn install_already_up_to_date_per_project(
    check: &UpToDateFastPathCheck<'_>,
) -> Result<UpToDateWorkspace, &'static str> {
    let config = check.config;
    if let Some(reason) = config_blocks_per_project(config) {
        return Err(reason);
    }
    let manifest_dir = check.manifest
        .path()
        .parent()
        .ok_or("the manifest has no directory")?;
    let workspace_root = configured_or_discovered_workspace_dir(config, manifest_dir)
        .map_err(|_| "the workspace root cannot be located")?
        .ok_or("there is no workspace")?;
    let (workspace_manifest, catalogs) = fast_path_workspace_context(config, Some(&workspace_root))
        .ok_or("the workspace manifest or its catalogs cannot be read")?;
    let workspace = ListedWorkspace {
        root: &workspace_root,
        manifest: workspace_manifest.as_ref().ok_or("there is no workspace manifest")?,
        catalogs: &catalogs,
    };
    let (projects, records) =
        recorded_projects(check, &workspace)?.into_iter().unzip::<_, _, Vec<_>, Vec<_>>();
    projects
        .par_iter()
        .zip(&records)
        .try_for_each(|(project, record)| {
            check_project(check, &workspace, (project, record), &projects)
        })?;
    projects
        .par_iter()
        .zip(&records)
        .try_for_each(|(project, record)| {
            register_fast_path_workspace(&record.config, &project.root_dir)
        })?;
    let manifests = projects
        .iter()
        .map(|project| (project.root_dir.as_path(), &project.manifest));
    Ok(CoveredProjects::new(
        Some(projects.len()),
        manifests,
        check.config.runtime_on_fail.map(pnpm_config::RuntimeOnFail::as_str),
    )
    .into_workspace(workspace_root.clone()))
}

/// The configuration that keeps the per-project fast path off.
fn config_blocks_per_project(config: &Config) -> Option<&'static str> {
    if config.package_configs
        .as_ref()
        .is_some_and(|configs| !configs.is_empty())
    {
        return Some("packageConfigs gives projects their own settings");
    }
    if config.enable_global_virtual_store {
        return Some("the global virtual store keeps build markers per project");
    }
    // The install verifies the patches against every project lockfile it
    // finds, which no single project's check covers, such as a patch only a
    // removed project used.
    if config.patched_dependencies
        .as_ref()
        .is_some_and(|patches| !patches.is_empty())
    {
        return Some("patches are verified against every project lockfile");
    }
    None
}

/// The workspace the projects are listed in.
struct ListedWorkspace<'a> {
    root: &'a Path,
    manifest: &'a pnpm_workspace::WorkspaceManifest,
    catalogs: &'a Catalogs,
}

/// What the check of one project reads besides its manifest.
struct ProjectRecord {
    /// The recorded digest of its manifest, `None` for the manifest the
    /// command read from disk.
    digest: Option<String>,
    state: WorkspaceState,
    /// The configuration anchored to the project.
    config: Config,
}

/// Every project the workspace lists, each with a manifest rebuilt from its
/// own state's record, or the one the command read for its own directory.
fn recorded_projects(
    check: &UpToDateFastPathCheck<'_>,
    workspace: &ListedWorkspace<'_>,
) -> Result<Vec<(Project, ProjectRecord)>, &'static str> {
    let listed = {
        let _span =
            tracing::debug_span!(target: "pacquet::install", "list_workspace_projects").entered();
        let opts = crate::install::checked_projects_opts(check.config, workspace.manifest);
        pnpm_workspace::find_workspace_project_manifests(workspace.root, &opts)
            .map_err(|_| "the workspace projects cannot be listed")?
    };
    let active_dir = check.manifest
        .path()
        .parent()
        .map(pnpm_fs::lexical_normalize);
    listed
        .into_par_iter()
        .filter(|listed| listed.candidates.iter().any(|candidate| candidate.is_file()))
        .map(|listed| recorded_project(check, &listed, active_dir.as_deref()))
        .collect()
}

fn recorded_project(
    check: &UpToDateFastPathCheck<'_>,
    listed: &ProjectManifestPaths,
    active_dir: Option<&Path>,
) -> Result<(Project, ProjectRecord), &'static str> {
    let root_dir = listed.root_dir.clone();
    let mut config = check.config.clone();
    config.anchor_dedicated_project(&root_dir, None);
    let state = load_fast_path_state(&config, &root_dir, &root_dir)?;
    if active_dir == Some(pnpm_fs::lexical_normalize(&root_dir).as_path()) {
        let project =
            Project { root_dir, manifest: check.manifest.clone(), dependency_manifest: None };
        return Ok((project, ProjectRecord { digest: None, state, config }));
    }
    let validation =
        state.validation.as_ref().ok_or("a project's state has no validation record")?;
    let mut stand_ins = manifest_stand_ins(&root_dir, validation, std::slice::from_ref(listed))
        .ok_or("a project's recorded manifest cannot decide")?;
    let project = stand_ins.projects.pop().ok_or("a project's manifest file is gone")?;
    let digest = stand_ins.digests.remove(&project.root_dir);
    Ok((project, ProjectRecord { digest, state, config }))
}

/// The repeat-install check of one project, as its own install would run it.
fn check_project(
    check: &UpToDateFastPathCheck<'_>,
    workspace: &ListedWorkspace<'_>,
    (project, record): (&Project, &ProjectRecord),
    listed: &[Project],
) -> Result<(), &'static str> {
    let root_dir = &project.root_dir;
    let manifests = [(root_dir.clone(), &project.manifest)];
    // The install refuses its shortcut for a malformed dependency declaration,
    // which only a `readPackage` hook that repairs it lets the install pass.
    if project.manifest.validate_dependency_types().is_err() {
        return Err("a recorded manifest declares a malformed dependency");
    }
    let recorded_digests: Option<HashMap<PathBuf, String>> = record.digest
        .clone()
        .map(|digest| HashMap::from([(root_dir.clone(), digest)]));
    let bin_projects = dedicated_bin_projects(&record.config, &manifests, Some(listed));
    let lockfile = lazy_wanted_lockfile(&record.config, root_dir);
    let context = FastPathContext {
        state_root: root_dir,
        lockfile_root: root_dir,
        workspace_root: workspace.root,
        workspace_manifest: Some(workspace.manifest),
        lockfile: &lockfile,
        catalogs: workspace.catalogs,
    };
    let anchored = UpToDateFastPathCheck {
        config: &record.config,
        manifest: check.manifest,
        dependency_groups: check.dependency_groups.clone(),
        node_linker: check.node_linker,
        supported_architectures: check.supported_architectures.clone(),
    };
    check_fast_path_repeat_install(
        &anchored,
        &context,
        FastPathProjects {
            state: &record.state,
            manifests: &manifests,
            recorded_digests: recorded_digests.as_ref(),
            bin_projects: bin_projects.as_deref(),
        },
    )
}
