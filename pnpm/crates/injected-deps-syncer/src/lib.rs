//! Refresh every injected copy of a workspace package.
//!
//! An injected dependency is materialized as a tree of hardlinks into
//! the virtual store. A build script that rewrites the source package
//! writes new inodes, which the copies do not share, so after such a
//! script the copies have to be diffed against the source and patched
//! in place. `syncInjectedDepsAfterScripts` names the scripts that
//! should trigger it. While one of those scripts is still running,
//! the copies are refreshed as independent files so a watcher on the
//! injected package sees each write.

pub use dir_patcher::{
    Change, DirDiff, DirPatcher, FileId, InodeMap, PatchError, Value, apply_patch, diff_dir,
    extend_files_map,
};
pub use injection_records::ProjectModulesDir;

mod bin_links;
mod dir_patcher;
mod injection_records;

#[cfg(test)]
mod tests;

use bin_links::{SyncBinLinks, bin_names, sync_bin_links};
use derive_more::{Display, Error};
use injection_records::{InjectionRecord, copies_of_source, injection_records};
use miette::Diagnostic;
use pnpm_cmd_shim::LinkBinsError;
use pnpm_modules_yaml::{ReadModulesError, read_modules_manifest};
use pnpm_package_manifest::PackageManifestError;
use pnpm_workspace::FindWorkspaceProjectsError;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc::{self, RecvTimeoutError},
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
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
    /// The workspace root's resolved modules directory, which holds the
    /// `.modules.yaml` that lists the injected copies. A `modulesDir` with
    /// a path separator puts it deeper than
    /// [`BinLinkSettings::modules_dir_name`] reaches.
    pub workspace_modules_dir: &'a Path,
    pub bin_links: BinLinkSettings<'a>,
    /// Selects the source files the install put in the copies, as
    /// [`sync_injected_deps_of_modules_dir`] does, so the sync adds no file
    /// the install left out.
    pub include_only_package_files: bool,
    /// The package's manifest as it was before the scripts ran. A script
    /// that drops a bin leaves its shim behind, and the copies cannot say
    /// which bins they used to have: their `package.json` is hardlinked to
    /// the source, so an in-place rewrite has already reached them.
    pub manifest_before_scripts: Option<&'a serde_json::Value>,
    /// The projects with their own lockfile (`sharedWorkspaceLockfile:
    /// false`), whose `.modules.yaml` lists the copies injected into them.
    /// Empty with a shared lockfile, where the workspace root's lists every
    /// copy.
    pub project_modules_dirs: &'a [ProjectModulesDir],
}

/// How the workspace links bins, so the shims the sync relinks match the
/// ones the install made.
pub struct BinLinkSettings<'a> {
    /// The name of the workspace's modules directories, `node_modules`
    /// unless `modulesDir` says otherwise.
    pub modules_dir_name: &'a std::ffi::OsStr,
    /// pnpm's `extendNodePath`: the relinked shims put a custom modules
    /// directory on `NODE_PATH`, as the install's shims do.
    pub extend_node_path: bool,
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
    let records = injection_records(opts, workspace_dir)?;

    let mut source_dirs = vec![content_source_dir.clone()];
    if content_source_dir != pkg_root_dir {
        source_dirs.push(pkg_root_dir);
    }
    for source_dir in &source_dirs {
        sync_injected_deps_from_source(opts, workspace_dir, source_dir, &records)?;
    }
    Ok(())
}

/// Patch every injected copy of `source_dir` that `records` list, and
/// relink its bins.
fn sync_injected_deps_from_source(
    opts: &SyncInjectedDeps<'_>,
    workspace_dir: &Path,
    source_dir: &Path,
    records: &[InjectionRecord],
) -> Result<(), SyncInjectedDepsError> {
    let copies = copies_of_source(opts, records, source_dir);
    if copies.is_empty() || source_not_yet_built(source_dir)? {
        return Ok(());
    }
    let copy_dirs: Vec<PathBuf> = copies
        .iter()
        .map(|copy| copy.dir.clone())
        .collect();
    patch_targets(source_dir, &copy_dirs, opts.include_only_package_files)?;

    // Read from the project root, not `source_dir`: when `source_dir` is a
    // `publishConfig.directory`, `prepare` has already run by the time this
    // is called, so scanning `source_dir` for the pre-script manifest's bins
    // would look at post-build content instead of what existed before.
    let previous_bin_names = opts.manifest_before_scripts.map_or_else(Vec::new, |manifest| {
        bin_names(manifest, &workspace_dir.join(opts.pkg_root_dir))
    });
    // The install hoists bins into the virtual store's own `.bin` as well.
    sync_bin_links(&SyncBinLinks {
        pkg_root_dir: source_dir,
        copies: &copies,
        workspace_dir,
        previous_bin_names: &previous_bin_names,
        settings: &opts.bin_links,
    })
}

/// Bring the injected copies listed in one modules directory back in step
/// with their sources.
///
/// A project with its own lockfile injects copies of workspace projects
/// whose lifecycle scripts run in their own install, so nothing else syncs
/// those copies after the scripts run. Only the copies of the sources in
/// `source_dirs` are synced. `source_dirs` holds lexically normalized paths.
/// `include_only_package_files` selects the source files the install put in
/// the copies, so the sync adds no file the install left out.
pub fn sync_injected_deps_of_modules_dir(
    lockfile_dir: &Path,
    modules_dir: &Path,
    source_dirs: &HashSet<PathBuf>,
    include_only_package_files: bool,
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
        patch_targets(&source_dir, &resolved_targets, include_only_package_files)?;
    }
    Ok(())
}

