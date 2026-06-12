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

/// Mirrors `optionalDependencies.ts:74` `skip optional dependency that
/// does not support the current OS`: an optional package whose `os`
/// list excludes the host is skipped, and the
/// `pnpm:skipped-optional-dependency` event carries the
/// `unsupported_platform` reason.
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

/// Mirrors `optionalDependencies.ts:143` `skip optional dependency
/// that does not support the current Node version`. The engine
/// rejection surfaces as `unsupported_engine`.
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

/// Compatible snapshots stay out of the skip set and trigger no events.
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

/// A non-optional incompatible package does NOT get skipped — it
/// surfaces a tracing-level warning and proceeds, matching pnpm's
/// non-engineStrict default. Verifies the skip set stays empty in
/// that case.
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

/// Fast path: a lockfile where no metadata row declares any
/// installability constraint skips the per-snapshot pass entirely.
/// Verifies the optimization triggers and produces the same
/// observable behavior as the slow path (empty skip set, no events).
#[test]
fn no_constraints_skips_the_per_snapshot_pass() {
    reset_events();
    let key = snapshot_key("no-constraints@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    // No engines / cpu / os / libc — the fast path returns an
    // empty SkippedSnapshots without inspecting individual
    // snapshots.
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

/// `cpu` / `os` / `libc` set to the `["any"]` sentinel is a no-op
/// in `check_platform`'s `check_list`, so it must not trigger the
/// slow path either.
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

/// Empty `cpu` / `os` / `libc` lists carry no exclusion either —
/// they cannot reject any host value. Should not block the fast
/// path.
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

/// A meaningful `engines.node` triggers the slow path. Sanity check
/// the predicate doesn't over-aggressively fast-path.
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

/// A meaningful non-`any` platform value triggers the slow path.
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

/// Peer-resolved variants of the same metadata row (e.g.
/// `react-dom@17.0.2(react@17.0.2)` vs the same against `react@18`)
/// must dedup at the reporter — upstream emits one event per
/// `pkgId`, not per snapshot.
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

    // Both snapshot variants land in the skip set (each variant has
    // its own virtual-store slot to suppress).
    assert!(skipped.contains(&snapshot_key_a));
    assert!(skipped.contains(&snapshot_key_b));

    // ...but the reporter only sees one event for the metadata row.
    let events = take_events();
    let skipped_events_count = events
        .iter()
        .filter(|event| matches!(event, LogEvent::SkippedOptionalDependency(_)))
        .count();
    assert_eq!(skipped_events_count, 1, "must dedup per metadata row");
}

/// `supportedArchitectures` widens the host triple so an optional
/// package whose `os` would normally exclude the host stays in the
/// install set when the user opts in via config / CLI. Ports the
/// install-step half of upstream's
/// `optionalDependencies.ts:594` `install optional dependency for
/// the supported architecture set by the user`.
///
/// Setup: snapshot's metadata declares `os: ['darwin']`, but the
/// host is linux. Without `supportedArchitectures`, the snapshot
/// would be skipped (slice 1's behavior). With
/// `supportedArchitectures.os = ['darwin']` set, the per-axis
/// accept list is `['darwin']` instead of `['linux']`, and the
/// snapshot stays.
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

/// `supportedArchitectures` is additive in pacquet's semantics: an
/// axis explicitly listing only one value still skips packages
/// whose constraint excludes that value. Setup: package wants
/// `os: ['darwin']`, host is linux, supportedArchitectures.os =
/// ['linux'] (NOT darwin). The skip still fires.
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

/// A seeded snapshot bypasses the per-snapshot re-check entirely
/// and emits no `pnpm:skipped-optional-dependency` event. Mirrors
/// upstream's early return at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L194>:
/// a snapshot listed in `.modules.yaml.skipped` from the previous
/// install is treated as already skipped without re-running
/// `package_is_installable`, so the user is not re-notified of a
/// known skip on every reinstall.
#[test]
fn seeded_snapshot_short_circuits_recheck() {
    reset_events();
    let key = snapshot_key("for-legacy-node@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    // Metadata is still incompatible (`engines.node: "0.10"`).
    // Without the short-circuit a recompute would skip the package
    // AND emit a `pnpm:skipped-optional-dependency` event; with the
    // short-circuit the snapshot stays in the seeded set without
    // re-emitting.
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

/// On the fast path (no constraint anywhere in the lockfile) the
/// seed survives verbatim. Same upstream rationale as the seeded
/// short-circuit: a previously skipped snapshot must not re-appear
/// just because the lockfile's per-snapshot constraints were
/// dropped between installs.
#[test]
fn fast_path_preserves_seed() {
    reset_events();
    let key = snapshot_key("previously-skipped@1.0.0");
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry { optional: true, ..Default::default() });
    let mut packages = HashMap::new();
    // Constraint-free metadata so `any_installability_constraint`
    // returns false; the per-snapshot loop is short-circuited and
    // the seed becomes the final set.
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

/// `from_strings` silently drops unparsable depPath entries.
/// Orphaned strings — e.g. a snapshot that has since been removed
/// from the lockfile, or a malformed line from a hand-edited
/// `.modules.yaml` — must not crash the read; they simply don't
/// match any current key and survive as no-ops in the set.
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

/// The three subsets (`installability`, `fetch_failed`,
/// `optional_excluded`) must stay disjoint so [`SkippedSnapshots::len`]
/// and [`SkippedSnapshots::iter`] stay consistent with
/// [`SkippedSnapshots::contains`]. The realistic overlap is a
/// platform-incompatible optional snapshot installed with
/// `--no-optional`: the same key would be added to both
/// `installability` (via the installability check) and
/// `optional_excluded` (via the `--no-optional` filter).
/// `add_optional_excluded` must no-op when the key is already
/// installability-skipped; precedence is "installability wins"
/// because that subset persists across installs.
#[test]
fn disjoint_subsets_preserve_len_and_iter() {
    let key: PackageKey = "platform-mismatch-optional@1.0.0".parse().unwrap();
    let mut skipped = SkippedSnapshots::new();
    skipped.insert_installability(key.clone());
    skipped.add_optional_excluded(key.clone());
    // Even though both `add_*` methods were called for `key`, the
    // higher-precedence subset wins — `len` and `iter` see exactly
    // one entry, matching `contains`.
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped.iter().count(), 1);
    assert!(skipped.contains(&key));

    // `add_fetch_failed` against the same key is similarly a no-op
    // — installability has highest precedence.
    skipped.add_fetch_failed(key);
    assert_eq!(skipped.len(), 1);
}

/// `add_fetch_failed` and `add_optional_excluded` are symmetric
/// against each other — neither inserts when the other subset
/// already has the key, so first-insert wins regardless of call
/// order. This overlap can't arise in practice (a snapshot
/// dropped by `--no-optional` never reaches the cold-batch
/// dispatch where `fetch_failed` is populated), but the guard
/// makes the public API safe to call in any order without
/// breaking the disjoint-subset invariant.
#[test]
fn fetch_failed_and_optional_excluded_are_symmetric() {
    // Order A: fetch_failed first, then optional_excluded — the
    // second insert no-ops; the entry stays in fetch_failed.
    let key: PackageKey = "weird-overlap@1.0.0".parse().unwrap();
    let mut skipped_a = SkippedSnapshots::new();
    skipped_a.add_fetch_failed(key.clone());
    skipped_a.add_optional_excluded(key.clone());
    assert_eq!(skipped_a.len(), 1);
    assert_eq!(skipped_a.iter().count(), 1);
    assert!(skipped_a.contains(&key));

    // Order B: optional_excluded first, then fetch_failed — the
    // second insert must also no-op (Copilot PR <https://github.com/pnpm/pacquet/pull/485> review:
    // `add_fetch_failed` needs the symmetric guard so callers
    // can't corrupt the skip set by reversing the order).
    let mut skipped_b = SkippedSnapshots::new();
    skipped_b.add_optional_excluded(key.clone());
    skipped_b.add_fetch_failed(key.clone());
    assert_eq!(skipped_b.len(), 1);
    assert_eq!(skipped_b.iter().count(), 1);
    assert!(skipped_b.contains(&key));
}

/// An optional snapshot whose metadata row carries no platform
/// fields (some registries strip os/cpu/libc from the metadata they
/// serve, and lockfile entries written from such metadata lack them
/// too) is still skipped when its package name declares an
/// unsupported platform. Ports the headless half of upstream's
/// `optionalDependencies.ts` `skip optional dependencies that do not
/// support the target architecture when their lockfile entries have
/// no platform fields`.
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

/// The platform is not inferred from the name of a non-optional
/// snapshot: a regular dependency that happens to carry a platform
/// token in its name installs everywhere unless its metadata says
/// otherwise.
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

/// A missing libc field alone is taken from the package name: with
/// `supportedArchitectures.libc = ['glibc']`, the `-musl` variant is
/// skipped and the `-gnu` variant stays, even though neither
/// metadata row declares `libc`.
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

/// An optional snapshot whose name carries a platform token while
/// its metadata row declares no platform fields must block the
/// fast path — otherwise the inference never gets a chance to run.
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

/// A name without an operating-system token never marks a package
/// that declares no platform fields as platform-specific, so it
/// must not block the fast path either.
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
