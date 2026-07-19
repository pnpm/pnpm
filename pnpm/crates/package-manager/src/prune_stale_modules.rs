//! Reconcile an existing `node_modules` with the wanted lockfile before
//! linking: the port of the removal half of pnpm's `modules-cleaner`
//! `prune`. Removes direct-dependency links (and their bin shims) the
//! wanted lockfile no longer records, unlinks hoisted aliases whose
//! owner snapshot disappeared, and emits the `pnpm:root` `removed` and
//! `pnpm:stats` `removed` events pnpm emits from the same spot. Runs
//! before the link passes so a follow-up hoist can claim the vacated
//! slots; over-removal of a still-wanted entry is self-healing because
//! everything wanted is re-linked right after.
//!
//! Orphan *virtual-store directories* are deliberately not removed
//! here: they are the modules cache, swept by the throttled
//! [`crate::prune_virtual_store`] pass (`modulesCacheMaxAge`), exactly
//! like upstream's `pruneVirtualStore` gate.

use crate::{
    hoist::HoistedDependencies,
    prune_direct_deps::{PruneDirectDepsError, confined_modules_dir, remove_direct_dep_link},
    symlink_direct_dependencies::{importer_root_dir, validate_importer_id},
};
use pacquet_config::Config;
use pacquet_lockfile::{
    ImporterDepVersion, Lockfile, PkgName, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec,
};
use pacquet_modules_yaml::HoistKind;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::{
    DependencyType, LogEvent, LogLevel, RemovedRoot, Reporter, RootLog, RootMessage,
};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    path::Path,
};

/// Remove what a previous install materialized that the wanted lockfile
/// no longer records. See the module docs for scope; construction
/// mirrors the other `prune_*` passes.
#[derive(Debug)]
pub struct PruneStaleModules<'a> {
    pub config: &'a Config,
    pub workspace_root: &'a Path,
    /// The lockfile about to be materialized (already narrowed to the
    /// selected importers on a filtered install).
    pub wanted_lockfile: &'a Lockfile,
    /// What the previous install materialized
    /// (`<virtual_store_dir>/lock.yaml`).
    pub current_lockfile: &'a Lockfile,
    /// `hoistedDependencies` from the previous `.modules.yaml`; the
    /// only record of where hoist links were written, so orphan hoist
    /// cleanup is skipped without it.
    pub prior_hoisted_dependencies: Option<&'a HoistedDependencies>,
    /// Dependency groups this install materializes; a direct dep whose
    /// group is excluded is handled by
    /// [`crate::prune_direct_deps_excluded_by_groups`], not here.
    pub included_groups: &'a [DependencyGroup],
    /// `false` on a filtered install: the wanted lockfile then only
    /// covers the selected importers, so a global snapshot diff would
    /// misread every unselected importer's packages as orphans.
    pub prune_orphans: bool,
}

/// Every group an importer snapshot records; the current lockfile was
/// written filtered by the groups of the install that produced it, so
/// its whole recorded set is subject to the diff.
const RECORDED_GROUPS: [DependencyGroup; 3] =
    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];

impl PruneStaleModules<'_> {
    /// Returns the count of unique orphan *packages* (dep-paths
    /// deduped down to `name@version`, matching pnpm's
    /// `orphanPkgIds`), for the caller's single `pnpm:stats`
    /// `removed` emission. `0` when the orphan diff is skipped.
    pub fn run<Reporter: self::Reporter>(self) -> Result<u64, PruneDirectDepsError> {
        let PruneStaleModules {
            config,
            workspace_root,
            wanted_lockfile,
            current_lockfile,
            prior_hoisted_dependencies,
            included_groups,
            prune_orphans,
        } = self;

        let modules_dir_name: &OsStr =
            config.modules_dir.file_name().unwrap_or_else(|| OsStr::new("node_modules"));

        for (importer_id, current_snapshot) in &current_lockfile.importers {
            // An importer the wanted lockfile doesn't cover was not
            // re-materialized; leave its links alone (the partial
            // install guard).
            let Some(wanted_snapshot) = wanted_lockfile.importers.get(importer_id) else {
                continue;
            };
            if validate_importer_id(importer_id).is_err() {
                continue;
            }
            let importer_dir = importer_root_dir(workspace_root, importer_id);
            let modules_dir = importer_dir.join(modules_dir_name);
            let Some(modules_dir) = confined_modules_dir(&modules_dir, workspace_root) else {
                continue;
            };
            let wanted_specs = direct_deps_of(wanted_snapshot, included_groups);
            let prefix = importer_dir.display().to_string();
            for (alias, current_spec, group) in direct_deps_of(current_snapshot, &RECORDED_GROUPS) {
                if wanted_specs
                    .iter()
                    .any(|(name, spec, _)| *name == alias && spec.version == current_spec.version)
                {
                    continue;
                }
                remove_direct_dep_link(&modules_dir, &alias.to_string())?;
                Reporter::emit(&LogEvent::Root(RootLog {
                    level: LogLevel::Debug,
                    message: RootMessage::Removed {
                        prefix: prefix.clone(),
                        removed: RemovedRoot {
                            name: alias.to_string(),
                            version: removed_version(current_spec),
                            dependency_type: Some(dependency_type(group)),
                        },
                    },
                }));
            }
        }

        if prune_orphans {
            prune_orphan_snapshots(
                config,
                workspace_root,
                wanted_lockfile,
                current_lockfile,
                prior_hoisted_dependencies,
            )
        } else {
            Ok(0)
        }
    }
}

