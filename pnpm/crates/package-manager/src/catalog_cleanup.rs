//! Shared workspace-manifest persistence for the manifest-mutating
//! commands (`add`, `update`, `remove`): merge freshly resolved catalog
//! entries and, under `cleanupUnusedCatalogs`, drop the entries no
//! workspace project references anymore. One write covers both, the
//! same single read-modify-write upstream's `updateWorkspaceManifest`
//! performs.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_types::Catalogs;
use pacquet_config::Config;
use pacquet_package_manifest::PackageManifest;
use pacquet_workspace::{
    FindWorkspaceDirError, FindWorkspaceProjectsError, FindWorkspaceProjectsOpts, Project,
    ReadWorkspaceManifestError, find_workspace_dir, find_workspace_projects,
    read_workspace_manifest, workspace_package_patterns,
};
use pacquet_workspace_manifest_writer::{
    UpdateWorkspaceManifestError, UpdateWorkspaceManifestOptions, update_workspace_manifest,
};
use std::path::{Path, PathBuf};

/// Failure modes of the workspace-manifest write (including the
/// project discovery the cleanup pass needs).
#[derive(Debug, Display, Error, Diagnostic)]
pub enum WriteWorkspaceCatalogsError {
    #[diagnostic(transparent)]
    FindWorkspaceDir(#[error(source)] FindWorkspaceDirError),

    #[diagnostic(transparent)]
    ReadWorkspaceManifest(#[error(source)] ReadWorkspaceManifestError),

    #[diagnostic(transparent)]
    FindWorkspaceProjects(#[error(source)] FindWorkspaceProjectsError),

    #[diagnostic(transparent)]
    Write(#[error(source)] UpdateWorkspaceManifestError),
}

/// Single-project variant: `current_manifest` (whose in-memory
/// dependency edits may not be on disk yet) stands in for its project
/// in the reference scan. The workspace dir is derived from the
/// manifest's directory when the caller has not already resolved one.
pub(crate) fn write_workspace_catalogs(
    config: &Config,
    workspace_dir: Option<&Path>,
    updated_catalogs: &Catalogs,
    current_manifest: &PackageManifest,
) -> Result<(), WriteWorkspaceCatalogsError> {
    if updated_catalogs.is_empty() && !config.cleanup_unused_catalogs {
        return Ok(());
    }
    let workspace_dir = match workspace_dir {
        Some(dir) => dir.to_path_buf(),
        None => derive_workspace_dir(current_manifest)?,
    };
    let projects = if config.cleanup_unused_catalogs {
        load_cleanup_projects(&workspace_dir)?
    } else {
        Vec::new()
    };
    let all_projects = manifest_refs_with_current(&projects, current_manifest);
    update_workspace_manifest(
        &workspace_dir,
        &UpdateWorkspaceManifestOptions {
            updated_catalogs: Some(updated_catalogs),
            cleanup_unused_catalogs: config.cleanup_unused_catalogs,
            all_projects: &all_projects,
        },
    )
    .map_err(WriteWorkspaceCatalogsError::Write)
}

/// Workspace variant: `projects` already carries every project with its
/// in-memory manifest edits, so the reference scan uses it directly.
pub(crate) fn write_workspace_catalogs_selected(
    config: &Config,
    workspace_dir: &Path,
    updated_catalogs: &Catalogs,
    projects: &[Project],
) -> Result<(), WriteWorkspaceCatalogsError> {
    if updated_catalogs.is_empty() && !config.cleanup_unused_catalogs {
        return Ok(());
    }
    let all_projects: Vec<&PackageManifest> =
        projects.iter().map(|project| &project.manifest).collect();
    update_workspace_manifest(
        workspace_dir,
        &UpdateWorkspaceManifestOptions {
            updated_catalogs: Some(updated_catalogs),
            cleanup_unused_catalogs: config.cleanup_unused_catalogs,
            all_projects: &all_projects,
        },
    )
    .map_err(WriteWorkspaceCatalogsError::Write)
}

fn derive_workspace_dir(
    current_manifest: &PackageManifest,
) -> Result<PathBuf, WriteWorkspaceCatalogsError> {
    let manifest_dir = current_manifest
        .path()
        .parent()
        .expect("manifest path always has a parent dir")
        .to_path_buf();
    let workspace_dir = find_workspace_dir(&manifest_dir)
        .map_err(WriteWorkspaceCatalogsError::FindWorkspaceDir)?
        .unwrap_or(manifest_dir);
    Ok(workspace_dir)
}

/// Every project manifest under `workspace_dir`, read from disk. An
/// absent `pnpm-workspace.yaml` yields no projects, which disables the
/// cleanup pass — there is no workspace manifest to clean either.
fn load_cleanup_projects(
    workspace_dir: &Path,
) -> Result<Vec<Project>, WriteWorkspaceCatalogsError> {
    let Some(workspace_manifest) = read_workspace_manifest(workspace_dir)
        .map_err(WriteWorkspaceCatalogsError::ReadWorkspaceManifest)?
    else {
        return Ok(Vec::new());
    };
    let opts = FindWorkspaceProjectsOpts {
        patterns: Some(workspace_package_patterns(&workspace_manifest)),
    };
    find_workspace_projects(workspace_dir, &opts)
        .map_err(WriteWorkspaceCatalogsError::FindWorkspaceProjects)
}

/// `projects` with `current` standing in for its on-disk manifest, and
/// appended when discovery did not surface its project at all.
fn manifest_refs_with_current<'a>(
    projects: &'a [Project],
    current: &'a PackageManifest,
) -> Vec<&'a PackageManifest> {
    let mut refs: Vec<&PackageManifest> = Vec::with_capacity(projects.len() + 1);
    let mut replaced = false;
    for project in projects {
        if project.manifest.path() == current.path() {
            refs.push(current);
            replaced = true;
        } else {
            refs.push(&project.manifest);
        }
    }
    if !replaced {
        refs.push(current);
    }
    refs
}
