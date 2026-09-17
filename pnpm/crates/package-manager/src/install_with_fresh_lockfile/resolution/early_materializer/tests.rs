use super::{EarlyMaterializationFit, early_materialization_eligible};
use pnpm_config::{Config, NodeLinker};

/// The default shape this optimization is built for: an isolated,
/// unfiltered, non-hoisted install with no custom fetcher.
fn eligible_fit(config: &Config) -> EarlyMaterializationFit<'_> {
    EarlyMaterializationFit {
        config,
        node_linker: NodeLinker::Isolated,
        lockfile_only: false,
        filtered_isolated: false,
        is_hoisted: false,
        has_custom_fetcher: false,
    }
}

/// Guards the platform assumption the rest of these cases rest on: where
/// the directory-clone cache serves slots, early materialization is off
/// regardless of the other inputs.
#[test]
fn the_baseline_fit_tracks_the_directory_clone_cache() {
    let config = Config::default();
    assert_eq!(
        early_materialization_eligible(eligible_fit(&config)),
        !pnpm_deps_restorer::DirCloneCache::eligible(&config, NodeLinker::Isolated),
    );
}

/// `--force` re-imports every slot in the link phase, so populating them
/// during resolution would only be staged away again.
#[test]
fn force_rules_out_early_materialization() {
    let config = Config { force: true, ..Config::default() };
    assert!(!early_materialization_eligible(eligible_fit(&config)));
}

#[test]
fn a_lockfile_only_run_rules_out_early_materialization() {
    let config = Config::default();
    let fit = EarlyMaterializationFit { lockfile_only: true, ..eligible_fit(&config) };
    assert!(!early_materialization_eligible(fit));
}
