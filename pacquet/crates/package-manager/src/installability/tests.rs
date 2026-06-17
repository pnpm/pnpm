//! Unit tests for [`crate::installability::compute_skipped_snapshots`].

use crate::installability::{
    InstallabilityHost, SkippedSnapshots, any_installability_constraint, compute_skipped_snapshots,
};
use pacquet_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgNameVerPeer, SnapshotEntry,
    TarballResolution,
};
use pacquet_reporter::{LogEvent, Reporter, SkippedOptionalPackage, SkippedOptionalReason};
use pretty_assertions::assert_eq;
use std::{cell::RefCell, collections::HashMap};

// Thread-local recording so the cargo-default parallel test runner
// can fan out without tests polluting each other's event stream.
// `Reporter::emit` is a free function; the captured buffer has to
// live in static storage somewhere — thread-local trades a small
// allocation per test thread for zero cross-test contention.
thread_local! {
    static RECORDED_EVENTS: RefCell<Vec<LogEvent>> = const { RefCell::new(Vec::new()) };
}

struct RecordingReporter;

impl Reporter for RecordingReporter {
    fn emit(event: &LogEvent) {
        RECORDED_EVENTS.with(|cell| cell.borrow_mut().push(event.clone()));
    }
}

fn take_events() -> Vec<LogEvent> {
    RECORDED_EVENTS.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

fn reset_events() {
    RECORDED_EVENTS.with(|cell| cell.borrow_mut().clear());
}

fn snapshot_key(name_at_version: &str) -> PackageKey {
    name_at_version.parse::<PkgNameVerPeer>().expect("valid package key")
}

fn synthetic_metadata(
    engines: Option<&[(&str, &str)]>,
    cpu: Option<&[&str]>,
    os: Option<&[&str]>,
    libc: Option<&[&str]>,
) -> PackageMetadata {
    // Tarball resolution — the installability check ignores the
    // resolution shape entirely, but every `PackageMetadata` must
    // carry one.
    PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            integrity: None,
            tarball: "https://example.test/pkg.tgz".to_string(),
            git_hosted: None,
            path: None,
        }),
        version: None,
        engines: engines.map(|entries| {
            entries.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
        }),
        cpu: cpu.map(|values| values.iter().map(|s| (*s).to_string()).collect()),
        os: os.map(|values| values.iter().map(|s| (*s).to_string()).collect()),
        libc: libc.map(|values| values.iter().map(|s| (*s).to_string()).collect()),
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn host(node_version: &str, os: &'static str, cpu: &'static str) -> InstallabilityHost {
    InstallabilityHost {
        node_version: node_version.to_string(),
        node_detected: true,
        os,
        cpu,
        libc: "unknown",
        supported_architectures: None,
        engine_strict: false,
    }
}

#[test]
fn skip_optional_with_wrong_os() {
    reset_events();
    let key = snapshot_key("not-compatible-with-any-os@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(
        key.clone(),
        synthetic_metadata(None, None, Some(&["this-os-does-not-exist"]), None),
    );

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &snapshots,
        &packages,
        &host("20.10.0", "darwin", "arm64"),
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap();

    assert_eq!(skipped.len(), 1);
    assert!(skipped.contains(&key));

    let events = take_events();
    let skipped_events: Vec<_> = events
        .iter()
        .filter(|event| matches!(event, LogEvent::SkippedOptionalDependency(_)))
        .collect();
    assert_eq!(skipped_events.len(), 1);
    if let LogEvent::SkippedOptionalDependency(log) = skipped_events[0] {
        assert_eq!(log.reason, SkippedOptionalReason::UnsupportedPlatform);
        let SkippedOptionalPackage::Installed { name, version, .. } = &log.package else {
            panic!("expected Installed payload for unsupported_platform, got {:?}", log.package);
        };
        assert_eq!(name, "not-compatible-with-any-os");
        assert_eq!(version, "1.0.0");
        assert_eq!(log.prefix, "/proj");
    }
}

