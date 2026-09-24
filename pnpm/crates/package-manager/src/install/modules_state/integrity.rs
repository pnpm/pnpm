use super::{Config, Lockfile, NodeLinker, Path};
use pnpm_lockfile::PackageKey;
use std::collections::BTreeMap;

/// On-disk probe backing the frozen no-op short-circuit: the
/// short-circuit skips the materialization walk entirely, so it must
/// first prove the tree it would skip is still whole, as pnpm's headless
/// path stats every package dir on every run, which is what repairs a
/// hand-deleted package. Required packages and direct-dependency links are
/// checked; any missing entry falls through to the full
/// frozen path, which re-materializes it (emitting
/// `pnpm:_broken_node_modules`).
///
/// Under a global virtual store the slot paths depend on graph hashes
/// the short-circuit doesn't compute, and the hoisted linker has no
/// virtual-store slots. Hoisted packages are probed at their recorded
/// placements, then importer links are checked for both layouts.
pub(crate) fn frozen_tree_intact(
    wanted: &Lockfile,
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
    workspace_root: &Path,
    node_linker: NodeLinker,
) -> bool {
    if matches!(node_linker, NodeLinker::Pnp) && !workspace_root.join(crate::PNP_FILENAME).is_file()
    {
        return false;
    }
    let skipped = crate::SkippedSnapshots::from_strings(&modules.skipped);
    if node_linker == NodeLinker::Hoisted
        && !hoisted_packages_present(wanted, config, workspace_root, &skipped)
    {
        return false;
    }
    let probe_slots =
        !matches!(node_linker, NodeLinker::Hoisted) && !config.enable_global_virtual_store;
    if probe_slots
        && let Some(snapshots) = wanted.snapshots.as_ref()
        && !all_virtual_store_slots_present(snapshots, config, &skipped)
    {
        return false;
    }
    if !config.symlink {
        return probe_slots;
    }
    importer_symlinks_intact(wanted, modules, config, workspace_root, node_linker, &skipped)
}

pub(crate) fn hoisted_workspace_packages_present(
    current: &Lockfile,
    config: &Config,
    workspace_root: &Path,
    included: pnpm_modules_yaml::IncludedDependencies,
    projects: &[(std::path::PathBuf, &pnpm_package_manifest::PackageManifest)],
    skipped: &crate::SkippedSnapshots,
) -> bool {
    if !config.hoist_workspace_packages {
        return true;
    }
    let candidates = pnpm_deps_restorer::workspace_packages_for_hoist(workspace_root, projects);
    if candidates.is_empty() {
        return true;
    }
    let private = pnpm_matcher::create_matcher(
        config.hoist_pattern
            .as_deref()
            .unwrap_or(&[]),
    );
    let public = pnpm_matcher::create_matcher(
        config.public_hoist_pattern
            .as_deref()
            .unwrap_or(&[]),
    );
    if private.is_empty() && public.is_empty() {
        return true;
    }
    let claimed_by_dependencies =
        aliases_claimed_by_dependencies(current, config, included, skipped);
    let private_root = config.virtual_store_dir.join("node_modules");
    candidates
        .iter()
        .all(|(name, (_, project_dir))| {
            let root = if public.matches(name) {
                config.modules_dir.as_path()
            } else if private.matches(name) {
                private_root.as_path()
            } else {
                return true;
            };
            claimed_by_dependencies.contains(&name.to_lowercase())
                || crate::safe_join_modules_dir::safe_join_workspace_modules_dir(root, name)
                    .is_ok_and(|destination| workspace_link_points_to(&destination, project_dir))
        })
}

