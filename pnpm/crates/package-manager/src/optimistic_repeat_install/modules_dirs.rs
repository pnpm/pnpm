use super::{OptimisticRepeatInstallCheck, manifest_has_runtime_deps, manifest_string_field};
use pnpm_config::{Config, NodeLinker};
use pnpm_fs::lexical_normalize;
use pnpm_lockfile::{
    Lockfile, MaybeLazyLockfile, PkgName, ProjectSnapshot, ResolvedDependencySpec,
};
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_workspace::importer_id_from_root_dir;
use pnpm_workspace_state::WorkspaceState;
use rayon::prelude::*;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

pub(super) fn modules_dirs_present(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
) -> bool {
    first_project_missing_modules_dir(check, state).is_none()
}
/// The id (`name` field, falling back to the root dir) of the first
/// project that declares dependencies but has no modules directory, or
/// `None` when every project with dependencies has one.
pub(super) fn first_project_missing_modules_dir(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
) -> Option<String> {
    first_missing_modules_dir(check, state, &|_| true)
}
/// [`first_project_missing_modules_dir`] restricted to the projects the gate
/// selected.
pub(super) fn first_selected_project_missing_modules_dir(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    selected_project_dirs: &[&Path],
) -> Option<String> {
    let selected: HashSet<PathBuf> = selected_project_dirs
        .iter()
        .map(|dir| lexical_normalize(dir))
        .collect();
    first_missing_modules_dir(check, state, &|root_dir| {
        selected.contains(&lexical_normalize(root_dir))
    })
}
/// The id of the first selected project that declares dependencies but that
/// the current lockfile does not list among its importers.
///
/// After a filtered install, a modules directory does not prove that the
/// install materialized a project: only the importers the current lockfile
/// lists were. The current lockfile is read only when a selected project needs
/// the proof.
pub(super) fn first_selected_project_missing_from_current_lockfile(
    check: &OptimisticRepeatInstallCheck<'_>,
    selected_project_dirs: &[&Path],
) -> Option<String> {
    let selected: HashSet<PathBuf> = selected_project_dirs
        .iter()
        .map(|dir| lexical_normalize(dir))
        .collect();
    let mut needing_install = check.project_manifests
        .iter()
        .filter(|(root_dir, manifest)| {
            selected.contains(&lexical_normalize(root_dir)) && manifest_has_runtime_deps(manifest)
        })
        .peekable();
    needing_install.peek()?;
    let current = Lockfile::load_current_from_install_state_dir(&check.config.install_state_dir)
        .ok()
        .flatten();
    needing_install
        .find(|(root_dir, _)| {
            let importer_id = importer_id_from_root_dir(check.workspace_root, root_dir);
            !current
                .as_ref()
                .is_some_and(|current| current.importers.contains_key(&importer_id))
        })
        .map(|(root_dir, manifest)| project_id(root_dir, manifest))
}

/// The first project that needs a modules directory and lacks one, over the
/// projects `is_selected` accepts.
fn first_missing_modules_dir(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    is_selected: &dyn Fn(&Path) -> bool,
) -> Option<String> {
    first_project_without_modules_dir(check, is_selected)
        .or_else(|| first_project_missing_recorded_hoisted_modules_dir(check, state, is_selected))
}

/// The hoisted linker gives a sibling its own modules directory only for the
/// dependencies it nests there, so [`first_project_without_modules_dir`] cannot
/// require one. The last install recorded which siblings have one
/// ([`pnpm_workspace_state::ProjectEntry::has_modules_dir`]); this returns
/// the first of them that no longer does. The workspace root is left out: the
/// workspace state lives in its modules directory, and
/// [`first_project_without_modules_dir`] covers it.
fn first_project_missing_recorded_hoisted_modules_dir(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    is_selected: &dyn Fn(&Path) -> bool,
) -> Option<String> {
    if check.layout.node_linker != NodeLinker::Hoisted {
        return None;
    }
    check.project_manifests
        .iter()
        .find_map(|(root_dir, manifest)| {
            let missing = is_selected(root_dir)
                && lexical_normalize(root_dir) != lexical_normalize(check.workspace_root)
                && state.projects
                    .get(&*root_dir.to_string_lossy())
                    .is_some_and(|entry| entry.has_modules_dir)
                && !hoisted_project_modules_dir(root_dir).is_dir();
            missing.then(|| project_id(root_dir, manifest))
        })
}

