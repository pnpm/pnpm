use super::{
    Decision,
    OptimisticRepeatInstallCheck,
    manifest_agreement::{
        LinkedPackagesContext,
        ManifestStat,
    },
    manifest_has_runtime_deps,
    manifest_string_field,
};
use pnpm_config::{
    Config,
    LinkWorkspacePackages,
    NodeLinker,
};
use pnpm_fs::lexical_normalize;
use pnpm_lockfile::{
    Lockfile,
    MaybeLazyLockfile,
    PkgName,
    ProjectSnapshot,
    ResolvedDependencySpec,
};
use pnpm_modules_yaml::{
    Host,
    IncludedDependencies,
};
use pnpm_package_manifest::{
    DependencyGroup,
    PackageManifest,
};
use pnpm_workspace::importer_id_from_root_dir;
use pnpm_workspace_state::{
    WorkspaceState,
    update_workspace_state,
};
use std::{
    fs,
    path::{
        Path,
        PathBuf,
    },
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
/// alone, unless the tree `moved` and the state has to be recorded where it
/// is now. A failed write only costs the next run a repeat of the content
/// check, so it degrades rather than fails.
pub(super) fn settle_repeat_install(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    loaded_current: Option<Lockfile>,
    filesystem_now: Option<i64>,
    moved: bool,
) -> Result<(), &'static str> {
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        project_manifests,
        is_workspace_install,
        catalogs,
        layout:
            crate::RepeatInstallLayout {
                node_linker,
                included,
                supported_architectures,
                ..
            },
        ..
    } = check;
    regenerate_wanted_lockfile_if_missing(check, loaded_current)?;
    if !is_workspace_install && !moved {
        return Ok(());
    }
    // This path refreshes the timestamp without materializing anything, so
    // it carries the previous run's `filtered_install` forward: clearing it
    // would claim every importer is materialized when a filtered install
    // left the unselected ones untouched.
    let mut new_state = crate::install::build_workspace_state::<Host>(
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
    new_state.settings.auto_dedupe = state.settings.auto_dedupe;
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
                workspace_packages
                    .entry(name)
                    .or_default()
                    .insert(version, root_dir.as_path());
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
            .and_then(|value| {
                value
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
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
    project_manifests
        .iter()
        .all(|(root_dir, manifest)| {
            let key = root_dir.to_string_lossy().into_owned();
            let Some(entry) = state.projects.get(&key) else {
                return false;
            };
            entry.name.as_deref() == manifest_string_field(manifest, "name").as_deref()
                && entry.version.as_deref().unwrap_or("0.0.0")
                    == manifest_string_field(manifest, "version").as_deref().unwrap_or("0.0.0")
        })
}
pub(super) fn modules_dirs_present(check: &OptimisticRepeatInstallCheck<'_>) -> bool {
    first_project_missing_modules_dir(check).is_none()
}
/// The id (`name` field, falling back to the root dir) of the first
/// project that declares dependencies but has no modules directory, or
/// `None` when every project with dependencies has one.
///
/// Under `dedupeDirectDeps` a sibling whose every direct dependency
/// resolves to the same target as the root's gets nothing linked, so the
/// linker never creates its modules directory; such a sibling is installed
/// all the same and does not count as missing one.
pub(super) fn first_project_missing_modules_dir(
    check: &OptimisticRepeatInstallCheck<'_>,
) -> Option<String> {
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        project_manifests,
        lockfile,
        layout: crate::RepeatInstallLayout { node_linker, included, .. },
        ..
    } = check;
    let root_modules_dir_exists = config.modules_dir.is_dir();

    project_manifests
        .iter()
        .find_map(|(root_dir, manifest)| {
            let root_project_dir = workspace_dir_of(config, root_dir);
            let is_root = *root_dir == root_project_dir;
            let installed = !manifest_has_runtime_deps(manifest)
                || modules_dir_exists(node_linker, root_dir, is_root, root_modules_dir_exists)
                || (!is_root
                    && root_modules_dir_exists
                    && config.dedupe_direct_deps
                    && dedupe_links_nothing(
                        lockfile,
                        &included_groups(included),
                        DedupeImporters {
                            lockfile_root: workspace_root,
                            root_dir: &root_project_dir,
                            sibling_dir: root_dir,
                        },
                    ));
            (!installed).then(|| {
                manifest_string_field(manifest, "name")
                    .unwrap_or_else(|| root_dir.to_string_lossy().into_owned())
            })
        })
}

/// The root importer uses `config.modules_dir`; siblings use their own
/// `<root>/node_modules`. Matches the isolated-linker default —
/// `config.modules_dir` is `<workspace_root>/node_modules` unless the user
/// overrode it explicitly.
fn modules_dir_exists(
    node_linker: NodeLinker,
    root_dir: &Path,
    is_root: bool,
    root_modules_dir_exists: bool,
) -> bool {
    match node_linker {
        NodeLinker::Hoisted => root_modules_dir_exists,
        NodeLinker::Isolated | NodeLinker::Pnp => {
            if is_root {
                root_modules_dir_exists
            } else {
                root_dir.join("node_modules").is_dir()
            }
        }
    }
}

/// The dependency groups this install materializes, the only ones the
/// linker links and dedupes.
fn included_groups(included: IncludedDependencies) -> Vec<DependencyGroup> {
    [
        (included.dependencies, DependencyGroup::Prod),
        (included.dev_dependencies, DependencyGroup::Dev),
        (included.optional_dependencies, DependencyGroup::Optional),
    ]
    .into_iter()
    .filter_map(|(included, group)| included.then_some(group))
    .collect()
}

/// The two importers a dedupe verdict compares, as directories.
#[derive(Clone, Copy)]
struct DedupeImporters<'a> {
    /// The directory importer ids are relative to.
    lockfile_root: &'a Path,
    root_dir: &'a Path,
    sibling_dir: &'a Path,
}

