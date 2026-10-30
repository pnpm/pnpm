//! The direct dependency links of each project: whether one is broken, or a
//! local link the last install made is gone.

use super::{OptimisticRepeatInstallCheck, included_groups, project_modules_dirs};
use crate::optimistic_repeat_install::manifest_string_field;
use pnpm_config::NodeLinker;
use pnpm_fs::lexical_normalize;
use pnpm_lockfile::{Lockfile, PkgName};
use pnpm_package_manifest::DependencyGroup;
use pnpm_workspace::importer_id_from_root_dir;
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// Whether a direct dependency's entry in its project's modules directory is
/// a link whose target no longer exists, or a local link the last install
/// made and that is gone. Nothing the fast path records moves when a link is
/// broken, deleted, or retargeted outside pnpm, and the full install relinks
/// it.
///
/// A missing entry counts only for a `workspace:` or `link:` spec of a
/// project in `held_project_dirs` (every project when `None`), only when the
/// linker symlinks direct dependencies ([`links_local_dependencies`]), and
/// only when [`local_link_missing`] confirms it against the lockfile.
/// A registry dependency may be absent because it is an excluded or skipped
/// optional one, and a healthy entry costs one `stat`, so it stays out of
/// that check.
pub(in crate::optimistic_repeat_install) fn direct_dependency_link_dangling(
    check: &OptimisticRepeatInstallCheck<'_>,
    held_project_dirs: Option<&HashSet<PathBuf>>,
) -> bool {
    let groups = included_groups(check.layout.included);
    let links_local_deps = links_local_dependencies(check);
    let project_dirs_by_name = OnceLock::new();
    check.project_manifests
        .par_iter()
        .any(|(root_dir, manifest)| {
            let modules_dirs = project_modules_dirs(check, root_dir, manifest);
            let held = links_local_deps
                && held_project_dirs.is_none_or(|held| held.contains(&lexical_normalize(root_dir)));
            manifest
                .dependencies(groups.iter().copied())
                .any(|(alias, spec)| {
                    let own_entry = entry_state(&modules_dirs[0].join(alias));
                    own_entry == EntryState::Dangling
                        || modules_dirs[1..]
                            .iter()
                            .any(|modules_dir| {
                                entry_state(&modules_dir.join(alias)) == EntryState::Dangling
                            })
                        || (own_entry == EntryState::Missing
                            && held
                            && is_local_link_spec(spec)
                            && local_link_missing(
                                LocalLinkCheck {
                                    check,
                                    groups: &groups,
                                    project_dirs_by_name: &project_dirs_by_name,
                                },
                                root_dir,
                                &modules_dirs[0],
                                (alias, spec),
                            ))
                })
        })
}

/// Whether the linker symlinks every `link:` dependency of the lockfile into
/// its project's modules directory. `symlink: false` links no direct
/// dependency, and the pnp and loaded linkers lay out no `node_modules` tree.
fn links_local_dependencies(check: &OptimisticRepeatInstallCheck<'_>) -> bool {
    check.config.symlink
        && matches!(check.layout.node_linker, NodeLinker::Isolated | NodeLinker::Hoisted)
}

/// What [`local_link_missing`] reads besides the dependency itself.
#[derive(Clone, Copy)]
struct LocalLinkCheck<'a> {
    check: &'a OptimisticRepeatInstallCheck<'a>,
    groups: &'a [DependencyGroup],
    /// Each workspace project's directory by name, built on first use.
    project_dirs_by_name: &'a OnceLock<HashMap<String, PathBuf>>,
}

/// Whether the lockfile links `alias` of the project at `root_dir`, whose
/// own entry is missing, to a local directory that no root entry the linker
/// may have placed holds. The lockfile, not the manifest, says what the last
/// install linked: an override or a `readPackage` hook may drop or rewrite the
/// declared spec, and an injected workspace dependency resolves to a `file:`
/// snapshot that a platform check may skip. It is read only once no root
/// entry links the target `spec` names.
///
/// The hoisted linker may place a sibling's dependency in the root modules
/// directory, and under `dedupeDirectDeps` a shared-lockfile install links
/// the root's entry in place of a sibling's to the same target, so a root
/// entry counts when it resolves to that target.
fn local_link_missing(
    LocalLinkCheck { check, groups, project_dirs_by_name }: LocalLinkCheck<'_>,
    root_dir: &Path,
    own_modules_dir: &Path,
    (alias, spec): (&str, &str),
) -> bool {
    let is_root = lexical_normalize(root_dir) == lexical_normalize(check.workspace_root);
    let shares_root_modules_dir = !is_root
        && (check.layout.node_linker == NodeLinker::Hoisted
            || (check.config.dedupe_direct_deps && check.config.shared_workspace_lockfile));
    let root_entry = check.config.modules_dir.join(alias);
    if shares_root_modules_dir
        && declared_link_target(check, project_dirs_by_name, root_dir, (alias, spec))
            .is_some_and(|target| resolves_to(&root_entry, &target))
    {
        return false;
    }
    let Some(target) = locked_link_target(check, groups, root_dir, own_modules_dir, alias) else {
        return false;
    };
    !(shares_root_modules_dir && resolves_to(&root_entry, &target))
}