/// Where the hoisted linker nests the dependencies of the workspace project at
/// `root_dir` that it cannot hoist to the root. The walker always uses
/// `node_modules` here, whatever `modulesDir` says.
pub(crate) fn hoisted_project_modules_dir(root_dir: &Path) -> PathBuf {
    root_dir.join("node_modules")
}

fn project_id(root_dir: &Path, manifest: &PackageManifest) -> String {
    manifest_string_field(manifest, "name")
        .unwrap_or_else(|| root_dir.to_string_lossy().into_owned())
}

/// The first project that declares dependencies but has no modules directory
/// where its linker needs one. Under the hoisted linker only the root's is
/// required.
///
/// Under `dedupeDirectDeps` a sibling whose every direct dependency resolves to
/// the same target as the root's gets nothing linked, so the linker never
/// creates its modules directory; such a sibling is installed all the same and
/// does not count as missing one.
fn first_project_without_modules_dir(
    check: &OptimisticRepeatInstallCheck<'_>,
    is_selected: &dyn Fn(&Path) -> bool,
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
            if !is_selected(root_dir) {
                return None;
            }
            let is_root = lexical_normalize(root_dir) == lexical_normalize(workspace_root);
            let installed = !manifest_has_runtime_deps(manifest)
                || modules_dir_exists(node_linker, is_root, root_modules_dir_exists, || {
                    sibling_modules_dir(config, root_dir, manifest)
                })
                || (!is_root
                    && root_modules_dir_exists
                    && config.dedupe_direct_deps
                    && dedupe_links_nothing(
                        lockfile,
                        &included_groups(included),
                        DedupeImporters {
                            lockfile_root: workspace_root,
                            root_dir: workspace_root,
                            sibling_dir: root_dir,
                        },
                    ));
            (!installed).then(|| project_id(root_dir, manifest))
        })
}

/// Whether a direct dependency's entry in its project's modules directory is
/// a link whose target no longer exists. Nothing the fast path records moves
/// when a link is broken or retargeted outside pnpm, and the full install
/// relinks it. A missing entry is not a broken link: skipped optional and
/// excluded dependencies have none, and a healthy entry costs one `stat`.
/// The hoisted linker places a sibling's dependencies in the root modules
/// directory too, so both are probed there.
pub(super) fn direct_dependency_link_dangling(check: &OptimisticRepeatInstallCheck<'_>) -> bool {
    let groups = included_groups(check.layout.included);
    check.project_manifests
        .par_iter()
        .any(|(root_dir, manifest)| {
            project_modules_dirs(check, root_dir, manifest)
                .iter()
                .any(|modules_dir| {
                    manifest
                        .dependencies(groups.iter().copied())
                        .any(|(alias, _)| is_dangling_link(&modules_dir.join(alias)))
                })
        })
}

/// The modules directories the linker may place a project's direct
/// dependencies in.
fn project_modules_dirs(
    check: &OptimisticRepeatInstallCheck<'_>,
    root_dir: &Path,
    manifest: &PackageManifest,
) -> Vec<PathBuf> {
    let config = check.config;
    if lexical_normalize(root_dir) == lexical_normalize(check.workspace_root) {
        return vec![config.modules_dir.clone()];
    }
    let own = sibling_modules_dir(config, root_dir, manifest);
    if check.layout.node_linker == NodeLinker::Hoisted {
        vec![own, config.modules_dir.clone()]
    } else {
        vec![own]
    }
}

fn is_dangling_link(path: &Path) -> bool {
    fs::metadata(path).is_err() && fs::symlink_metadata(path).is_ok()
}

/// The modules directory an isolated install creates for the workspace
/// project at `root_dir`.
fn sibling_modules_dir(config: &Config, root_dir: &Path, manifest: &PackageManifest) -> PathBuf {
    root_dir.join(config.modules_dir_name_for(
        root_dir,
        manifest_string_field(manifest, "name").as_deref(),
    ))
}

/// The root importer uses `config.modules_dir`; under the isolated linker
/// each sibling has its own, which the last argument computes.
fn modules_dir_exists(
    node_linker: NodeLinker,
    is_root: bool,
    root_modules_dir_exists: bool,
    sibling_modules_dir: impl FnOnce() -> PathBuf,
) -> bool {
    match node_linker {
        NodeLinker::Hoisted => root_modules_dir_exists,
        NodeLinker::Isolated | NodeLinker::Pnp | NodeLinker::Cas => {
            if is_root {
                root_modules_dir_exists
            } else {
                sibling_modules_dir().is_dir()
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
        (included.includes_project_optional_dependencies(), DependencyGroup::Optional),
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
