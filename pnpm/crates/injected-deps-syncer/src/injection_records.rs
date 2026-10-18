//! The `.modules.yaml` files that list the injected copies of a package.

use pnpm_workspace::{FindWorkspaceProjectsOpts, find_workspace_projects_no_check};
use std::path::{Path, PathBuf};

use crate::{SyncInjectedDeps, SyncInjectedDepsError, read_workspace_modules};

/// A `.modules.yaml` and the directory its injected-copy paths are
/// relative to: the workspace root, or a project with its own lockfile.
pub(crate) struct InjectionRecord {
    pub(crate) lockfile_dir: PathBuf,
    pub(crate) modules: Option<pnpm_modules_yaml::Modules>,
}

/// The modules manifests that list injected copies. See
/// [`SyncInjectedDeps::shared_workspace_lockfile`].
pub(crate) fn injection_records(
    opts: &SyncInjectedDeps<'_>,
    workspace_dir: &Path,
) -> Result<Vec<InjectionRecord>, SyncInjectedDepsError> {
    let mut records = vec![InjectionRecord {
        lockfile_dir: workspace_dir.to_path_buf(),
        modules: read_workspace_modules(opts.workspace_modules_dir)?,
    }];
    if opts.shared_workspace_lockfile {
        return Ok(records);
    }
    let projects = find_workspace_projects_no_check(
        workspace_dir,
        &FindWorkspaceProjectsOpts {
            patterns: None,
            ignored_directories: opts.bin_links.ignored_directories.clone(),
        },
    )
    .map_err(|error| SyncInjectedDepsError::FindProjects { error })?;
    let workspace_root = pnpm_fs::lexical_normalize(workspace_dir);
    for project in projects {
        if pnpm_fs::lexical_normalize(&project.root_dir) == workspace_root {
            continue;
        }
        let modules =
            read_workspace_modules(&project.root_dir.join(opts.bin_links.modules_dir_name))?;
        records.push(InjectionRecord { lockfile_dir: project.root_dir, modules });
    }
    Ok(records)
}

/// The resolved directories that hold injected copies of `content_source_dir`,
/// or `None` if there are none to sync.
pub(crate) fn resolved_injected_targets(
    opts: &SyncInjectedDeps<'_>,
    lockfile_dir: &Path,
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
        .get(&injected_dep_key(lockfile_dir, source_dir))
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
            .map(|target_dir| lockfile_dir.join(target_dir))
            .collect(),
    )
}

/// The key `.modules.yaml` files an injected dependency under: the
/// package's path relative to the manifest's lockfile directory, with
/// forward slashes on every host.
fn injected_dep_key(lockfile_dir: &Path, pkg_root_dir: &Path) -> String {
    pnpm_fs::relative_path(lockfile_dir, pkg_root_dir).to_string_lossy().replace('\\', "/")
}

/// Every copy of `source` that `records` list.
pub(crate) fn targets_of_source(
    opts: &SyncInjectedDeps<'_>,
    records: &[InjectionRecord],
    source: &Path,
) -> Vec<PathBuf> {
    records
        .iter()
        .filter_map(|record| {
            resolved_injected_targets(opts, &record.lockfile_dir, source, record.modules.as_ref())
        })
        .flatten()
        .collect()
}
