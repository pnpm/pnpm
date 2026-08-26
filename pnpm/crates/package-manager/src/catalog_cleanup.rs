//! Shared workspace-manifest persistence for the manifest-mutating
//! commands (`add`, `update`, `remove`): merge freshly resolved catalog
//! entries and, under `catalogPrune`, drop the entries no
//! workspace project references anymore. One write covers both, the
//! same single read-modify-write upstream's `updateWorkspaceManifest`
//! performs.
//!
//! A second, post-install write runs the
//! `minimumReleaseAgeExcludePrune` pass: it needs the lockfile
//! the install just wrote (the catalog write happens before the install
//! so the resolver reads the new entries back), so it cannot ride along.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::{LoadLockfileError, Lockfile};
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::{
    FindWorkspaceDirError, FindWorkspaceProjectsError, FindWorkspaceProjectsOpts, Project,
    ReadWorkspaceManifestError, find_workspace_dir, find_workspace_projects,
    read_workspace_manifest, workspace_package_patterns,
};
use pnpm_workspace_manifest_writer::{
    ResolvedPackageVersions, UpdateWorkspaceManifestError, UpdateWorkspaceManifestOptions,
    update_workspace_manifest,
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
    LoadLockfile(#[error(source)] LoadLockfileError),

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
    if updated_catalogs.is_empty() && !config.catalog_prune {
        return Ok(());
    }
    let workspace_dir = match workspace_dir {
        Some(dir) => dir.to_path_buf(),
        None => derive_workspace_dir(current_manifest)?,
    };
    let projects =
        if config.catalog_prune { load_cleanup_projects(&workspace_dir)? } else { Vec::new() };
    let all_projects = manifest_refs_with_current(&projects, current_manifest);
    update_workspace_manifest(
        &workspace_dir,
        &UpdateWorkspaceManifestOptions {
            updated_catalogs: Some(updated_catalogs),
            catalog_prune: config.catalog_prune,
            all_projects: &all_projects,
            ..Default::default()
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
    if updated_catalogs.is_empty() && !config.catalog_prune {
        return Ok(());
    }
    let all_projects: Vec<&PackageManifest> =
        projects.iter().map(|project| &project.manifest).collect();
    update_workspace_manifest(
        workspace_dir,
        &UpdateWorkspaceManifestOptions {
            updated_catalogs: Some(updated_catalogs),
            catalog_prune: config.catalog_prune,
            all_projects: &all_projects,
            ..Default::default()
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

/// Post-install pass under `minimumReleaseAgeExcludePrune`: prune
/// `minimumReleaseAgeExclude` entries whose versions the lockfile written
/// by the just-finished install no longer records.
///
/// The pass may only drop an entry it can prove nothing resolves, so it
/// needs a lockfile covering every project `minimumReleaseAgeExclude`
/// governs — only a shared one does. Under dedicated per-project
/// lockfiles (`sharedWorkspaceLockfile: false` with no `lockfileDir`
/// pinning them back together) every entry a sibling project needs would
/// look unresolved, so the pass no-ops. It also no-ops when the setting
/// is off, when lockfile persistence is disabled (`lockfile: false` — the
/// on-disk lockfile would be stale), and when no lockfile exists,
/// mirroring the `all_projects` guard of the catalog cleanup.
pub(crate) fn post_install_prune(
    config: &Config,
    workspace_dir: Option<&Path>,
    current_manifest: &PackageManifest,
) -> Result<(), WriteWorkspaceCatalogsError> {
    if !config.lockfile || !config.shares_one_lockfile() {
        return Ok(());
    }
    let workspace_dir = match workspace_dir {
        Some(dir) => dir.to_path_buf(),
        None => derive_workspace_dir(current_manifest)?,
    };
    // The entries live in the workspace's `pnpm-workspace.yaml`; the
    // lockfile that proves what still resolves sits wherever
    // `lockfileDir` put it.
    let Some(lockfile) = Lockfile::load_wanted_from_dir(config.lockfile_dir_for(&workspace_dir))
        .map_err(WriteWorkspaceCatalogsError::LoadLockfile)?
    else {
        return Ok(());
    };
    let resolved = resolved_package_versions(&lockfile);
    update_workspace_manifest(
        &workspace_dir,
        &UpdateWorkspaceManifestOptions {
            prune_minimum_release_age_excludes: config.minimum_release_age_exclude_prune,
            prune_allow_builds: true,
            resolved_package_versions: Some(&resolved),
            ..Default::default()
        },
    )
    .map_err(WriteWorkspaceCatalogsError::Write)
}

/// Maps every package in the lockfile to its resolved versions.
/// A registry-qualified slot (`<name>@<registryName>:<version>`)
/// registers the version after the prefix — `PkgVerPeer::version_semver`
/// treats it as opaque for reuse/preference paths, but here the version
/// within the named registry is exactly what a versioned exclude entry
/// names. Packages resolved from a non-semver source (git, tarball,
/// `file:`) register only their name: their presence can still be
/// confirmed (a bare-name exclude entry survives), but no exact version
/// can (a versioned entry is pruned).
fn resolved_package_versions(lockfile: &Lockfile) -> ResolvedPackageVersions {
    let mut resolved = ResolvedPackageVersions::new();
    for key in lockfile.snapshots.iter().flat_map(|snapshots| snapshots.keys()) {
        let versions = resolved.entry(key.name.to_string()).or_default();
        let version = key
            .suffix
            .version_semver()
            .or_else(|| key.suffix.registry_qualified().map(|(_, version)| version));
        if let Some(version) = version {
            versions.insert(version.to_string());
        }
    }
    resolved
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

#[cfg(test)]
mod tests;