/// Diff the snapshot key sets, unlink the hoisted aliases the orphans
/// owned, and return the unique orphan-package count.
fn prune_orphan_snapshots(
    config: &Config,
    workspace_root: &Path,
    wanted_lockfile: &Lockfile,
    current_lockfile: &Lockfile,
    prior_hoisted_dependencies: Option<&HoistedDependencies>,
) -> Result<u64, PruneDirectDepsError> {
    let empty = HashMap::new();
    let current_snapshots = current_lockfile.snapshots.as_ref().unwrap_or(&empty);
    let wanted_snapshots = wanted_lockfile.snapshots.as_ref().unwrap_or(&empty);
    let orphan_keys: Vec<_> =
        current_snapshots.keys().filter(|key| !wanted_snapshots.contains_key(*key)).collect();

    let orphan_pkg_ids: HashSet<String> =
        orphan_keys.iter().map(|key| format!("{}@{}", key.name, key.suffix.version())).collect();
    let removed = orphan_pkg_ids.len() as u64;

    let Some(prior_hoisted) = prior_hoisted_dependencies else {
        return Ok(removed);
    };
    // Hoist links live in exactly two dirs; resolve + confine each once.
    let private_dir = config.virtual_store_dir.join("node_modules");
    let private_dir = confined_modules_dir(&private_dir, workspace_root);
    let public_dir = confined_modules_dir(&config.modules_dir, workspace_root);
    for key in orphan_keys {
        let Some(aliases) = prior_hoisted.get(&key.to_string()) else {
            continue;
        };
        for (alias, kind) in aliases {
            let target_dir = match kind {
                HoistKind::Private => private_dir.as_deref(),
                HoistKind::Public => public_dir.as_deref(),
            };
            // No `pnpm:root` event for hoist unlinks — pnpm removes
            // these with `muteLogs: true`.
            if let Some(target_dir) = target_dir {
                remove_direct_dep_link(target_dir, alias)?;
            }
        }
    }
    Ok(removed)
}

/// The `(alias, spec, group)` view of an importer snapshot, in the
/// prod → dev → optional order pnpm merges the maps in.
fn direct_deps_of<'a>(
    snapshot: &'a ProjectSnapshot,
    groups: &[DependencyGroup],
) -> Vec<(&'a PkgName, &'a ResolvedDependencySpec, DependencyGroup)> {
    let mut deps = Vec::new();
    let mut push = |map: &'a Option<ResolvedDependencyMap>, group: DependencyGroup| {
        if let Some(map) = map {
            deps.extend(map.iter().map(move |(alias, spec)| (alias, spec, group)));
        }
    };
    for group in groups {
        match group {
            DependencyGroup::Prod => push(&snapshot.dependencies, DependencyGroup::Prod),
            DependencyGroup::Dev => push(&snapshot.dev_dependencies, DependencyGroup::Dev),
            DependencyGroup::Optional => {
                push(&snapshot.optional_dependencies, DependencyGroup::Optional);
            }
            DependencyGroup::Peer => {}
        }
    }
    deps
}

/// Peer-free version string for the `pnpm:root` `removed` payload —
/// the same wire formatting the `added` emit uses.
fn removed_version(spec: &ResolvedDependencySpec) -> Option<String> {
    match &spec.version {
        ImporterDepVersion::Regular(ver) => Some(ver.version().to_string()),
        ImporterDepVersion::Alias(alias) => Some(alias.suffix.version().to_string()),
        ImporterDepVersion::Link(target) => Some(format!("link:{target}")),
        ImporterDepVersion::File(target) => Some(format!("file:{target}")),
    }
}

fn dependency_type(group: DependencyGroup) -> DependencyType {
    match group {
        DependencyGroup::Prod => DependencyType::Prod,
        DependencyGroup::Dev => DependencyType::Dev,
        DependencyGroup::Optional => DependencyType::Optional,
        DependencyGroup::Peer => unreachable!("peers are not an importer dependency map"),
    }
}
