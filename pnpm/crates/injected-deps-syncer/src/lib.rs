//! Refresh every injected copy of a workspace package.
//!
//! An injected dependency is materialized as a tree of hardlinks into
//! the virtual store. A build script that rewrites the source package
//! writes new inodes, which the copies do not share, so after such a
//! script the copies have to be diffed against the source and patched
//! in place. `syncInjectedDepsAfterScripts` names the scripts that
//! should trigger it.

mod dir_patcher;

pub use dir_patcher::{
    Change, DirDiff, DirPatcher, FileId, InodeMap, PatchError, Value, apply_patch, diff_dir,
    extend_files_map,
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_cmd_shim::{LinkBinsError, PackageBinSource, link_bins, link_bins_of_packages};
use pacquet_modules_yaml::{ReadModulesError, read_modules_manifest};
use pacquet_package_manifest::{PackageManifestError, safe_read_package_json_from_dir};
use pacquet_workspace::{
    FindWorkspaceProjectsError, FindWorkspaceProjectsOpts, find_workspace_projects_no_check,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Error type for [`sync_injected_deps`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum SyncInjectedDepsError {
    #[display("Failed to read the modules manifest: {error}")]
    #[diagnostic(code(ERR_PNPM_INJECTED_DEPS_SYNC_READ_MODULES))]
    ReadModules {
        #[error(source)]
        error: ReadModulesError,
    },

    #[display("Failed to read the manifest of {dir:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_INJECTED_DEPS_SYNC_READ_MANIFEST))]
    ReadManifest {
        dir: PathBuf,
        #[error(source)]
        error: PackageManifestError,
    },

    #[display("Failed to enumerate the workspace projects: {error}")]
    #[diagnostic(code(ERR_PNPM_INJECTED_DEPS_SYNC_FIND_PROJECTS))]
    FindProjects {
        #[error(source)]
        error: FindWorkspaceProjectsError,
    },

    #[display("{_0}")]
    #[diagnostic(transparent)]
    Patch(PatchError),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    LinkBins(LinkBinsError),
}

/// Which package to sync, and where its workspace lives.
pub struct SyncInjectedDeps<'a> {
    /// A package without a name cannot be a dependency, so there is
    /// nothing to sync.
    pub pkg_name: Option<&'a str>,
    pub pkg_root_dir: &'a Path,
    pub workspace_dir: Option<&'a Path>,
}

/// Bring every injected copy of `pkg_root_dir` back in step with it.
pub fn sync_injected_deps(opts: &SyncInjectedDeps<'_>) -> Result<(), SyncInjectedDepsError> {
    if opts.pkg_name.is_none() {
        tracing::debug!(
            target: "pacquet::sync_injected_deps",
            pkg_root_dir = ?opts.pkg_root_dir,
            "Skipping sync as an injected dependency because, without a name, it cannot be a dependency",
        );
        return Ok(());
    }
    // Outside a workspace nothing can be injected, so there is nothing
    // to sync. The setting reaches here anyway because every schema key
    // also reads from `PNPM_CONFIG_*`, and a script that already ran and
    // succeeded must not fail the run over it.
    let Some(workspace_dir) = opts.workspace_dir else {
        tracing::debug!(
            target: "pacquet::sync_injected_deps",
            pkg_root_dir = ?opts.pkg_root_dir,
            "Skipping sync of injected dependencies because there is no workspace",
        );
        return Ok(());
    };

    let pkg_root_dir = workspace_dir.join(opts.pkg_root_dir);
    let modules =
        read_modules_manifest::<pacquet_modules_yaml::Host>(&workspace_dir.join("node_modules"))
            .map_err(|error| SyncInjectedDepsError::ReadModules { error })?;
    let Some(injected_deps) = modules.as_ref().and_then(|modules| modules.injected_deps.as_ref())
    else {
        tracing::debug!(
            target: "pacquet::sync_injected_deps",
            "Skipping sync of injected dependencies because none were detected",
        );
        return Ok(());
    };

    let injected_dep_key = injected_dep_key(workspace_dir, &pkg_root_dir);
    let Some(target_dirs) = injected_deps.get(&injected_dep_key).filter(|dirs| !dirs.is_empty())
    else {
        tracing::debug!(
            target: "pacquet::sync_injected_deps",
            pkg_root_dir = ?opts.pkg_root_dir,
            "There are no injected dependencies from this package",
        );
        return Ok(());
    };

    let resolved_targets: Vec<PathBuf> =
        target_dirs.iter().map(|target_dir| workspace_dir.join(target_dir)).collect();
    for patcher in DirPatcher::from_multiple_targets(&pkg_root_dir, &resolved_targets)
        .map_err(SyncInjectedDepsError::Patch)?
    {
        patcher.apply().map_err(SyncInjectedDepsError::Patch)?;
    }

    sync_bin_links(&pkg_root_dir, &resolved_targets, workspace_dir)
}

/// The key `.modules.yaml` files an injected dependency under: the
/// package's path relative to the workspace root, with forward slashes
/// on every host.
fn injected_dep_key(workspace_dir: &Path, pkg_root_dir: &Path) -> String {
    pacquet_fs::relative_path(workspace_dir, pkg_root_dir).to_string_lossy().replace('\\', "/")
}

/// Re-link the bins of a package whose files just changed: a build
/// script can add, remove, or rewrite a bin, and the shims in the
/// consuming projects have to follow.
fn sync_bin_links(
    pkg_root_dir: &Path,
    resolved_targets: &[PathBuf],
    workspace_dir: &Path,
) -> Result<(), SyncInjectedDepsError> {
    let manifest = safe_read_package_json_from_dir(pkg_root_dir).map_err(|error| {
        SyncInjectedDepsError::ReadManifest { dir: pkg_root_dir.to_path_buf(), error }
    })?;
    let has_bin_and_name = manifest
        .as_ref()
        .is_some_and(|manifest| manifest.get("bin").is_some() && manifest.get("name").is_some());
    if !has_bin_and_name {
        return Ok(());
    }
    let manifest = Arc::new(manifest.expect("checked above"));

    for target_dir in resolved_targets {
        let Some(parent_modules_dir) = target_dir.parent() else {
            continue;
        };
        let packages = [PackageBinSource::new(target_dir.clone(), Arc::clone(&manifest))];
        link_bins_of_packages::<pacquet_cmd_shim::Host>(
            &packages,
            &parent_modules_dir.join(".bin"),
            &[],
        )
        .map_err(SyncInjectedDepsError::LinkBins)?;
    }

    // Any project in the workspace may consume the injected package, so
    // every project's bin directory is refreshed rather than only the
    // ones this sync touched.
    let projects =
        find_workspace_projects_no_check(workspace_dir, &FindWorkspaceProjectsOpts::default())
            .map_err(|error| SyncInjectedDepsError::FindProjects { error })?;
    for project in projects {
        let project_modules_dir = project.root_dir.join("node_modules");
        link_bins::<pacquet_cmd_shim::Host>(
            &project_modules_dir,
            &project_modules_dir.join(".bin"),
            &[],
        )
        .map_err(SyncInjectedDepsError::LinkBins)?;
    }
    Ok(())
}
