use super::{
    Config, DependencyGroup, Lockfile, OptimisticRepeatInstallCheck, Path, WorkspaceState,
};
use pnpm_modules_yaml::IncludedDependencies;
use std::collections::HashSet;

/// pnpm's issue wording for a current lockfile that does not record what the
/// wanted lockfile materializes (`RUN_CHECK_DEPS_OUTDATED_DEPS`).
pub(super) const OUTDATED_DEPS: &str =
    "the installed dependencies are not up to date with the lockfile";

/// Assert the wanted lockfile equals the current one: with no current
/// lockfile every importer of the wanted one must be dependency-free
/// (`RUN_CHECK_DEPS_NO_DEPS`); otherwise the current lockfile must record
/// what materializing the wanted one produces, per
/// [`materialized_shape_matches`] (`RUN_CHECK_DEPS_OUTDATED_DEPS`).
pub(crate) fn assert_wanted_lockfile_equals_current(
    wanted: &Lockfile,
    config: &Config,
    included: IncludedDependencies,
) -> Result<(), &'static str> {
    assert_current_lockfile_records(wanted, config, |current| {
        materialized_shape_matches(wanted, current, included, config.peer_edge_options())
    })
}

/// [`assert_wanted_lockfile_equals_current`] for a current lockfile a
/// filtered install wrote. That install materialized only the importers it
/// selected, so the current lockfile must record the closure of the wanted
/// lockfile rooted at the importers it lists.
pub(crate) fn assert_wanted_lockfile_equals_filtered_current(
    wanted: &Lockfile,
    config: &Config,
    included: IncludedDependencies,
    workspace_root: &Path,
) -> Result<(), &'static str> {
    assert_current_lockfile_records(wanted, config, |current| {
        let peer_edges = config.peer_edge_options();
        if materialized_shape_matches(wanted, current, included, peer_edges) {
            return true;
        }
        let importer_ids = current.importers
            .keys()
            .cloned()
            .collect();
        current
            == &crate::materialization_closure(
                wanted,
                workspace_root,
                &importer_ids,
                &crate::GroupSelection::classify(wanted, included, peer_edges),
                &crate::SkippedSnapshots::new(),
            )
            .lockfile
    })
}

/// [`assert_wanted_lockfile_equals_current`] with `records` deciding
/// whether the parsed current lockfile is up to date with the wanted one,
/// for a caller that accepts a current lockfile of another shape.
pub(crate) fn assert_current_lockfile_records(
    wanted: &Lockfile,
    config: &Config,
    records: impl FnOnce(&Lockfile) -> bool,
) -> Result<(), &'static str> {
    let current = Lockfile::load_current_from_install_state_dir(&config.install_state_dir)
        .map_err(|_| "the current lockfile cannot be loaded")?;
    assert_loaded_current_lockfile_records(wanted, current.as_ref(), records)
}

/// [`assert_current_lockfile_records`] for a caller that already holds the
/// parsed current lockfile.
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
    // further, and its set is not known here. Such a tree simply falls
    // through to materialization, which retries the fetch anyway.
    current
        == &crate::filter_lockfile_for_current(
            wanted,
            &crate::GroupSelection::classify(wanted, included, peer_edges),
            &crate::SkippedSnapshots::new(),
        )
}

/// Whether `current` records what the wanted lockfile materializes for
/// `importer_ids` in the dependency groups `state` recorded. A missing wanted
/// lockfile leaves nothing to compare: the current one stands in for it.
pub(super) fn selection_matches_wanted_lockfile(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    current: &Lockfile,
    importer_ids: &HashSet<String>,
) -> bool {
    let wanted = match check.lockfile.get() {
        Ok(Some(wanted)) => wanted,
        Ok(None) => return true,
        Err(_) => return false,
    };
    let included = IncludedDependencies {
        dependencies: state.settings.production.unwrap_or(true),
        dev_dependencies: state.settings.dev.unwrap_or(true),
        optional_dependencies: state.settings.optional.unwrap_or(true),
    };
    let closure = |lockfile: &Lockfile| {
        crate::materialization_closure(
            lockfile,
            check.workspace_root,
            importer_ids,
            &crate::GroupSelection::classify(lockfile, included, check.config.peer_edge_options()),
            &crate::SkippedSnapshots::new(),
        )
        .lockfile
    };
    let (wanted, current) = (closure(wanted), closure(current));
    wanted.importers == current.importers
        && wanted.snapshots == current.snapshots
        && wanted.packages == current.packages
}
