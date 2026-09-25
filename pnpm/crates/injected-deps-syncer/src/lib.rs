//! Refresh every injected copy of a workspace package.
//!
//! An injected dependency is materialized as a tree of hardlinks into
//! the virtual store. A build script that rewrites the source package
//! writes new inodes, which the copies do not share, so after such a
//! script the copies have to be diffed against the source and patched
//! in place. `syncInjectedDepsAfterScripts` names the scripts that
//! should trigger it.

pub use dir_patcher::{
    Change, DirDiff, DirPatcher, FileId, InodeMap, PatchError, Value, apply_patch, diff_dir,
    extend_files_map,
};

mod bin_links;
mod dir_patcher;

#[cfg(test)]
mod tests;

use bin_links::{SyncBinLinks, bin_names, sync_bin_links};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_cmd_shim::LinkBinsError;
use pnpm_modules_yaml::{ReadModulesError, read_modules_manifest};
use pnpm_package_manifest::PackageManifestError;
use pnpm_workspace::FindWorkspaceProjectsError;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
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

    #[diagnostic(transparent)]
    Patch(PatchError),

    #[display("Failed to remove the bin link at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_INJECTED_DEPS_SYNC_REMOVE_BIN))]
    RemoveBin {
        path: PathBuf,
        #[error(source)]
        error: std::io::Error,
    },

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
    /// The name of the workspace's modules directories, `node_modules`
    /// unless `modulesDir` says otherwise.
    pub modules_dir_name: &'a std::ffi::OsStr,
    /// The workspace root's resolved modules directory, which holds the
    /// `.modules.yaml` that lists the injected copies. A `modulesDir` with
    /// a path separator puts it deeper than `modules_dir_name` reaches.
    pub workspace_modules_dir: &'a Path,
    /// pnpm's `extendNodePath`: the relinked shims put a custom modules
    /// directory on `NODE_PATH`, as the install's shims do.
    pub extend_node_path: bool,
    /// The package's manifest as it was before the scripts ran. A script
    /// that drops a bin leaves its shim behind, and the copies cannot say
    /// which bins they used to have: their `package.json` is hardlinked to
    /// the source, so an in-place rewrite has already reached them.
    pub manifest_before_scripts: Option<&'a serde_json::Value>,
    /// Passed to [`pnpm_workspace::FindWorkspaceProjectsOpts::ignored_directories`] when
    /// discovering the projects whose bins are relinked.
    pub ignored_directories: Vec<PathBuf>,
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

    sync_workspace_injected_deps(opts, workspace_dir)
}

fn sync_workspace_injected_deps(
    opts: &SyncInjectedDeps<'_>,
    workspace_dir: &Path,
) -> Result<(), SyncInjectedDepsError> {
    let pkg_root_dir = workspace_dir.join(opts.pkg_root_dir);
    // A project whose `publishConfig.directory` is injected is tracked
    // under that publish directory, not its own root: the resolver names
    // the dependency `file:<publishDir>` (see `resolve_workspace_package_dir`
    // in `pnpm-resolving-npm-resolver`), and `.modules.yaml` keys
    // `injectedDeps` off that same resolved directory. A `file:` dependency
    // on this project bypasses that redirect, so it is tracked under the
    // project root instead, and its copy has to be patched from there.
    let content_source_dir = publish_source_dir(&pkg_root_dir, opts.manifest_before_scripts);
    let modules = read_workspace_modules(opts.workspace_modules_dir)?;
    let hoisted_bin_dir = hoisted_bin_path(workspace_dir, modules.as_ref());

    let mut source_dirs = vec![content_source_dir.clone()];
    if content_source_dir != pkg_root_dir {
        source_dirs.push(pkg_root_dir);
    }
    for source_dir in &source_dirs {
        sync_injected_deps_from_source(
            opts,
            workspace_dir,
            source_dir,
            modules.as_ref(),
            hoisted_bin_dir.as_deref(),
        )?;
    }
    Ok(())
}

/// Patch every injected copy of `source_dir` and relink its bins.
fn sync_injected_deps_from_source(
    opts: &SyncInjectedDeps<'_>,
    workspace_dir: &Path,
    source_dir: &Path,
    modules: Option<&pnpm_modules_yaml::Modules>,
    hoisted_bin_dir: Option<&Path>,
) -> Result<(), SyncInjectedDepsError> {
    let Some(resolved_targets) =
        resolved_injected_targets(opts, workspace_dir, source_dir, modules)
    else {
        return Ok(());
    };
    if source_not_yet_built(source_dir)? {
        return Ok(());
    }
    patch_targets(source_dir, &resolved_targets)?;

    let previous_bin_names = opts.manifest_before_scripts.map_or_else(Vec::new, |manifest| {
        bin_names(manifest, source_dir)
    });
    // The install hoists bins into the virtual store's own `.bin` as well.
    sync_bin_links(&SyncBinLinks {
        pkg_root_dir: source_dir,
        resolved_targets: &resolved_targets,
        workspace_dir,
        previous_bin_names: &previous_bin_names,
        hoisted_bin_dir,
        ignored_directories: &opts.ignored_directories,
        modules_dir_name: opts.modules_dir_name,
        extend_node_path: opts.extend_node_path,
    })
}

