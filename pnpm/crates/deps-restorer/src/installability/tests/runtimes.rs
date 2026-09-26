use super::{host, no_importers, root_importer, snapshot_key, synthetic_metadata};
use crate::installability::{
    InstallabilityHost, SkippedSnapshots, any_installability_constraint, compute_skipped_snapshots,
    find_root_runtime_node_key,
};
use pnpm_lockfile::SnapshotEntry;
use pnpm_reporter::{LogEvent, SkippedOptionalReason};
use pretty_assertions::assert_eq;
use std::collections::HashMap;

#[test]
fn skip_optional_with_wrong_node_engine() {
    recording_reporter!(reset_events, take_events);
    reset_events();
    let key = snapshot_key("for-legacy-node@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key.clone(), synthetic_metadata(Some(&[("node", "0.10")]), None, None, None));

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &no_importers(),
        &snapshots,
        &packages,
        &host("20.10.0", "darwin", "arm64"),
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap();

    assert!(skipped.contains(&key));
    let events = take_events();
    let skipped_events: Vec<_> = events
        .iter()
        .filter(|event| matches!(event, LogEvent::SkippedOptionalDependency(_)))
        .collect();
    assert_eq!(skipped_events.len(), 1);
    if let LogEvent::SkippedOptionalDependency(log) = skipped_events[0] {
        assert_eq!(log.reason, SkippedOptionalReason::UnsupportedEngine);
    }
}
/// `engines` block with no `node` / `pnpm` key (e.g. only `npm`)
/// does NOT trigger the slow path. Pacquet doesn't evaluate the npm
/// engine, so a package declaring `engines.npm` alone is no
/// constraint as far as installability is concerned.
#[test]
fn engines_without_node_or_pnpm_does_not_count_as_constraint() {
    let key = snapshot_key("npm-engine-only@1.0.0");
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(Some(&[("npm", ">=8")]), None, None, None));
    assert!(
        !any_installability_constraint(&HashMap::new(), &packages),
        "engines.npm alone should not block the fast path",
    );
}
#[test]
fn meaningful_engines_node_triggers_slow_path() {
    let key = snapshot_key("for-legacy-node@1.0.0");
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(Some(&[("node", "0.10")]), None, None, None));
    assert!(
        any_installability_constraint(&HashMap::new(), &packages),
        "engines.node must trigger the slow path",
    );
}
#[test]
fn detect_with_overrides_node_version_and_engine_strict() {
    let overridden = InstallabilityHost::detect_with(true, Some("18.20.4".to_string()));
    assert_eq!(overridden.node_version, "18.20.4");
    // An explicit `nodeVersion` is authoritative — treated as detected so the
    // side-effects cache keys off it.
    assert!(overridden.node_detected);
    assert!(overridden.engine_strict);

    // A `v`-prefixed / whitespace-padded value (as in `process.version`) is
    // canonicalized so it parses as exact semver.
    assert_eq!(
        InstallabilityHost::detect_with(false, Some(" v22.11.0\n".to_string())).node_version,
        "22.11.0",
    );

    // Without a version override, `engine_strict` still layers on detection.
    assert!(InstallabilityHost::detect_with(true, None).engine_strict);
}

/// A dependency's own `engines.runtime` pin adds a second `node@runtime:`
/// snapshot. Only the root project's pin is returned, whichever of the two
/// the snapshot map yields first.
#[test]
fn find_root_runtime_node_key_ignores_dependency_runtime_pins() {
    let snapshots: HashMap<_, _> = ["node@runtime:22.0.0", "node@runtime:24.0.0", "dep@1.0.0"]
        .into_iter()
        .map(|key| (snapshot_key(key), SnapshotEntry::default()))
        .collect();

    for pinned in ["node@runtime:22.0.0", "node@runtime:24.0.0"] {
        let importers = root_importer(&[pinned, "dep@1.0.0"], &[]);
        assert_eq!(find_root_runtime_node_key(&importers, &snapshots), Some(&snapshot_key(pinned)));
    }
    assert_eq!(find_root_runtime_node_key(&root_importer(&["dep@1.0.0"], &[]), &snapshots), None);
}
