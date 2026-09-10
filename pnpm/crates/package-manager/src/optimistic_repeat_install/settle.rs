use super::{
    Decision, OptimisticRepeatInstallCheck,
    manifest_agreement::{LinkedPackagesContext, ManifestStat},
    manifest_has_runtime_deps, manifest_string_field,
};
use pnpm_config::{Config, LinkWorkspacePackages, NodeLinker};
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::Host;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::{WorkspaceState, update_workspace_state};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The verdict the fast path can already reach from the current lockfile and
/// the fact that nothing was modified.
pub(super) fn early_repeat_verdict(
    check: &OptimisticRepeatInstallCheck<'_>,
    modified: &[&ManifestStat<'_>],
    lockfile_modified: bool,
) -> Option<Decision> {
    match current_lockfile_unusable_with_non_empty_wanted(check) {
        Ok(true) => return Some(Decision::Skipped { reason: "current lockfile missing" }),
        Ok(false) => {}
        Err(reason) => return Some(Decision::Skipped { reason }),
    }
    if modified.is_empty() && !lockfile_modified {
        return Some(match regenerate_wanted_lockfile_if_missing(check, None) {
            Ok(()) => Decision::UpToDate,
            Err(reason) => Decision::Skipped { reason },
        });
    }
    None
}
/// Restore a missing wanted lockfile and refresh `lastValidatedTimestamp`
/// after the content checks passed, so a repeat run short-circuits earlier.
///
/// The workspace branch rewrites the state; the single-project branch keys
/// its comparisons off the lockfile mtimes instead and leaves the state
/// alone. A failed write only costs the next run a repeat of the content
/// check, so it degrades rather than fails.
pub(super) fn settle_repeat_install(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    loaded_current: Option<Lockfile>,
    filesystem_now: Option<i64>,
) -> Result<(), &'static str> {
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        node_linker,
        included,
        supported_architectures,
        project_manifests,
        is_workspace_install,
        catalogs,
        ..
    } = check;
    regenerate_wanted_lockfile_if_missing(check, loaded_current)?;
    if !is_workspace_install {
        return Ok(());
    }
    // This path refreshes the timestamp without materializing anything, so
    // it carries the previous run's `filtered_install` forward: clearing it
    // would claim every importer is materialized when a filtered install
    // left the unselected ones untouched.
    let new_state = crate::install::build_workspace_state::<Host>(
        workspace_root,
        config,
        node_linker,
        included,
        supported_architectures,
        catalogs,
        project_manifests,
        state.filtered_install,
        filesystem_now,
    );
    if let Err(error) = update_workspace_state(workspace_root, &new_state) {
        tracing::warn!(
            target: "pacquet::install",
            ?error,
            "Failed to refresh the workspace state after the repeat-install content check",
        );
    }
    Ok(())
}
/// Restore a missing `pnpm-lock.yaml` from the current lockfile before
/// the fast path reports "Already up to date", so the short-circuit
/// leaves the same on-disk contract a full install would (the full
/// path synthesizes the wanted lockfile from the current one and
/// rewrites it). No-op when `pnpm-lock.yaml` was loaded, when lockfile
/// writing is disabled (`lockfile: false`), or when there is no
/// current lockfile to restore from (a dependency-less project).
/// A write failure falls through to the full install path rather than
/// reporting up-to-date while leaving the lockfile missing.
pub(super) fn regenerate_wanted_lockfile_if_missing(
    check: &OptimisticRepeatInstallCheck<'_>,
    loaded_current: Option<Lockfile>,
) -> Result<(), &'static str> {
    if check.lockfile.is_loaded_or_on_disk() || !check.config.lockfile {
        return Ok(());
    }
    let current = match loaded_current {
        Some(current) => Some(current),
        None => Lockfile::load_current_from_virtual_store_dir(&check.config.virtual_store_dir)
            .map_err(|_| "the current lockfile cannot be loaded")?,
    };
    let Some(current) = current else {
        return Ok(());
    };
    current
        .save_to_path(&check.workspace_root.join(check.config.wanted_lockfile_name()))
        .map_err(|_| "failed to regenerate the wanted lockfile from the current lockfile")
}
impl<'a> LinkedPackagesContext<'a> {
    pub(super) fn new(
        config: &Config,
        project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    ) -> Self {
        let mut manifests_by_dir = std::collections::HashMap::new();
        let mut workspace_packages: std::collections::HashMap<
            String,
            std::collections::HashMap<String, &'a Path>,
        > = std::collections::HashMap::new();
        for (root_dir, manifest) in project_manifests {
            manifests_by_dir.insert(root_dir.as_path(), *manifest);
            if let (Some(name), Some(version)) = (
                manifest_string_field(manifest, "name"),
                manifest_string_field(manifest, "version"),
            ) {
                workspace_packages.entry(name).or_default().insert(version, root_dir.as_path());
            }
        }
        LinkedPackagesContext {
            link_workspace_packages: config.link_workspace_packages != LinkWorkspacePackages::Off,
            manifests_by_dir,
            workspace_packages,
        }
    }

