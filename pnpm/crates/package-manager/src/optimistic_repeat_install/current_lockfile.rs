use super::{DependencyGroup, Lockfile, OptimisticRepeatInstallCheck, Path, WorkspaceState};
use pnpm_modules_yaml::IncludedDependencies;
use std::{cell::OnceCell, collections::HashSet};

/// The current lockfile of one check, loaded the first time a phase needs it.
pub(crate) struct CurrentLockfile<'a> {
    install_state_dir: &'a Path,
    loaded: OnceCell<Option<Option<Lockfile>>>,
}

impl<'a> CurrentLockfile<'a> {
    pub(crate) fn new(install_state_dir: &'a Path) -> Self {
        Self { install_state_dir, loaded: OnceCell::new() }
    }

    /// `None` when the file exists but cannot be loaded.
    pub(crate) fn get(&self) -> Option<Option<&Lockfile>> {
        self.loaded
            .get_or_init(|| {
                Lockfile::load_current_from_install_state_dir(self.install_state_dir).ok()
            })
            .as_ref()
            .map(Option::as_ref)
    }
}

/// pnpm's issue wording for a current lockfile that does not record what the
/// wanted lockfile materializes (`RUN_CHECK_DEPS_OUTDATED_DEPS`).
pub(super) const OUTDATED_DEPS: &str =
    "the installed dependencies are not up to date with the lockfile";

/// Assert that the parsed current lockfile records what `wanted` materializes:
/// with no current lockfile every importer of `wanted` must be dependency-free
/// (`RUN_CHECK_DEPS_NO_DEPS`); otherwise `records` decides
/// (`RUN_CHECK_DEPS_OUTDATED_DEPS`).
pub(crate) fn assert_loaded_current_lockfile_records(
    wanted: &Lockfile,
    current: Option<&Lockfile>,
    records: impl FnOnce(&Lockfile) -> bool,
) -> Result<(), &'static str> {
    match current {
        None => {
            let any_deps = wanted.importers
                .values()
                .any(|snapshot| {
                    snapshot
                        .dependencies_by_groups([
                            DependencyGroup::Prod,
                            DependencyGroup::Dev,
                            DependencyGroup::Optional,
                        ])
                        .next()
                        .is_some()
                });
            if any_deps {
                Err("the lockfile requires dependencies but none were installed")
            } else {
                Ok(())
            }
        }
        Some(current) => {
            if records(current) {
                Ok(())
            } else {
                Err(OUTDATED_DEPS)
            }
        }
    }
}

/// Whether `current` already records what materializing `wanted` would
/// produce.
///
/// The current lockfile keeps only what the importers reach
/// ([`crate::filter_lockfile_for_current`]) and none of the top-level keys
/// pnpm does not define, so a wanted lockfile carrying a snapshot no
/// importer reaches any more, or an embedder's extension block, can never
/// equal it. Comparing the same shape both sides is what lets such a tree
/// settle instead of re-materializing on every run.
///
/// The equal case is the common one and answers without building the
/// filtered shape at all.
pub(crate) fn materialized_shape_matches(
    wanted: &Lockfile,
    current: &Lockfile,
    included: IncludedDependencies,
    peer_edges: pnpm_lockfile::PeerEdgeOptions,
) -> bool {
    if wanted == current {
        return true;
    }
    // A transient skip (a failed optional fetch) prunes the current lockfile
    // further, and its set is not known here. Such a tree falls
    // through to materialization, which retries the fetch anyway.
    current
        == &crate::filter_lockfile_for_current(
            wanted,
            &crate::GroupSelection::classify(wanted, included, peer_edges),
            &crate::SkippedSnapshots::new(),
        )
}

/// Whether `current` records exactly what `wanted` materializes for
/// `importer_ids` in the dependency groups `state` recorded. Every group
/// `current` records counts, so a tree that still holds a group the install
/// now leaves out does not match.
pub(super) fn selection_matches_wanted_lockfile(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    wanted: &Lockfile,
    current: &Lockfile,
    importer_ids: &HashSet<String>,
) -> bool {
    let closure = |lockfile: &Lockfile, groups: &crate::GroupSelection| {
        crate::materialization_closure(
            lockfile,
            check.workspace_root,
            importer_ids,
            groups,
            &crate::SkippedSnapshots::new(),
        )
        .lockfile
    };
    let recorded_groups = crate::GroupSelection::classify(
        wanted,
        recorded_groups(state),
        check.config.peer_edge_options(),
    );
    let (wanted, current) =
        (closure(wanted, &recorded_groups), closure(current, &crate::GroupSelection::all()));
    wanted.importers == current.importers
        && wanted.snapshots == current.snapshots
        && wanted.packages == current.packages
}

/// Whether any of `importer_ids` declares a dependency in `wanted`.
pub(super) fn selection_has_dependencies(
    wanted: &Lockfile,
    importer_ids: &HashSet<String>,
) -> bool {
    importer_ids
        .iter()
        .filter_map(|id| wanted.importers.get(id))
        .any(|importer| {
            importer
                .dependencies_by_groups([
                    DependencyGroup::Prod,
                    DependencyGroup::Dev,
                    DependencyGroup::Optional,
                ])
                .next()
                .is_some()
        })
}

/// Whether `current` records `importer_id` as installed: it lists the
/// importer and a snapshot for each of its direct dependencies that is not a
/// link. A filtered install in pnpm 11 keeps every importer in the current
/// lockfile while omitting the dependencies and packages of the projects it
/// did not select.
pub(super) fn current_records_importer(
    current: &Lockfile,
    importer_id: &str,
    status: super::freshness::ProjectStatus,
) -> bool {
    let Some(importer) = current.importers.get(importer_id) else {
        return false;
    };
    let groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
    if status == super::freshness::ProjectStatus::Unrecorded
        && importer
            .dependencies_by_groups(groups)
            .next()
            .is_none()
    {
        return false;
    }
    let snapshots = current.snapshots.as_ref();
    importer
        .dependencies_by_groups(groups)
        .filter_map(|(alias, spec)| spec.version.resolved_key(alias))
        .all(|key| snapshots.is_some_and(|snapshots| snapshots.contains_key(&key)))
}

/// The dependency groups the last install recorded.
fn recorded_groups(state: &WorkspaceState) -> IncludedDependencies {
    IncludedDependencies {
        dependencies: state.settings.production.unwrap_or(true),
        dev_dependencies: state.settings.dev.unwrap_or(true),
        optional_dependencies: state.settings.optional.unwrap_or(true),
    }
}
