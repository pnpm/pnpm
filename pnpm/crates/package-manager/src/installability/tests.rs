//! Unit tests for [`crate::installability::compute_skipped_snapshots`].

use crate::installability::{
    InstallabilityHost, SkippedSnapshots, any_installability_constraint,
    any_optional_installability_constraint, compute_skipped_snapshots,
};
use pacquet_lockfile::{
    ImporterDepVersion, LockfileResolution, PackageKey, PackageMetadata, PkgName, PkgNameVerPeer,
    ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
    TarballResolution,
};
use pacquet_reporter::{LogEvent, Reporter, SkippedOptionalPackage, SkippedOptionalReason};
use pretty_assertions::assert_eq;
use std::{collections::HashMap, sync::Mutex};

// Per-test recording reporter. Its `Mutex<Vec<LogEvent>>` buffer is fn-local,
// so each `#[test]` captures into its own and concurrent tests never share or
// race on it. Each test names the helpers it drives, so every emitted helper is
// used and none needs a `dead_code` allow.
macro_rules! recording_reporter {
    ($($helper:ident),* $(,)?) => {
        static RECORDED_EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

        struct RecordingReporter;
        impl Reporter for RecordingReporter {
            fn emit(event: &LogEvent) {
                RECORDED_EVENTS.lock().expect("RECORDED_EVENTS not poisoned").push(event.clone());
            }
        }

        $( recording_reporter!(@helper $helper); )*
    };

    (@helper take_events) => {
        fn take_events() -> Vec<LogEvent> {
            std::mem::take(&mut *RECORDED_EVENTS.lock().expect("RECORDED_EVENTS not poisoned"))
        }
    };
    (@helper reset_events) => {
        fn reset_events() {
            RECORDED_EVENTS.lock().expect("RECORDED_EVENTS not poisoned").clear();
        }
    };
    (@helper $unknown:ident) => {
        compile_error!(concat!(
            "unknown `recording_reporter!` helper `",
            stringify!($unknown),
            "`; expected one of: take_events, reset_events",
        ));
    };
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

fn no_importers() -> HashMap<String, ProjectSnapshot> {
    HashMap::new()
}

/// A `.` root importer whose `dependencies` / `optionalDependencies`
/// entries resolve each `name@version` pair through the virtual store.
fn root_importer(
    dependencies: &[&str],
    optional_dependencies: &[&str],
) -> HashMap<String, ProjectSnapshot> {
    let importer = ProjectSnapshot {
        dependencies: importer_dep_map(dependencies),
        optional_dependencies: importer_dep_map(optional_dependencies),
        ..Default::default()
    };
    std::iter::once((".".to_string(), importer)).collect()
}

fn importer_dep_map(entries: &[&str]) -> Option<ResolvedDependencyMap> {
    if entries.is_empty() {
        return None;
    }
    let map = entries
        .iter()
        .map(|name_at_version| {
            let key = snapshot_key(name_at_version);
            let spec = ResolvedDependencySpec {
                specifier: "*".to_string(),
                version: ImporterDepVersion::Regular(key.suffix.clone()),
            };
            (key.name, spec)
        })
        .collect();
    Some(map)
}

fn snapshot_dep_map(entries: &[&str]) -> Option<HashMap<PkgName, SnapshotDepRef>> {
    if entries.is_empty() {
        return None;
    }
    let map = entries
        .iter()
        .map(|name_at_version| {
            let key = snapshot_key(name_at_version);
            (key.name, SnapshotDepRef::Plain(key.suffix))
        })
        .collect();
    Some(map)
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
    recording_reporter!(reset_events, take_events);
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
        &no_importers(),
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

#[test]
fn compatible_snapshots_are_not_skipped() {
    recording_reporter!(reset_events, take_events);
    reset_events();
    let key = snapshot_key("compat@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, Some(&["darwin", "linux"]), None));

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &no_importers(),
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
    recording_reporter!(reset_events, take_events);
    reset_events();
    let key = snapshot_key("non-optional-but-wrong-os@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: false, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, Some(&["this-os-does-not-exist"]), None));

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &no_importers(),
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
    recording_reporter!(reset_events, take_events);
    reset_events();
    let key = snapshot_key("no-constraints@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, None, None));

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &no_importers(),
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
fn required_only_constraint_does_not_trigger_optional_gate() {
    let key = snapshot_key("not-compatible-with-any-os@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: false, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, Some(&["this-os-does-not-exist"]), None));

    assert!(any_installability_constraint(&snapshots, &packages));
    assert!(
        !any_optional_installability_constraint(&snapshots, &packages),
        "fresh pre-pass should stay off when only required snapshots carry constraints",
    );
}

#[test]
fn optional_constraint_triggers_optional_gate() {
    let key = snapshot_key("not-compatible-with-any-os@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, Some(&["this-os-does-not-exist"]), None));

    assert!(
        any_optional_installability_constraint(&snapshots, &packages),
        "fresh pre-pass should run when an optional snapshot can be skipped",
    );
}

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

#[test]
fn supported_architectures_widens_accept_set_so_optional_stays() {
    recording_reporter!(reset_events, take_events);
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
        &no_importers(),
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
    recording_reporter!(reset_events);
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
        &no_importers(),
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
fn seeded_still_incompatible_snapshot_stays_skipped() {
    recording_reporter!(reset_events, take_events);
    reset_events();
    let key = snapshot_key("for-legacy-node@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key.clone(), synthetic_metadata(Some(&[("node", "0.10")]), None, None, None));

    let seed = SkippedSnapshots::from_set(std::iter::once(key.clone()).collect());
    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &no_importers(),
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
        events.iter().any(|event| matches!(event, LogEvent::SkippedOptionalDependency(_))),
        "the skip is re-evaluated and re-reported on every install, got {events:?}",
    );
}

/// A seeded installability skip whose package passes the current check —
/// e.g. `supportedArchitectures` changed between installs — is dropped, so
/// the newly compatible optional is installed instead of staying skipped.
#[test]
fn seeded_snapshot_compatible_with_new_host_is_unskipped() {
    recording_reporter!(reset_events);
    reset_events();
    let key = snapshot_key("darwin-arm64-only@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages
        .insert(key.clone(), synthetic_metadata(None, Some(&["arm64"]), Some(&["darwin"]), None));

    let seed = SkippedSnapshots::from_set(std::iter::once(key.clone()).collect());
    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &no_importers(),
        &snapshots,
        &packages,
        &host("20.10.0", "darwin", "arm64"),
        "/proj",
        seed,
    )
    .unwrap();

    assert!(
        !skipped.contains(&key),
        "a seeded skip must be dropped once the package passes the installability check",
    );
}

#[test]
fn seeded_non_optional_snapshot_is_rechecked() {
    recording_reporter!(reset_events, take_events);
    reset_events();
    let key = snapshot_key("not-compatible-with-any-os@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: false, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key.clone(), synthetic_metadata(None, None, Some(&["missing-os"]), None));

    let seed = SkippedSnapshots::from_set(std::iter::once(key.clone()).collect());
    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &no_importers(),
        &snapshots,
        &packages,
        &host("20.10.0", "darwin", "arm64"),
        "/proj",
        seed,
    )
    .unwrap();

    assert!(!skipped.contains(&key), "non-optional snapshots must not stay seeded as skipped");
    let events = take_events();
    assert!(
        events.iter().all(|event| !matches!(event, LogEvent::SkippedOptionalDependency(_))),
        "non-optional incompatibility must not emit skipped-optional events, got {events:?}",
    );
}

#[test]
fn fast_path_preserves_seed() {
    recording_reporter!(reset_events);
    reset_events();
    let key = snapshot_key("previously-skipped@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key.clone(), synthetic_metadata(None, None, None, None));

    let seed = SkippedSnapshots::from_set(std::iter::once(key.clone()).collect());
    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &no_importers(),
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
fn fast_path_drops_seed_for_non_optional_snapshot() {
    recording_reporter!();
    let key = snapshot_key("previously-skipped@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: false, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key.clone(), synthetic_metadata(None, None, None, None));

    let seed = SkippedSnapshots::from_set(std::iter::once(key).collect());
    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &no_importers(),
        &snapshots,
        &packages,
        &host("20.10.0", "darwin", "arm64"),
        "/proj",
        seed,
    )
    .unwrap();

    assert!(skipped.is_empty());
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
    recording_reporter!(reset_events, take_events);
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
        &no_importers(),
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
    recording_reporter!(reset_events);
    reset_events();
    let key = snapshot_key("@nx/nx-win32-arm64-msvc@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: false, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(None, None, None, None));

    let skipped = compute_skipped_snapshots::<RecordingReporter>(
        &no_importers(),
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
    recording_reporter!(reset_events);
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
        &no_importers(),
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
    assert!(
        any_optional_installability_constraint(&snapshots, &packages),
        "a platform-named optional snapshot must trigger the fresh pre-pass",
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

#[test]
fn engine_strict_hard_fails_a_required_incompatible_dep() {
    recording_reporter!(reset_events);
    let key = snapshot_key("needs-newer-node@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: false, ..Default::default() });
    let mut packages = HashMap::new();
    packages.insert(key, synthetic_metadata(Some(&[("node", ">=99")]), None, None, None));

    let strict_host = InstallabilityHost {
        node_version: "20.10.0".to_string(),
        node_detected: true,
        os: "darwin",
        cpu: "arm64",
        libc: "unknown",
        supported_architectures: None,
        engine_strict: true,
    };
    reset_events();
    let strict = compute_skipped_snapshots::<RecordingReporter>(
        &no_importers(),
        &snapshots,
        &packages,
        &strict_host,
        "/proj",
        SkippedSnapshots::new(),
    );
    assert!(strict.is_err(), "engine_strict must hard-fail a required incompatible dep");

    // The same graph under the default (non-strict) host only warns and installs.
    reset_events();
    let lenient = compute_skipped_snapshots::<RecordingReporter>(
        &no_importers(),
        &snapshots,
        &packages,
        &host("20.10.0", "darwin", "arm64"),
        "/proj",
        SkippedSnapshots::new(),
    );
    assert!(lenient.is_ok(), "without engine_strict a required incompatible dep is only a warning");
}

/// The graph of TS `fail on unsupported dependency of optional
/// dependency` (`optionalDependencies.ts:552`): an installable
/// optional's *regular* dependency is incompatible, so the required
/// dispatch wins over the snapshot-level `optional: true` flag.
#[test]
fn engine_strict_fails_incompatible_regular_dep_of_installed_optional() {
    recording_reporter!(reset_events, take_events);
    let importers = root_importer(&[], &["has-not-compatible-dep@1.0.0"]);
    let parent = snapshot_key("has-not-compatible-dep@1.0.0");
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
    packages.insert(parent, synthetic_metadata(None, None, None, None));
    packages.insert(
        incompatible,
        synthetic_metadata(None, None, Some(&["this-os-does-not-exist"]), None),
    );
    packages.insert(grandchild, synthetic_metadata(None, None, None, None));

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
    assert!(
        strict.is_err(),
        "an incompatible regular dep of an installed optional must fail under engine_strict",
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
    assert!(lenient.is_empty(), "without engine_strict the required dep is warned, not skipped");
    let events = take_events();
    assert!(
        events.iter().all(|event| !matches!(event, LogEvent::SkippedOptionalDependency(_))),
        "a warned required dep must not fire skipped-optional events, got {events:?}",
    );
}

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
