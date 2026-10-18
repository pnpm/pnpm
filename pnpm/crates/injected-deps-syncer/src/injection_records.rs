//! The `.modules.yaml` files that list the injected copies of a package.

use std::path::{Path, PathBuf};

use crate::{SyncInjectedDeps, SyncInjectedDepsError, hoisted_bin_path, read_workspace_modules};

/// A project with its own lockfile, and the modules directory that holds
/// its `.modules.yaml`.
pub struct ProjectModulesDir {
    pub project_dir: PathBuf,
    pub modules_dir: PathBuf,
}

/// A `.modules.yaml` and the directory its injected-copy paths are
/// relative to: the workspace root, or a project with its own lockfile.
pub(crate) struct InjectionRecord {
    pub(crate) lockfile_dir: PathBuf,
    pub(crate) modules: Option<pnpm_modules_yaml::Modules>,
}

/// An injected copy, and the `.bin` its install hoists the virtual
/// store's bins into.
pub(crate) struct InjectedCopy {
    pub(crate) dir: PathBuf,
    pub(crate) hoisted_bin_dir: Option<PathBuf>,
}

/// The modules manifests that list injected copies: the workspace root's,
/// and each of [`SyncInjectedDeps::project_modules_dirs`].
pub(crate) fn injection_records(
    opts: &SyncInjectedDeps<'_>,
    workspace_dir: &Path,
) -> Result<Vec<InjectionRecord>, SyncInjectedDepsError> {
    let mut records = vec![InjectionRecord {
        lockfile_dir: workspace_dir.to_path_buf(),
        modules: read_workspace_modules(opts.workspace_modules_dir)?,
    }];
    let workspace_root = pnpm_fs::lexical_normalize(workspace_dir);
    for project in opts.project_modules_dirs {
        if pnpm_fs::lexical_normalize(&project.project_dir) == workspace_root {
            continue;
        }
        records.push(InjectionRecord {
            lockfile_dir: project.project_dir.clone(),
            modules: read_workspace_modules(&project.modules_dir)?,
        });
    }
    Ok(records)
}

/// The resolved directories that hold injected copies of `content_source_dir`,
/// or `None` if there are none to sync.
fn resolved_injected_targets(
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
pub(crate) fn copies_of_source(
    opts: &SyncInjectedDeps<'_>,
    records: &[InjectionRecord],
    source: &Path,
) -> Vec<InjectedCopy> {
    records
        .iter()
        .flat_map(|record| {
            let hoisted_bin_dir = hoisted_bin_path(&record.lockfile_dir, record.modules.as_ref());
            resolved_injected_targets(opts, &record.lockfile_dir, source, record.modules.as_ref())
                .unwrap_or_default()
                .into_iter()
                .map(move |dir| InjectedCopy { dir, hoisted_bin_dir: hoisted_bin_dir.clone() })
        })
        .collect()
}
