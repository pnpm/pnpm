use super::{host, no_importers, snapshot_key, synthetic_metadata};
use crate::installability::{SkippedSnapshots, compute_skipped_snapshots};
use pnpm_lockfile::SnapshotEntry;
use pnpm_reporter::LogEvent;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

#[test]
fn duplicate_metadata_dedupes_reporter_events() {
    recording_reporter!(reset_events, take_events);
    reset_events();
    let metadata_key = snapshot_key("not-compatible-with-any-os@1.0.0");
    let snapshot_key_a = snapshot_key("not-compatible-with-any-os@1.0.0(react@17.0.2)");
    let snapshot_key_b = snapshot_key("not-compatible-with-any-os@1.0.0(react@18.0.0)");

    let mut snapshots = HashMap::new();
    snapshots
        .insert(snapshot_key_a.clone(), SnapshotEntry { optional: true, ..Default::default() });
    snapshots
        .insert(snapshot_key_b.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(
        metadata_key,
        synthetic_metadata(None, None, Some(&["this-os-does-not-exist"]), None),
    );

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &no_importers(),
        &snapshots,
        &packages,
        &host("20.10.0", "darwin", "arm64"),
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap();

    assert!(skipped.contains(&snapshot_key_a));
    assert!(skipped.contains(&snapshot_key_b));

    let events = take_events();
    let skipped_events_count = events
        .iter()
        .filter(|event| matches!(event, LogEvent::SkippedOptionalDependency(_)))
        .count();
    assert_eq!(skipped_events_count, 1, "must dedup per metadata row");
}