/// The hoisted linker's `hoist-workspace-packages` links in the root
/// `node_modules`: a project the setting and patterns select has a
/// directory at its name, reached through its own link or the package that
/// holds the name, and a project they do not select is not linked.
pub(crate) fn hoisted_linker_workspace_links_intact(
    config: &Config,
    workspace_root: &Path,
    projects: &[(std::path::PathBuf, &pnpm_package_manifest::PackageManifest)],
) -> bool {
    let candidates = pnpm_deps_restorer::workspace_packages_for_hoist(workspace_root, projects);
    if candidates.is_empty() {
        return true;
    }
    let private = pnpm_matcher::create_matcher(
        config.hoist_pattern
            .as_deref()
            .unwrap_or(&[]),
    );
    let public = pnpm_matcher::create_matcher(
        config.public_hoist_pattern
            .as_deref()
            .unwrap_or(&[]),
    );
    candidates
        .iter()
        .all(|(name, (_, project_dir))| {
            let Ok(destination) = crate::safe_join_modules_dir::safe_join_workspace_modules_dir(
                &config.modules_dir,
                name,
            ) else {
                return true;
            };
            if config.hoist_workspace_packages && (public.matches(name) || private.matches(name)) {
                destination.is_dir()
            } else {
                !workspace_link_points_to(&destination, project_dir)
            }
        })
}

fn workspace_link_points_to(destination: &Path, project_dir: &Path) -> bool {
    let Ok(target) = pnpm_fs::read_symlink_dir(destination) else { return false };
    let target = if target.is_relative() {
        destination
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(target)
    } else {
        target
    };
    pnpm_fs::lexical_normalize(&target) == pnpm_fs::lexical_normalize(project_dir)
}

/// The aliases the hoist pass gives to a direct dependency, so an equally named
/// workspace project is not expected at a hoist target.
fn aliases_claimed_by_dependencies(
    current: &Lockfile,
    config: &Config,
    included: pnpm_modules_yaml::IncludedDependencies,
    skipped: &crate::SkippedSnapshots,
) -> std::collections::HashSet<String> {
    let Some((snapshots, packages)) = current.snapshots.as_ref().zip(current.packages.as_ref())
    else {
        return std::collections::HashSet::new();
    };
    let graph = pnpm_deps_restorer::build_hoist_graph_with_max_length(
        snapshots,
        packages,
        config.virtual_store_dir_max_length as usize,
    );
    let groups = pnpm_deps_restorer::selected_groups(included);
    pnpm_deps_restorer::build_direct_deps_by_importer(&current.importers, groups)
        .values()
        .flat_map(indexmap::IndexMap::iter)
        .filter(|(_, node_id)| graph.contains_key(*node_id) && !skipped.contains(node_id))
        .map(|(alias, _)| alias)
        .map(|alias| alias.to_lowercase())
        .collect()
}

/// Every required hoisted placement must survive, so a missing nested
/// version cannot silently resolve to a different version at an ancestor.
fn hoisted_packages_present(
    wanted: &Lockfile,
    config: &Config,
    workspace_root: &Path,
    skipped: &crate::SkippedSnapshots,
) -> bool {
    let Some(snapshots) = wanted.snapshots.as_ref() else { return true };
    if snapshots
        .keys()
        .all(|key| skipped.contains(key))
    {
        return true;
    }
    let Ok(Some(modules)) =
        pnpm_modules_yaml::read_modules_manifest::<super::Host>(&config.modules_dir)
    else {
        return false;
    };
    let Some(locations) = modules.hoisted_locations else { return false };
    let Some(locations) = hoisted_locations_by_identity(&locations) else { return false };
    let Ok(root) = std::fs::canonicalize(workspace_root) else { return false };
    snapshots
        .keys()
        .filter(|key| !skipped.contains(key))
        .all(|key| {
            locations
                .get(&hoisted_package_identity(key))
                .is_some_and(|dirs| {
                    !dirs.is_empty() && dirs.iter().all(|dir| hoisted_location_present(&root, dir))
                })
        })
}