/// Whether `dedupeDirectDeps` links nothing into the sibling: for every
/// alias the sibling declares in a materialized group, the wanted lockfile
/// records one target on each side, and the two are the same, which is what
/// the linker compares. An alias declared with differing targets in several
/// groups has one effective target the linker picks by group order; that
/// choice is not reproduced here, so such an alias proves nothing. Neither
/// does a lockfile that cannot be loaded or lacks either importer.
fn dedupe_links_nothing(
    lockfile: MaybeLazyLockfile<'_>,
    groups: &[DependencyGroup],
    importers: DedupeImporters<'_>,
) -> bool {
    let DedupeImporters {
        lockfile_root,
        root_dir,
        sibling_dir,
    } = importers;
    let Ok(Some(lockfile)) = lockfile.get() else { return false };
    let importer =
        |dir: &Path| lockfile.importers.get(&importer_id_from_root_dir(lockfile_root, dir));
    let (Some(root), Some(sibling)) = (importer(root_dir), importer(sibling_dir)) else {
        return false;
    };
    let mut seen = std::collections::HashSet::new();
    sibling
        .dependencies_by_groups(groups.iter().copied())
        .all(|(alias, _)| {
            if !seen.insert(alias) {
                return true;
            }
            let (Some(dep), Some(root_dep)) =
                (sole_target(sibling, groups, alias), sole_target(root, groups, alias))
            else {
                return false;
            };
            resolves_to_same_target(root_dir, root_dep, sibling_dir, dep)
        })
}

/// The one target `importer` resolves `alias` to across `groups`, or `None`
/// when it declares the alias nowhere or with differing targets.
fn sole_target<'a>(
    importer: &'a ProjectSnapshot,
    groups: &[DependencyGroup],
    alias: &PkgName,
) -> Option<&'a ResolvedDependencySpec> {
    let mut declarations = importer
        .dependencies_by_groups(groups.iter().copied())
        .filter(|(declared, _)| *declared == alias)
        .map(|(_, dep)| dep);
    let first = declarations.next()?;
    declarations
        .all(|dep| dep.version == first.version)
        .then_some(first)
}

/// Whether two importer dependencies resolve to one target: the same
/// snapshot, or `link:` paths that name the same directory once resolved
/// against their own importer directories.
fn resolves_to_same_target(
    root_dir: &Path,
    root_dep: &ResolvedDependencySpec,
    sibling_dir: &Path,
    dep: &ResolvedDependencySpec,
) -> bool {
    match (root_dep.version.as_link_target(), dep.version.as_link_target()) {
        (Some(root_target), Some(target)) => {
            lexical_normalize(&root_dir.join(root_target))
                == lexical_normalize(&sibling_dir.join(target))
        }
        (None, None) => root_dep.version == dep.version,
        _ => false,
    }
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