/// The sources [`sync_injected_deps_of_modules_dir`] syncs from, given each
/// project's root and its manifest as read before its lifecycle scripts ran:
/// the root, plus the `publishConfig.directory` injected in place of it.
pub fn injected_source_dirs<'a>(
    projects: impl IntoIterator<Item = (&'a Path, Option<&'a serde_json::Value>)>,
) -> HashSet<PathBuf> {
    projects
        .into_iter()
        .flat_map(|(root_dir, manifest)| {
            [
                pnpm_fs::lexical_normalize(root_dir),
                pnpm_fs::lexical_normalize(&publish_source_dir(root_dir, manifest)),
            ]
        })
        .collect()
}

/// A source directory and the injected copies a running script should
/// publish from it.
pub struct InjectedEditSource {
    pub source: PathBuf,
    pub targets: Vec<PathBuf>,
    /// See [`SyncInjectedDeps::include_only_package_files`].
    pub include_only_package_files: bool,
}

/// The same sources [`sync_injected_deps`] patches from: the
/// `publishConfig.directory` of the manifest read before the script, and the
/// project root when that differs. Empty when the package has no name, no
/// workspace, or no injected copies, or when the modules manifest cannot be
/// read. [`sync_injected_deps`] reports that error after the script, so it
/// does not stop the script from starting.
#[must_use]
pub fn injected_edit_sources(opts: &SyncInjectedDeps<'_>) -> Vec<InjectedEditSource> {
    let (Some(_), Some(workspace_dir)) = (opts.pkg_name, opts.workspace_dir) else {
        return Vec::new();
    };
    let records = match injection_records(opts, workspace_dir) {
        Ok(records) => records,
        Err(error) => {
            tracing::debug!(
                target: "pacquet::sync_injected_deps",
                "Not publishing injected dependencies while the script is running: {error}",
            );
            return Vec::new();
        }
    };
    let pkg_root_dir = workspace_dir.join(opts.pkg_root_dir);
    let content_source_dir = publish_source_dir(&pkg_root_dir, opts.manifest_before_scripts);
    let mut sources = vec![content_source_dir.clone()];
    if content_source_dir != pkg_root_dir {
        sources.push(pkg_root_dir);
    }
    sources
        .into_iter()
        .filter_map(|source| {
            let targets: Vec<PathBuf> = copies_of_source(opts, &records, &source)
                .into_iter()
                .map(|copy| copy.dir)
                .collect();
            if targets.is_empty() {
                return None;
            }
            Some(InjectedEditSource {
                source,
                targets,
                include_only_package_files: opts.include_only_package_files,
            })
        })
        .collect()
}

/// Polls injected copies until [`InjectedEditWatch::stop`] or drop.
///
/// The thread joins on stop, so the end-of-script hardlink sync does not
/// run beside a publish.
pub struct InjectedEditWatch {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl InjectedEditWatch {
    pub fn stop(&mut self) {
        // Dropping the sender wakes the thread out of its wait.
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for InjectedEditWatch {
    fn drop(&mut self) {
        self.stop();
    }
}

const PUBLISH_INTERVAL: Duration = Duration::from_millis(200);

/// Publish injected copies on a short interval for as long as the watch lives.
///
/// `edited_since` stays fixed at two seconds before the watch starts, which
/// covers a filesystem's one-second modification-time resolution. An
/// already-copied file is not republished: its inode differs and its
/// modification time was preserved.
#[must_use]
pub fn watch_injected_edits(sources: Vec<InjectedEditSource>) -> InjectedEditWatch {
    let (stop, stopped) = mpsc::channel::<()>();
    let thread = thread::spawn(move || {
        let edited_since = SystemTime::now()
            .checked_sub(Duration::from_secs(2))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        loop {
            for source in &sources {
                publish_to_targets(source, edited_since);
            }
            if !matches!(stopped.recv_timeout(PUBLISH_INTERVAL), Err(RecvTimeoutError::Timeout)) {
                break;
            }
        }
    });
    InjectedEditWatch { stop: Some(stop), thread: Some(thread) }
}

fn publish_to_targets(
    InjectedEditSource {
        source,
        targets,
        include_only_package_files,
    }: &InjectedEditSource,
    edited_since: SystemTime,
) {
    // A publish directory the script has not built yet reads back empty,
    // which would empty the injected copies.
    if !source.is_dir() {
        return;
    }
    let source_files = match dir_patcher::PublishSource::load(source, *include_only_package_files) {
        Ok(source_files) => source_files,
        Err(error) => {
            tracing::debug!(
                target: "pacquet::sync_injected_deps",
                source = ?source,
                "Failed to read an injected dependency while its script is running: {error}",
            );
            return;
        }
    };
    for target in targets {
        if let Err(error) = dir_patcher::publish_edits(&source_files, target, edited_since) {
            tracing::debug!(
                target: "pacquet::sync_injected_deps",
                source = ?source,
                target = ?target,
                "Failed to publish an injected dependency while its script is running: {error}",
            );
        }
    }
}

pub(crate) fn read_workspace_modules(
    workspace_modules_dir: &Path,
) -> Result<Option<pnpm_modules_yaml::Modules>, SyncInjectedDepsError> {
    read_modules_manifest::<pnpm_modules_yaml::Host>(workspace_modules_dir)
        .map_err(|error| SyncInjectedDepsError::ReadModules { error })
}

pub(crate) fn hoisted_bin_path(
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
    include_only_package_files: bool,
) -> Result<(), SyncInjectedDepsError> {
    for patcher in DirPatcher::from_multiple_targets(
        pkg_root_dir,
        resolved_targets,
        include_only_package_files,
    )
    .map_err(SyncInjectedDepsError::Patch)?
    {
        patcher.apply().map_err(SyncInjectedDepsError::Patch)?;
    }
    Ok(())
}