fn hoisted_locations_by_identity(
    locations: &BTreeMap<String, Vec<String>>,
) -> Option<BTreeMap<String, Vec<&str>>> {
    let mut indexed: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for (reference, dirs) in locations {
        let key = reference.parse::<PackageKey>().ok()?;
        indexed
            .entry(hoisted_package_identity(&key))
            .or_default()
            .extend(dirs.iter().map(String::as_str));
    }
    Some(indexed)
}

/// Hoisting collapses registry peer variants, but injected directory variants
/// keep separate copies. A different patch must still require materialization.
fn hoisted_package_identity(key: &PackageKey) -> String {
    let reference = key.to_string();
    if pnpm_real_hoist::pkg_id(key) == reference {
        reference
    } else {
        pnpm_deps_path::get_pkg_id_with_patch_hash(&reference).to_string()
    }
}

fn hoisted_location_present(root: &Path, location: &str) -> bool {
    std::fs::canonicalize(root.join(location))
        .is_ok_and(|package_dir| {
            package_dir.starts_with(root) && package_dir != root && package_dir.is_dir()
        })
}

/// Whether every snapshot the lockfile records still has its virtual-store
/// slot on disk.
fn all_virtual_store_slots_present(
    snapshots: &std::collections::HashMap<pnpm_lockfile::PackageKey, pnpm_lockfile::SnapshotEntry>,
    config: &Config,
    skipped: &crate::SkippedSnapshots,
) -> bool {
    let layout = crate::VirtualStoreLayout::legacy(
        config.virtual_store_dir.clone(),
        config.virtual_store_dir_max_length as usize,
    );
    snapshots
        .keys()
        .all(|key| {
            if skipped.contains(key) {
                return true;
            }
            // The name is lockfile-controlled: join it with the same
            // traversal-rejecting helper the linkers use, and treat a
            // malformed name as not-intact so the full path's
            // structural lockfile gate rejects it.
            let slot_node_modules = layout.slot_dir(key).join("node_modules");
            match crate::safe_join_modules_dir::safe_join_modules_dir(
                &slot_node_modules,
                &key.name.to_string(),
            ) {
                Ok(dir) => dir.is_dir(),
                Err(_) => false,
            }
        })
}

/// Whether every importer's direct dependencies resolve from its own
/// `node_modules`, or from an ancestor inside a hoisted workspace.
fn importer_symlinks_intact(
    wanted: &Lockfile,
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
    workspace_root: &Path,
    node_linker: NodeLinker,
    skipped: &crate::SkippedSnapshots,
) -> bool {
    let groups = crate::prune_direct_deps::selected_groups(modules.included);
    let modules_dir_name: &std::ffi::OsStr = config.modules_dir_name();
    wanted.importers
        .iter()
        .all(|(importer_id, snapshot)| {
            if crate::symlink_direct_dependencies::validate_importer_id(importer_id).is_err() {
                return false;
            }
            let importer_root =
                crate::symlink_direct_dependencies::importer_root_dir(workspace_root, importer_id);
            crate::symlink_direct_dependencies::direct_dep_names_for_importer(
                snapshot,
                groups.iter().copied(),
                skipped,
                false,
            )
            .iter()
            .all(|name| {
                importer_root
                    .ancestors()
                    .take_while(|dir| {
                        dir.starts_with(workspace_root)
                            && (node_linker == NodeLinker::Hoisted || *dir == importer_root)
                    })
                    .any(|dir| direct_dep_link_resolves(&dir.join(modules_dir_name), name))
            })
        })
}

fn direct_dep_link_resolves(modules_dir: &Path, name: &str) -> bool {
    match crate::safe_join_modules_dir::safe_join_modules_dir(modules_dir, name) {
        // `metadata` follows the link, so a dangling direct-dep
        // symlink (a wiped GVS store, a hand-deleted target)
        // reads as broken and falls through to the repairing
        // full path.
        Ok(link) => std::fs::metadata(link).is_ok(),
        // A malformed alias never probes the disk; the full
        // path rejects it with its own typed error.
        Err(_) => false,
    }
}