#[test]
fn skip_optional_with_wrong_node_engine() {
    reset_events();
    let key = snapshot_key("for-legacy-node@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key.clone(), synthetic_metadata(Some(&[("node", "0.10")]), None, None, None));

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
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

#[test]
fn compatible_snapshots_are_not_skipped() {
    reset_events();
    let key = snapshot_key("compat@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, Some(&["darwin", "linux"]), None));

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &snapshots,
        &packages,
        &host("20.10.0", "darwin", "arm64"),
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap();

    assert!(skipped.is_empty());
    let events = take_events();
    assert!(
        events.iter().all(|event| !matches!(event, LogEvent::SkippedOptionalDependency(_))),
        "expected no skipped-optional events, got {events:?}",
    );
}

#[test]
fn non_optional_incompatible_is_not_skipped() {
    reset_events();
    let key = snapshot_key("non-optional-but-wrong-os@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: false, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, Some(&["this-os-does-not-exist"]), None));

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &snapshots,
        &packages,
        &host("20.10.0", "darwin", "arm64"),
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap();

    assert!(skipped.is_empty());
    let events = take_events();
    assert!(
        events.iter().all(|event| !matches!(event, LogEvent::SkippedOptionalDependency(_))),
        "non-optional must not fire skipped-optional events",
    );
}

#[test]
fn no_constraints_skips_the_per_snapshot_pass() {
    reset_events();
    let key = snapshot_key("no-constraints@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, None, None));

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &snapshots,
        &packages,
        &host("20.10.0", "darwin", "arm64"),
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap();

    assert!(skipped.is_empty());
    let events = take_events();
    assert!(
        events.iter().all(|event| !matches!(event, LogEvent::SkippedOptionalDependency(_))),
        "fast path must not fire skipped-optional events",
    );
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
fn platform_any_sentinel_does_not_count_as_constraint() {
    let key = snapshot_key("any-platforms@1.0.0");
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, Some(&["any"]), Some(&["any"]), Some(&["any"])));
    assert!(
        !any_installability_constraint(&HashMap::new(), &packages),
        r#"cpu/os/libc = ["any"] should not block the fast path"#,
    );
}

#[test]
fn empty_platform_lists_do_not_count_as_constraint() {
    let key = snapshot_key("empty-platforms@1.0.0");
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, Some(&[]), Some(&[]), Some(&[])));
    assert!(
        !any_installability_constraint(&HashMap::new(), &packages),
        "empty platform lists should not block the fast path",
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
fn meaningful_platform_value_triggers_slow_path() {
    let key = snapshot_key("not-compatible-with-any-os@1.0.0");
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, Some(&["this-os-does-not-exist"]), None));
    assert!(
        any_installability_constraint(&HashMap::new(), &packages),
        "non-any os must trigger the slow path",
    );
}

#[test]
fn duplicate_metadata_dedupes_reporter_events() {
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

#[test]
fn supported_architectures_widens_accept_set_so_optional_stays() {
    reset_events();
    let key = snapshot_key("darwin-only-pkg@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, Some(&["darwin"]), None));

    let mut host = host("20.10.0", "linux", "x64");
    host.supported_architectures = Some(pacquet_package_is_installable::SupportedArchitectures {
        os: Some(vec!["darwin".to_string()]),
        cpu: None,
        libc: None,
    });

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &snapshots,
        &packages,
        &host,
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap();

    assert!(skipped.is_empty(), "supportedArchitectures.os=['darwin'] should keep the package");
    let events = take_events();
    assert!(
        events.iter().all(|event| !matches!(event, LogEvent::SkippedOptionalDependency(_))),
        "no skipped-optional event expected, got {events:?}",
    );
}

#[test]
fn supported_architectures_does_not_implicitly_include_host() {
    reset_events();
    let key = snapshot_key("darwin-only-pkg@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key.clone(), synthetic_metadata(None, None, Some(&["darwin"]), None));

    let mut host = host("20.10.0", "linux", "x64");
    host.supported_architectures = Some(pacquet_package_is_installable::SupportedArchitectures {
        os: Some(vec!["linux".to_string()]),
        cpu: None,
        libc: None,
    });

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &snapshots,
        &packages,
        &host,
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap();

    assert!(
        skipped.contains(&key),
        "supportedArchitectures.os=['linux'] should still skip a darwin-only package",
    );
}

#[test]
fn seeded_snapshot_short_circuits_recheck() {
    reset_events();
    let key = snapshot_key("for-legacy-node@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key.clone(), synthetic_metadata(Some(&[("node", "0.10")]), None, None, None));

    let seed = SkippedSnapshots::from_set(std::iter::once(key.clone()).collect());
    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &snapshots,
        &packages,
        &host("20.10.0", "darwin", "arm64"),
        "/proj",
        seed,
    )
    .unwrap();

    assert!(skipped.contains(&key), "seeded key must survive the recompute");
    let events = take_events();
    assert!(
        events.iter().all(|event| !matches!(event, LogEvent::SkippedOptionalDependency(_))),
        "no SkippedOptionalDependency event must fire for a seeded snapshot, got {events:?}",
    );
}

#[test]
fn fast_path_preserves_seed() {
    reset_events();
    let key = snapshot_key("previously-skipped@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key.clone(), synthetic_metadata(None, None, None, None));

    let seed = SkippedSnapshots::from_set(std::iter::once(key.clone()).collect());
    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &snapshots,
        &packages,
        &host("20.10.0", "darwin", "arm64"),
        "/proj",
        seed,
    )
    .unwrap();

    assert_eq!(skipped.len(), 1, "fast path must keep the seed entry");
    assert!(skipped.contains(&key));
}

#[test]
fn from_strings_skips_unparsable_entries() {
    let set = SkippedSnapshots::from_strings([
        "valid-pkg@1.0.0",
        "@scope/pkg@2.0.0",
        "",
        "not a depPath",
        "@@@no-version",
    ]);
    let key1: PackageKey = "valid-pkg@1.0.0".parse().unwrap();
    let key2: PackageKey = "@scope/pkg@2.0.0".parse().unwrap();
    assert_eq!(set.len(), 2);
    assert!(set.contains(&key1));
    assert!(set.contains(&key2));
}

#[test]
fn disjoint_subsets_preserve_len_and_iter() {
    let key: PackageKey = "platform-mismatch-optional@1.0.0".parse().unwrap();
    let mut skipped = SkippedSnapshots::new();
    skipped.insert_installability(key.clone());
    skipped.add_optional_excluded(key.clone());
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped.iter().count(), 1);
    assert!(skipped.contains(&key));

    skipped.add_fetch_failed(key);
    assert_eq!(skipped.len(), 1);
}

#[test]
fn fetch_failed_and_optional_excluded_are_symmetric() {
    let key: PackageKey = "weird-overlap@1.0.0".parse().unwrap();
    let mut skipped_a = SkippedSnapshots::new();
    skipped_a.add_fetch_failed(key.clone());
    skipped_a.add_optional_excluded(key.clone());
    assert_eq!(skipped_a.len(), 1);
    assert_eq!(skipped_a.iter().count(), 1);
    assert!(skipped_a.contains(&key));

    let mut skipped_b = SkippedSnapshots::new();
    skipped_b.add_optional_excluded(key.clone());
    skipped_b.add_fetch_failed(key.clone());
    assert_eq!(skipped_b.len(), 1);
    assert_eq!(skipped_b.iter().count(), 1);
    assert!(skipped_b.contains(&key));
}

#[test]
fn skip_optional_with_platform_inferred_from_name() {
    reset_events();
    let foreign = snapshot_key("@nx/nx-win32-arm64-msvc@1.0.0");
    let matching = snapshot_key("@nx/nx-linux-x64-gnu@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(foreign.clone(), SnapshotEntry { optional: true, ..Default::default() });
    snapshots.insert(matching.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(foreign.clone(), synthetic_metadata(None, None, None, None));
    packages.insert(matching.clone(), synthetic_metadata(None, None, None, None));

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &snapshots,
        &packages,
        &host("20.10.0", "linux", "x64"),
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap();

    assert_eq!(skipped.len(), 1);
    assert!(skipped.contains(&foreign));
    assert!(!skipped.contains(&matching));

    let events = take_events();
    let skipped_events_count = events
        .iter()
        .filter(|event| matches!(event, LogEvent::SkippedOptionalDependency(_)))
        .count();
    assert_eq!(skipped_events_count, 1);
}

#[test]
fn name_inference_does_not_apply_to_non_optional_snapshots() {
    reset_events();
    let key = snapshot_key("@nx/nx-win32-arm64-msvc@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: false, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, None, None));

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &snapshots,
        &packages,
        &host("20.10.0", "linux", "x64"),
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap();

    assert!(skipped.is_empty());
}

#[test]
fn missing_libc_is_inferred_from_name() {
    reset_events();
    let musl = snapshot_key("@nx/nx-linux-x64-musl@1.0.0");
    let gnu = snapshot_key("@nx/nx-linux-x64-gnu@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(musl.clone(), SnapshotEntry { optional: true, ..Default::default() });
    snapshots.insert(gnu.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(musl.clone(), synthetic_metadata(None, Some(&["x64"]), Some(&["linux"]), None));
    packages.insert(gnu.clone(), synthetic_metadata(None, Some(&["x64"]), Some(&["linux"]), None));

    let mut host = host("20.10.0", "linux", "x64");
    host.libc = "glibc";
    host.supported_architectures = Some(pacquet_package_is_installable::SupportedArchitectures {
        os: Some(vec!["linux".to_string()]),
        cpu: Some(vec!["x64".to_string()]),
        libc: Some(vec!["glibc".to_string()]),
    });

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &snapshots,
        &packages,
        &host,
        "/proj",
        SkippedSnapshots::new(),
    )
    .unwrap();

    assert_eq!(skipped.len(), 1);
    assert!(skipped.contains(&musl));
    assert!(!skipped.contains(&gnu));
}

#[test]
fn name_inferable_optional_snapshot_triggers_slow_path() {
    let key = snapshot_key("@nx/nx-win32-arm64-msvc@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, None, None));
    assert!(
        any_installability_constraint(&snapshots, &packages),
        "a platform-named optional snapshot must trigger the slow path",
    );
}

#[test]
fn generic_name_does_not_trigger_slow_path() {
    let key = snapshot_key("is-arm@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, None, None));
    assert!(
        !any_installability_constraint(&snapshots, &packages),
        "a generic name segment must not block the fast path",
    );
}