    /// The version of the package manifest at `dir`, preferring the
    /// already-loaded workspace manifests over a disk read.
    pub(super) fn linked_version(&self, dir: &Path) -> Option<String> {
        if let Some(manifest) = self.manifests_by_dir.get(dir) {
            return manifest_string_field(manifest, "version");
        }
        pnpm_package_manifest::safe_read_package_json_from_dir(dir)
            .ok()
            .flatten()
            .and_then(|value| value.get("version").and_then(|v| v.as_str()).map(str::to_string))
    }
}
pub(super) fn current_lockfile_unusable_with_non_empty_wanted(
    check: &OptimisticRepeatInstallCheck<'_>,
) -> Result<bool, &'static str> {
    if check.is_workspace_install || !check.config.lockfile {
        return Ok(false);
    }
    if current_lockfile_file_has_content(&check.config.virtual_store_dir) {
        return Ok(false);
    }
    let Some(wanted) =
        check.lockfile.get().map_err(|_| "the wanted lockfile cannot be read or parsed")?
    else {
        return Ok(false);
    };
    Ok(!wanted.is_empty())
}
pub(super) fn current_lockfile_file_has_content(virtual_store_dir: &Path) -> bool {
    fs::metadata(virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME))
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}
/// Project count + per-project (key, name, version) match between the
/// cached state and today's walk. The key is the project's root dir;
/// `build_workspace_state` and pnpm both use it as the map key, so a
/// renamed / removed / added project trips the check immediately.
pub(super) fn project_structure_matches(
    state: &WorkspaceState,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    if state.projects.len() != project_manifests.len() {
        return false;
    }
    project_manifests.iter().all(|(root_dir, manifest)| {
        let key = root_dir.to_string_lossy().into_owned();
        let Some(entry) = state.projects.get(&key) else {
            return false;
        };
        entry.name.as_deref() == manifest_string_field(manifest, "name").as_deref()
            && entry.version.as_deref().unwrap_or("0.0.0")
                == manifest_string_field(manifest, "version").as_deref().unwrap_or("0.0.0")
    })
}
pub(super) fn modules_dirs_present(
    config: &Config,
    node_linker: NodeLinker,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    first_project_missing_modules_dir(config, node_linker, project_manifests).is_none()
}
/// The id (`name` field, falling back to the root dir) of the first
/// project that declares dependencies but has no modules directory, or
/// `None` when every project with dependencies has one.
pub(super) fn first_project_missing_modules_dir(
    config: &Config,
    node_linker: NodeLinker,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> Option<String> {
    let root_modules_dir_exists = config.modules_dir.exists();

    project_manifests.iter().find_map(|(root_dir, manifest)| {
        if !manifest_has_runtime_deps(manifest) {
            return None;
        }
        // The root importer uses `config.modules_dir`; siblings use
        // their own `<root>/node_modules`. Matches the isolated-linker
        // default — `config.modules_dir` is `<workspace_root>/node_modules`
        // unless the user overrode it explicitly.
        let modules_dir_exists = match node_linker {
            NodeLinker::Hoisted => root_modules_dir_exists,
            NodeLinker::Isolated | NodeLinker::Pnp => {
                if *root_dir == workspace_dir_of(config, root_dir) {
                    root_modules_dir_exists
                } else {
                    root_dir.join("node_modules").exists()
                }
            }
        };

        (!modules_dir_exists).then(|| {
            manifest_string_field(manifest, "name")
                .unwrap_or_else(|| root_dir.to_string_lossy().into_owned())
        })
    })
}
/// Recover the workspace root from `config.modules_dir`. The root
/// importer's `root_dir` equals `config.modules_dir.parent()` because
/// `config.modules_dir` is `<workspace_root>/node_modules`. Used by
/// [`modules_dirs_present`] to tell root from sibling — a brittle
/// shape but it matches how the install path itself derives
/// `config.modules_dir`.
pub(super) fn workspace_dir_of(config: &Config, fallback: &Path) -> PathBuf {
    config.modules_dir.parent().map_or_else(|| fallback.to_path_buf(), Path::to_path_buf)
}
