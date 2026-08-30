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
use pnpm_cmd_shim::{
    LinkBinsError, PackageBinSource, get_bins_from_package_manifest, link_bins,
    link_bins_of_packages, remove_bin,
};
use pnpm_modules_yaml::{ReadModulesError, read_modules_manifest};
use pnpm_package_manifest::{PackageManifestError, safe_read_package_json_from_dir};
use pnpm_workspace::{
    FindWorkspaceProjectsError, FindWorkspaceProjectsOpts, find_workspace_projects_no_check,
};
use std::{
    collections::HashSet,
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

    #[display("Failed to remove the bin link at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_INJECTED_DEPS_SYNC_REMOVE_BIN))]
    RemoveBin {
        path: PathBuf,
        #[error(source)]
        error: std::io::Error,
    },

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
    /// The package's manifest as it was before the scripts ran. A script
    /// that drops a bin leaves its shim behind, and the copies cannot say
    /// which bins they used to have: their `package.json` is hardlinked to
    /// the source, so an in-place rewrite has already reached them.
    pub manifest_before_scripts: Option<&'a serde_json::Value>,
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
        read_modules_manifest::<pnpm_modules_yaml::Host>(&workspace_dir.join("node_modules"))
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

    let previous_bin_names = opts.manifest_before_scripts.map_or_else(Vec::new, |manifest| {
        get_bins_from_package_manifest::<pnpm_cmd_shim::Host>(manifest, &pkg_root_dir)
            .into_iter()
            .map(|command| command.name)
            .collect()
    });
    // The install hoists bins into the virtual store's own `.bin` as well.
    let hoisted_bin_dir = modules.as_ref().map(|modules| {
        workspace_dir.join(&modules.virtual_store_dir).join("node_modules").join(".bin")
    });
    sync_bin_links(&SyncBinLinks {
        pkg_root_dir: &pkg_root_dir,
        resolved_targets: &resolved_targets,
        workspace_dir,
        previous_bin_names: &previous_bin_names,
        hoisted_bin_dir: hoisted_bin_dir.as_deref(),
    })
}

/// The key `.modules.yaml` files an injected dependency under: the
/// package's path relative to the workspace root, with forward slashes
/// on every host.
fn injected_dep_key(workspace_dir: &Path, pkg_root_dir: &Path) -> String {
    pnpm_fs::relative_path(workspace_dir, pkg_root_dir).to_string_lossy().replace('\\', "/")
}

/// Re-link the bins of a package whose files just changed: a build
/// script can add, remove, or rewrite a bin, and the shims in the
/// consuming projects have to follow.
struct SyncBinLinks<'a> {
    pkg_root_dir: &'a Path,
    resolved_targets: &'a [PathBuf],
    workspace_dir: &'a Path,
    previous_bin_names: &'a [String],
    hoisted_bin_dir: Option<&'a Path>,
}

fn sync_bin_links(opts: &SyncBinLinks<'_>) -> Result<(), SyncInjectedDepsError> {
    let SyncBinLinks {
        pkg_root_dir,
        resolved_targets,
        workspace_dir,
        previous_bin_names,
        hoisted_bin_dir,
    } = *opts;
    let manifest = safe_read_package_json_from_dir(pkg_root_dir).map_err(|error| {
        SyncInjectedDepsError::ReadManifest { dir: pkg_root_dir.to_path_buf(), error }
    })?;
    let Some(manifest) = manifest.filter(|manifest| manifest.get("name").is_some()) else {
        return Ok(());
    };

    // `link_bins` only ever creates shims, so a bin the script dropped keeps
    // its shim, pointing at a command that is no longer there.
    let current_bin_names: HashSet<String> =
        get_bins_from_package_manifest::<pnpm_cmd_shim::Host>(&manifest, pkg_root_dir)
            .into_iter()
            .map(|command| command.name)
            .collect();
    let stale_bin_names: Vec<&String> =
        previous_bin_names.iter().filter(|name| !current_bin_names.contains(*name)).collect();

    let has_bins = manifest.get("bin").is_some();
    let manifest = Arc::new(manifest);

    for target_dir in resolved_targets {
        let Some(parent_modules_dir) = target_dir.parent() else {
            continue;
        };
        // The installer writes an injected package's own bins inside the
        // copy, while this function writes them beside it. A dropped bin has
        // to be cleared from both, or the one this function never wrote
        // survives.
        let bin_dirs =
            [parent_modules_dir.join(".bin"), target_dir.join("node_modules").join(".bin")];
        for bin_dir in bin_dirs.iter().map(PathBuf::as_path).chain(hoisted_bin_dir) {
            for name in &stale_bin_names {
                remove_bin(&bin_dir.join(name.as_str())).map_err(|error| {
                    SyncInjectedDepsError::RemoveBin { path: bin_dir.join(name.as_str()), error }
                })?;
            }
        }

        if !has_bins {
            continue;
        }
        let packages = [PackageBinSource::new(target_dir.clone(), Arc::clone(&manifest))];
        link_bins_of_packages::<pnpm_cmd_shim::Host>(
            &packages,
            &parent_modules_dir.join(".bin"),
            &pnpm_cmd_shim::LinkBinsOptions::default(),
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
        // A stale name another package legitimately owns is put back by the
        // relink below, so removing first costs nothing and catches the shim
        // this package left behind.
        let project_bin_dir = project_modules_dir.join(".bin");
        for name in &stale_bin_names {
            remove_bin(&project_bin_dir.join(name.as_str())).map_err(|error| {
                SyncInjectedDepsError::RemoveBin {
                    path: project_bin_dir.join(name.as_str()),
                    error,
                }
            })?;
        }
        link_bins::<pnpm_cmd_shim::Host>(
            &project_modules_dir,
            &project_modules_dir.join(".bin"),
            &pnpm_cmd_shim::LinkBinsOptions::default(),
        )
        .map_err(SyncInjectedDepsError::LinkBins)?;
    }
    Ok(())
}