/// Bring the injected copies listed in one modules directory back in step
/// with their sources.
///
/// A project with its own lockfile injects copies of workspace projects
/// whose lifecycle scripts run in their own install, so nothing else syncs
/// those copies after the scripts run. Only the copies of the sources in
/// `source_dirs` are synced. `source_dirs` holds lexically normalized paths.
pub fn sync_injected_deps_of_modules_dir(
    lockfile_dir: &Path,
    modules_dir: &Path,
    source_dirs: &HashSet<PathBuf>,
) -> Result<(), SyncInjectedDepsError> {
    let modules = read_workspace_modules(modules_dir)?;
    let Some(injected_deps) =
        modules.as_ref().and_then(|modules| modules.injected_deps.as_ref())
    else {
        return Ok(());
    };
    for (source_id, target_dirs) in injected_deps {
        let source_dir = pnpm_fs::lexical_normalize(&lockfile_dir.join(source_id));
        if target_dirs.is_empty() || !source_dirs.contains(&source_dir) {
            continue;
        }
        if source_not_yet_built(&source_dir)? {
            continue;
        }
        let resolved_targets: Vec<PathBuf> = target_dirs
            .iter()
            .map(|target_dir| lockfile_dir.join(target_dir))
            .collect();
        patch_targets(&source_dir, &resolved_targets)?;
    }
    Ok(())
}

/// The resolved directories that hold injected copies of `content_source_dir`,
/// or `None` if there are none to sync.
fn resolved_injected_targets(
    opts: &SyncInjectedDeps<'_>,
    workspace_dir: &Path,
    source_dir: &Path,
    modules: Option<&pnpm_modules_yaml::Modules>,
) -> Option<Vec<PathBuf>> {
    let Some(injected_deps) = modules.and_then(|modules| modules.injected_deps.as_ref()) else {
        tracing::debug!(
            target: "pacquet::sync_injected_deps",
            "Skipping sync of injected dependencies because none were detected",
        );
        return None;
    };

    let Some(target_dirs) = injected_deps
        .get(&injected_dep_key(workspace_dir, source_dir))
        .filter(|dirs| !dirs.is_empty())
    else {
        tracing::debug!(
            target: "pacquet::sync_injected_deps",
            pkg_root_dir = ?opts.pkg_root_dir,
            "There are no injected dependencies from this package",
        );
        return None;
    };

    Some(
        target_dirs
            .iter()
            .map(|target_dir| workspace_dir.join(target_dir))
            .collect(),
    )
}

fn read_workspace_modules(
    workspace_modules_dir: &Path,
) -> Result<Option<pnpm_modules_yaml::Modules>, SyncInjectedDepsError> {
    read_modules_manifest::<pnpm_modules_yaml::Host>(workspace_modules_dir)
        .map_err(|error| SyncInjectedDepsError::ReadModules { error })
}

fn hoisted_bin_path(
    workspace_dir: &Path,
    modules: Option<&pnpm_modules_yaml::Modules>,
) -> Option<PathBuf> {
    modules.map(|modules| {
        workspace_dir
            .join(&modules.virtual_store_dir)
            .join("node_modules")
            .join(".bin")
    })
}

/// The directory an injected copy's content is diffed against: a package
/// that publishes from `publishConfig.directory` is injected as the built
/// output of that directory, not its project root, so a copy is patched
/// from there once the script that builds it has run.
pub fn publish_source_dir(pkg_root_dir: &Path, manifest: Option<&serde_json::Value>) -> PathBuf {
    let publish_config = manifest.and_then(|manifest| manifest.get("publishConfig"));
    let publish_dir = publish_config
        .and_then(|config| config.get("directory"))
        .and_then(serde_json::Value::as_str);
    let link_directory = publish_config
        .and_then(|config| config.get("linkDirectory"))
        .and_then(serde_json::Value::as_bool);
    match publish_dir {
        Some(publish_dir) if link_directory != Some(false) => pkg_root_dir.join(publish_dir),
        _ => pkg_root_dir.to_path_buf(),
    }
}

/// A publish directory that has not been built yet reads back empty from
/// `DirectoryFetcher` (so the fetch-time bootstrap in
/// `pnpm-directory-fetcher` can tolerate it too), which would otherwise diff
/// as "the target holds everything the source doesn't" and delete the
/// injected copy's content. Leave the copy alone until the source actually
/// exists; an existing-but-empty source is still synced, since that means
/// the build genuinely produced nothing.
fn source_not_yet_built(source_dir: &Path) -> Result<bool, SyncInjectedDepsError> {
    let exists = source_dir
        .try_exists()
        .map_err(|error| {
            SyncInjectedDepsError::Patch(PatchError::Stat { path: source_dir.to_path_buf(), error })
        })?;
    if exists {
        return Ok(false);
    }
    tracing::debug!(
        target: "pacquet::sync_injected_deps",
        source_dir = ?source_dir,
        "Skipping sync because the source directory does not exist yet",
    );
    Ok(true)
}

fn patch_targets(
    pkg_root_dir: &Path,
    resolved_targets: &[PathBuf],
) -> Result<(), SyncInjectedDepsError> {
    for patcher in DirPatcher::from_multiple_targets(pkg_root_dir, resolved_targets)
        .map_err(SyncInjectedDepsError::Patch)?
    {
        patcher.apply().map_err(SyncInjectedDepsError::Patch)?;
    }
    Ok(())
}

/// The key `.modules.yaml` files an injected dependency under: the
/// package's path relative to the workspace root, with forward slashes
/// on every host.
fn injected_dep_key(workspace_dir: &Path, pkg_root_dir: &Path) -> String {
    pnpm_fs::relative_path(workspace_dir, pkg_root_dir).to_string_lossy().replace('\\', "/")
}
