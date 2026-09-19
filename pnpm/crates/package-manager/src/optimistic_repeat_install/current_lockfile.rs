use super::{Config, DependencyGroup, Lockfile};
use pnpm_modules_yaml::IncludedDependencies;

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
        materialized_shape_matches(wanted, current, included)
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
    let current = Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir)
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
                Err("the installed dependencies are not up to date with the lockfile")
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
) -> bool {
    if wanted == current {
        return true;
    }
    // A transient skip (a failed optional fetch) prunes the current lockfile
    // further, and its set is not known here. Such a tree simply falls
    // through to materialization, which retries the fetch anyway.
    current
        == &crate::filter_lockfile_for_current(wanted, included, &crate::SkippedSnapshots::new())
}
