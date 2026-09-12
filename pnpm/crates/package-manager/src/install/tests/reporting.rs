use super::{
    super::{Install, InstallError, ProjectMutation},
    InstallDirs, PARTIAL_INSTALL_LOCKFILE, recorded_verified_file_integrity_report,
    seed_placeholder_virtual_store_slot,
};
use crate::{InstallWithFreshLockfileError, MinimumReleaseAgeError, PolicyExcludes};
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{
    BrokenModulesLog, ContextLog, IgnoredScriptsLog, LogEvent, PackageManifestLog,
    PackageManifestMessage, ProgressLog, ProgressMessage, Reporter, ScopeLog, SilentReporter,
    Stage, StageLog, StatsLog, StatsMessage, SummaryLog,
};
use pnpm_store_dir::{STORE_VERSION, VerifiedFileIntegrity};
use pnpm_testing_utils::registry::TestRegistry;
use std::{sync::Mutex, time::Duration};
use tempfile::tempdir;
use text_block_macros::text_block;

#[tokio::test]
async fn fresh_install_reports_strict_minimum_release_age_violations_before_writing() {
    let mock_instance = TestRegistry::start();
    let dir = tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    let mut manifest = PackageManifest::create_if_needed(dir.path().join("package.json")).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dir.path().join("store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    config.registry = mock_instance.url();
    config.minimum_release_age = Some(60 * 24 * 365 * 100);
    config.minimum_release_age_strict = Some(true);
    let config = config.leak();

    let error = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: Some(false),
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        preferred_versions_override: None,
        auth_override: None,
        resolution_observer: None,
        peer_issues_sink: None,
        deps_requiring_build_sink: None,
        catalogs_override: None,
        disable_optimistic_repeat_install: false,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run_with_prompt_eligibility::<SilentReporter>(false)
    .await
    .expect_err("strict non-interactive install must reject immature picks");

    assert!(matches!(
        error,
        InstallError::WithFreshLockfile(InstallWithFreshLockfileError::MinimumReleaseAge(
            MinimumReleaseAgeError::NoMatureMatchingVersion { .. }
        ))
    ));
    assert!(!dir.path().join("pnpm-lock.yaml").exists());
    assert!(!modules_dir.exists());
}
/// [`Install::run`] emits `pnpm:package-manifest initial`,
/// `pnpm:context`, then `pnpm:stage` `importing_started`, then on
/// the success path `importing_done` followed by `pnpm:summary`.
/// On an early-error path such as [`InstallError::NoLockfile`]
/// only the leading events fire. This matches pnpm: the manifest
/// snapshot lands first so consumers can diff it against
/// `updated`, context is emitted alongside the install header, the
/// stage pairing drives the JS reporter's progress UI, and summary
/// closes the run so the reporter can render its "+N -M" block.
///
/// `pnpm:package-import-method` is emitted lazily by `link_file`
/// the first time each method actually resolves (after `auto`'s
/// fallback chain finishes), so an empty-lockfile install like this
/// one has no `link_file` calls and no such event in the captured
/// sequence. See `link_file::tests` for that channel's coverage.
///
/// `pnpm:context` carries `currentLockfileExists`, `storeDir`,
/// `virtualStoreDir`. `currentLockfileExists` is hard-coded
/// `false` today (pacquet doesn't read or write
/// `node_modules/.pnpm/lock.yaml`), matching the TODO in
/// [`Install::run`].
#[tokio::test]
async fn install_emits_pnpm_event_sequence() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    // Reset in case nextest reuses the process for a retry of this test.
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    config
        .registries_by_scope
        .insert("@private".to_string(), "https://private.example.com/npm/".to_string());
    let config = config.leak();

    // Empty v9 lockfile: `--frozen-lockfile` walks an empty snapshot
    // set successfully, which is the cheapest "real" install path.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies: {}"
        "packages: {}"
        "snapshots: {}"
    })
    .expect("parse minimal v9 lockfile");

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        preferred_versions_override: None,
        auth_override: None,
        resolution_observer: None,
        peer_issues_sink: None,
        deps_requiring_build_sink: None,
        catalogs_override: None,
        disable_optimistic_repeat_install: false,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run::<RecordingReporter>()
    .await
    .expect("empty-lockfile frozen install should succeed");

    let captured = EVENTS.lock().unwrap();

    // Event ordering: the scope this run covers, manifest snapshot,
    // context, importing_started, the `pnpm:stats` added/removed pair
    // from `CreateVirtualStore::run`, then `importing_done` once
    // extraction and symlink linking are complete,
    // followed by the `pnpm:ignored-scripts` summary that
    // `BuildModules::run` produces, then summary closing the run. The
    // empty snapshot map still triggers the stats emit (`added: 0`,
    // `removed: 0`), matching pnpm's unconditional emit at link time.
    // The empty lockfile produces no ignored builds, so
    // `ignored-scripts` carries an empty list. Outside a workspace the
    // scope is the single project, which is the payload pnpm reports
    // for a run it doesn't spread over a workspace.
    assert!(
        matches!(
            captured.as_slice(),
            [
                LogEvent::Scope(ScopeLog { selected: 1, total: None, .. }),
                LogEvent::PackageManifest(PackageManifestLog {
                    message: PackageManifestMessage::Initial { .. },
                    ..
                }),
                LogEvent::Context(_),
                LogEvent::Stage(StageLog { stage: Stage::ImportingStarted, .. }),
                LogEvent::Stats(StatsLog { message: StatsMessage::Added { added: 0, .. }, .. }),
                LogEvent::Stats(StatsLog { message: StatsMessage::Removed { removed: 0, .. }, .. }),
                LogEvent::Stage(StageLog { stage: Stage::ImportingDone, .. }),
                LogEvent::IgnoredScripts(_),
                LogEvent::Summary(_),
            ],
        ),
        "unexpected event sequence: {captured:?}",
    );

    // Empty lockfile produces no ignored builds.
    let LogEvent::IgnoredScripts(IgnoredScriptsLog { package_names, .. }) = &captured[7] else {
        unreachable!("ignored-scripts at index 7, asserted above");
    };
    assert!(package_names.is_empty(), "no builds in empty lockfile: {package_names:?}");

    let expected_prefix = manifest.path().parent().unwrap().to_string_lossy().into_owned();

    // Manifest event carries the on-disk JSON unchanged so consumers
    // can diff `initial` vs a later `updated` byte-for-byte.
    let LogEvent::PackageManifest(PackageManifestLog {
        message: PackageManifestMessage::Initial { prefix: manifest_prefix, initial },
        ..
    }) = &captured[1]
    else {
        unreachable!("package-manifest follows the scope, asserted above");
    };
    assert_eq!(manifest_prefix, &expected_prefix);
    assert_eq!(initial, manifest.value());

    // Spot-check the context payload: pacquet's directories must
    // round-trip through the wire shape, and `currentLockfileExists`
    // is `false` on this first install because no `lock.yaml` exists
    // in the (just-created) virtual store yet — pacquet writes the
    // file at end-of-install, so the next install would see `true`.
    let LogEvent::Context(ContextLog {
        current_lockfile_exists,
        store_dir: emitted_store_dir,
        virtual_store_dir: emitted_virtual_store_dir,
        ..
    }) = &captured[2]
    else {
        unreachable!("context follows the manifest snapshot, asserted above");
    };
    assert!(!current_lockfile_exists);
    assert_eq!(emitted_store_dir, &dirs.store_dir.join(STORE_VERSION).display().to_string());
    assert_eq!(emitted_virtual_store_dir, &dirs.virtual_store_dir.to_string_lossy().into_owned());

    // Summary's `prefix` must equal the manifest-parent value
    // `Install::run` derives, since pnpm's reporter keys its
    // accumulated root-events by prefix to render the diff.
    let LogEvent::Summary(SummaryLog { prefix: summary_prefix, .. }) = captured.last().unwrap()
    else {
        unreachable!("last event is summary, asserted above");
    };
    assert_eq!(summary_prefix, &expected_prefix);

    drop(dirs.dir);
}
/// When the cached directory is gone but the cache key still matches,
/// pacquet emits `pnpm:_broken_node_modules` and falls through to the
/// full install path for that snapshot.
#[tokio::test]
async fn warm_reinstall_emits_broken_modules_when_dir_is_missing() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // Manifest must match `PARTIAL_INSTALL_LOCKFILE` — the freshness
    // check (<https://github.com/pnpm/pacquet/issues/447>) rejects any drift between the on-disk manifest and
    // the lockfile importer entry.
    manifest.add_dependency("placeholder", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    // Opt out of the GVS layout — see the rationale on
    // [`warm_reinstall_skips_snapshot_when_current_lockfile_matches`].
    // The pre-seeded `<dirs.virtual_store_dir>/<flat-name>` slot is the
    // legacy shape the probe matches; the BrokenModules emit fires
    // identically under either layout once the slot is missing.
    config.enable_global_virtual_store = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    // Skip fetch retries entirely — the install is expected to fail
    // after emitting `_broken_node_modules`, so any retry budget is
    // pure waste here.
    config.fetch_retries = 0;
    config.fetch_retry_mintimeout = 1;
    config.fetch_retry_maxtimeout = 1;
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

    // Pre-seed the current lockfile but deliberately *not* the
    // virtual-store slot — the cache key matches but the directory is
    // gone (the `rm -rf node_modules/.pnpm/<slot>` scenario).
    std::fs::create_dir_all(&dirs.virtual_store_dir).unwrap();
    lockfile
        .save_current_to_virtual_store_dir(&dirs.virtual_store_dir)
        .expect("seed current lockfile");

    // The install will attempt to fetch the placeholder (bogus URL),
    // which fails — what we're testing is that the broken-modules
    // signal fires *before* the fetch happens. So we look for the
    // event in the captured set regardless of the final install
    // result.
    let _ = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: true, // fixture pins a tripwire tarball URL; skip resolution verification so the tarball-URL check doesn't flag it before the path under test
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        preferred_versions_override: None,
        auth_override: None,
        resolution_observer: None,
        peer_issues_sink: None,
        deps_requiring_build_sink: None,
        catalogs_override: None,
        disable_optimistic_repeat_install: false,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run::<RecordingReporter>()
    .await;

    let captured = EVENTS.lock().unwrap();
    let broken: Vec<&BrokenModulesLog> = captured
        .iter()
        .filter_map(|event| match event {
            LogEvent::BrokenModules(b) => Some(b),
            _ => None,
        })
        .collect();
    assert_eq!(
        broken.len(),
        1,
        "expected exactly one pnpm:_broken_node_modules emit; got: {captured:?}",
    );
    assert!(
        broken[0].missing.contains("placeholder@1.0.0"),
        "broken-modules `missing` path must name the affected slot; got: {missing}",
        missing = broken[0].missing,
    );

    drop(dirs.dir);
}
/// The skip path drops the snapshot from both the warm and cold
/// batches, so a warm reinstall must report `added: 0` and emit
/// zero `pnpm:progress imported` events. Pre-seeds `lock.yaml` and
/// the virtual-store slot manually here — the
/// [`context_log_reflects_current_lockfile_after_first_install`]
/// test covers the read-after-write loop on its own, so this one
/// can focus on the skip's reporter-visible effect.
#[tokio::test]
async fn warm_reinstall_reports_added_zero_and_emits_no_imported_events() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // Manifest must match `PARTIAL_INSTALL_LOCKFILE` — the freshness
    // check (<https://github.com/pnpm/pacquet/issues/447>) rejects any drift between the on-disk manifest and
    // the lockfile importer entry.
    manifest.add_dependency("placeholder", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    // Opt out of the GVS layout — the pre-seeded
    // `<dirs.virtual_store_dir>/<flat-name>` slot is the legacy shape the
    // skip probe matches under
    // [`warm_reinstall_skips_snapshot_when_current_lockfile_matches`].
    config.enable_global_virtual_store = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

    std::fs::create_dir_all(&dirs.virtual_store_dir).unwrap();
    lockfile
        .save_current_to_virtual_store_dir(&dirs.virtual_store_dir)
        .expect("seed current lockfile");
    seed_placeholder_virtual_store_slot(&dirs.virtual_store_dir);

    EVENTS.lock().unwrap().clear();
    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: true, // fixture pins a tripwire tarball URL; skip resolution verification so the tarball-URL check doesn't flag it before the path under test
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        preferred_versions_override: None,
        auth_override: None,
        resolution_observer: None,
        peer_issues_sink: None,
        deps_requiring_build_sink: None,
        catalogs_override: None,
        disable_optimistic_repeat_install: false,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run::<RecordingReporter>()
    .await
    .expect("warm reinstall should succeed via the skip path");

    // Stats reports `added: 0` — the only snapshot is the one that
    // got skipped.
    let added: Vec<u64> = EVENTS
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            LogEvent::Stats(StatsLog { message: StatsMessage::Added { added, .. }, .. }) => {
                Some(*added)
            }
            _ => None,
        })
        .collect();
    assert_eq!(added, vec![0], "warm reinstall must report added: 0; got {added:?}");

    // No per-snapshot `imported` progress event — the skip path
    // removes the snapshot from both warm and cold batches.
    let imported_count = EVENTS
        .lock()
        .unwrap()
        .iter()
        .filter(|e| {
            matches!(
                e,
                LogEvent::Progress(ProgressLog { message: ProgressMessage::Imported { .. }, .. }),
            )
        })
        .count();
    assert_eq!(
        imported_count, 0,
        "skip path must suppress `pnpm:progress imported` for skipped snapshots",
    );

    drop(dirs.dir);
}
/// The report is what tells a user why an otherwise warm install took
/// so long, or that their store is being churned even when it didn't.
/// Both message strings are a cross-stack contract: pnpm renders the
/// same ones from the same figures.
#[test]
fn slow_store_verification_is_reported_with_its_time() {
    let messages = recorded_verified_file_integrity_report(VerifiedFileIntegrity {
        files: 1234,
        duration: Duration::from_millis(2450),
    });
    assert_eq!(messages, vec!["The integrity of 1234 files was checked in 2.5s.".to_string()]);
}
/// Under the time threshold there is no time worth naming, so the
/// message points at what keeps invalidating the store instead.
#[test]
fn quick_verification_of_many_files_is_reported_as_churn() {
    let messages = recorded_verified_file_integrity_report(VerifiedFileIntegrity {
        files: 1001,
        duration: Duration::from_millis(80),
    });
    assert_eq!(
        messages,
        vec![
            "The integrity of 1001 files was checked, because their timestamps changed since the store recorded them. A backup tool, an antivirus scan, or a copied store can cause this."
                .to_string(),
        ],
    );
}
/// Both thresholds are exclusive, and a healthy store sits far below
/// either — the install says nothing at all.
#[test]
fn verification_below_both_thresholds_is_not_reported() {
    for verified in [
        VerifiedFileIntegrity { files: 1000, duration: Duration::from_secs(1) },
        VerifiedFileIntegrity { files: 12, duration: Duration::from_millis(3) },
        VerifiedFileIntegrity { files: 0, duration: Duration::ZERO },
    ] {
        assert_eq!(recorded_verified_file_integrity_report(verified), Vec::<String>::new());
    }
}
/// Each install reports its own verification, so a second install in
/// the same process (a recursive workspace run, or an embedder driving
/// several) doesn't re-report the first one's work.
#[test]
fn verified_file_integrity_is_scoped_to_one_install() {
    let baseline = VerifiedFileIntegrity { files: 400, duration: Duration::from_secs(9) };
    let after = VerifiedFileIntegrity { files: 401, duration: Duration::from_millis(9_100) };

    let this_install = after.since(baseline);
    dbg!(this_install);
    assert_eq!(this_install.files, 1);
    assert_eq!(this_install.duration, Duration::from_millis(100));
}
