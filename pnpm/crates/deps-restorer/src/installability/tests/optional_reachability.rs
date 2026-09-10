use super::{host, root_importer, snapshot_dep_map, snapshot_key, synthetic_metadata};
use crate::installability::{SkippedSnapshots, compute_skipped_snapshots};
use pnpm_lockfile::SnapshotEntry;
use pnpm_reporter::LogEvent;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

/// The graph of TS `do not fail on unsupported dependency of optional
/// dependency` (`optionalDependencies.ts:540`): the incompatible
/// package's only non-optional inbound edge comes from a skipped
/// parent, so it is skipped even under `engine_strict`.
#[test]
fn incompatible_regular_dep_of_skipped_optional_is_skipped_not_failed() {
    recording_reporter!(reset_events, take_events);
    let importers = root_importer(&[], &["not-compatible-with-not-compatible-dep@1.0.0"]);
    let parent = snapshot_key("not-compatible-with-not-compatible-dep@1.0.0");
    let incompatible = snapshot_key("not-compatible-with-any-os@1.0.0");
    let grandchild = snapshot_key("dep-of-optional-pkg@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(
        parent.clone(),
        SnapshotEntry {
            optional: true,
            dependencies: snapshot_dep_map(&["not-compatible-with-any-os@1.0.0"]),
            ..Default::default()
        },
    );
    snapshots.insert(
        incompatible.clone(),
        SnapshotEntry {
            optional: true,
            dependencies: snapshot_dep_map(&["dep-of-optional-pkg@1.0.0"]),
            ..Default::default()
        },
    );
    snapshots.insert(grandchild.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(
        parent.clone(),
        synthetic_metadata(None, None, Some(&["this-os-does-not-exist"]), None),
    );
    packages.insert(
        incompatible.clone(),
        synthetic_metadata(None, None, Some(&["this-os-does-not-exist"]), None),
    );
    packages.insert(grandchild.clone(), synthetic_metadata(None, None, None, None));

    let mut strict_host = host("20.10.0", "darwin", "arm64");
    strict_host.engine_strict = true;
    reset_events();
    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &importers,
        &snapshots,
        &packages,
        &strict_host,
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap();

    assert!(skipped.contains(&parent));
    assert!(
        skipped.contains(&incompatible),
        "an incompatible dep behind a skipped parent is skipped, not an engine-strict failure",
    );
    assert!(
        !skipped.contains(&grandchild),
        "the compatible grandchild is left to the dependency-closure extension",
    );
    let events = take_events();
    let skipped_events_count = events
        .iter()
        .filter(|event| matches!(event, LogEvent::SkippedOptionalDependency(_)))
        .count();
    assert_eq!(skipped_events_count, 2);
}
/// A package that is both an optional direct dep and a regular dep of
/// an installed package dispatches as required: the non-optional edge
/// wins over the (deliberately stale) snapshot-level `optional` flag.
#[test]
fn regular_edge_from_installed_parent_wins_over_optional_reachability() {
    recording_reporter!(reset_events);
    let importers = root_importer(&["compat-parent@1.0.0"], &["not-compatible-with-any-os@1.0.0"]);
    let compat_parent = snapshot_key("compat-parent@1.0.0");
    let incompatible = snapshot_key("not-compatible-with-any-os@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(
        compat_parent.clone(),
        SnapshotEntry {
            dependencies: snapshot_dep_map(&["not-compatible-with-any-os@1.0.0"]),
            ..Default::default()
        },
    );
    snapshots.insert(incompatible.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(compat_parent, synthetic_metadata(None, None, None, None));
    packages.insert(
        incompatible,
        synthetic_metadata(None, None, Some(&["this-os-does-not-exist"]), None),
    );

    reset_events();
    let lenient = compute_skipped_snapshots::<RecordingReporter>(
        &importers,
        &snapshots,
        &packages,
        &host("20.10.0", "darwin", "arm64"),
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap();
    assert!(lenient.is_empty(), "the regular edge from an installed parent must win over skip");

    let mut strict_host = host("20.10.0", "darwin", "arm64");
    strict_host.engine_strict = true;
    reset_events();
    let strict = compute_skipped_snapshots::<RecordingReporter>(
        &importers,
        &snapshots,
        &packages,
        &strict_host,
        "/proj",
        SkippedSnapshots::new(),
    );
    assert!(strict.is_err(), "the same required dispatch fails under engine_strict");
}