/// The directory `spec` names for `alias` of the project at `root_dir`: a
/// `link:` path, or the workspace project a `workspace:` spec selects by name.
fn declared_link_target(
    check: &OptimisticRepeatInstallCheck<'_>,
    project_dirs_by_name: &OnceLock<HashMap<String, PathBuf>>,
    root_dir: &Path,
    (alias, spec): (&str, &str),
) -> Option<PathBuf> {
    if let Some(path) = spec.strip_prefix("link:") {
        return Some(root_dir.join(path));
    }
    let range = spec.strip_prefix("workspace:")?;
    let name = match range.rfind('@') {
        Some(at) if at > 0 => &range[..at],
        _ => alias,
    };
    project_dirs_by_name
        .get_or_init(|| {
            check.project_manifests
                .iter()
                .filter_map(|(project_dir, manifest)| {
                    Some((manifest_string_field(manifest, "name")?, project_dir.clone()))
                })
                .collect()
        })
        .get(name)
        .cloned()
}

/// The directory the lockfile links `alias` of the project at `root_dir` to,
/// or `None` when it records no `link:` for it or none can be read. The
/// project's lockfile is the shared wanted lockfile, or its own under
/// `sharedWorkspaceLockfile: false`. The current lockfile stands in for a
/// missing wanted one, as it does for the rest of the fast path.
fn locked_link_target(
    check: &OptimisticRepeatInstallCheck<'_>,
    groups: &[DependencyGroup],
    root_dir: &Path,
    own_modules_dir: &Path,
    alias: &str,
) -> Option<PathBuf> {
    let alias = PkgName::parse(alias).ok()?;
    let target_in = |lockfile: &Lockfile, importer_id: &str| {
        lockfile.importers
            .get(importer_id)?
            .dependencies_by_groups(groups.iter().copied())
            .find(|(name, _)| **name == alias)
            .and_then(|(_, dep)| dep.version.as_link_target())
            .map(|target| root_dir.join(target))
    };
    let is_root = lexical_normalize(root_dir) == lexical_normalize(check.workspace_root);
    if check.config.shared_workspace_lockfile || is_root {
        let importer_id = importer_id_from_root_dir(check.workspace_root, root_dir);
        if let Ok(Some(lockfile)) = check.lockfile.get() {
            return target_in(lockfile, &importer_id);
        }
        let current =
            Lockfile::load_current_from_install_state_dir(&check.config.install_state_dir).ok()??;
        return target_in(&current, &importer_id);
    }
    let lockfile = match Lockfile::load_wanted_from_dir(root_dir) {
        Ok(Some(lockfile)) => lockfile,
        _ => Lockfile::load_current_from_install_state_dir(&own_modules_dir.join(".pnpm")).ok()??,
    };
    target_in(&lockfile, ".")
}

/// Whether `entry` exists and resolves to the directory `target` names.
fn resolves_to(entry: &Path, target: &Path) -> bool {
    match (fs::canonicalize(entry), fs::canonicalize(target)) {
        (Ok(entry), Ok(target)) => entry == target,
        _ => false,
    }
}

/// A spec the linker may link to a local directory: a path (`link:`) or a
/// workspace project (`workspace:`).
pub(in crate::optimistic_repeat_install) fn is_local_link_spec(spec: &str) -> bool {
    spec.starts_with("workspace:") || spec.starts_with("link:")
}

/// What a direct dependency's entry in a modules directory is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryState {
    Present,
    /// A link whose target is gone.
    Dangling,
    Missing,
}

/// The state of `path`, at one `stat` for a healthy entry.
fn entry_state(path: &Path) -> EntryState {
    if fs::metadata(path).is_ok() {
        EntryState::Present
    } else if fs::symlink_metadata(path).is_ok() {
        EntryState::Dangling
    } else {
        EntryState::Missing
    }
}
