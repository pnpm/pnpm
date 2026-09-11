use super::{
    super::{
        Install, InstallError, ProjectMutation, UpToDateFastPathCheck, install_already_up_to_date,
    },
    InstallDirs, PARTIAL_INSTALL_LOCKFILE, assert_package_absent, assert_package_present,
    fresh_lockfile_only_with_overrides, install_then_go_offline, install_with_pnpmfile,
    touch_manifest,
};
use crate::PolicyExcludes;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::{Host, LayoutVersion, Modules, NodeLinker, write_modules_manifest};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LockfileVerificationMessage, LogEvent, Reporter, SilentReporter};
use pnpm_testing_utils::registry::TestRegistry;
use pnpm_workspace_state as workspace_state;
use std::sync::Mutex;
use tempfile::tempdir;

/// Dispatch state 3b: lockfile present, but the manifest has drifted
/// from it; no `--frozen-lockfile` flag. The freshness gate inside the
/// auto-frozen branch must fail, and the dispatch must fall through
/// to the fresh-resolve path instead of surfacing `OutdatedLockfile`
/// the way state 1 would. We assert via the same "unreachable
/// registry" sentinel as the previous test.
#[tokio::test]
async fn stale_lockfile_under_no_flag_falls_through_to_fresh_resolve() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    // Deliberately omit the `placeholder` dep — this drifts from
    // `PARTIAL_INSTALL_LOCKFILE`'s importer entry.
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.registry = "http://invalid.local/".to_string();
    config.enable_global_virtual_store = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
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
    .run::<SilentReporter>()
    .await;

    let err = result.expect_err(
        "fresh-resolve dispatch must consult the unreachable registry and fail; \
         a success would mean the dispatch silently took the auto-frozen path",
    );
    assert!(
        !matches!(err, InstallError::OutdatedLockfile { .. }),
        "stale-lockfile fall-through must not surface as OutdatedLockfile, got {err:?}",
    );
}
#[test]
fn sync_fast_path_reads_the_workspace_root_wanted_lockfile_from_a_member() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path().join("workspace");
    let project_root = workspace_root.join("packages/app");
    let modules_dir = workspace_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pnpm");
    std::fs::create_dir_all(&project_root).expect("create workspace member");
    std::fs::create_dir_all(project_root.join("node_modules"))
        .expect("create member modules directory");
    std::fs::create_dir_all(&virtual_store_dir).expect("create virtual store");
    std::fs::write(workspace_root.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    std::fs::write(
        workspace_root.join("package.json"),
        r#"{ "name": "workspace-root", "version": "1.0.0" }"#,
    )
    .expect("write root manifest");
    let manifest_path = project_root.join("package.json");
    std::fs::write(
        &manifest_path,
        r#"{ "name": "app", "version": "1.0.0", "dependencies": { "sibling": "link:../../sibling" } }"#,
    )
    .expect("write member manifest");
    let manifest = PackageManifest::from_path(manifest_path).expect("read member manifest");

    let current = "lockfileVersion: '9.0'\nimporters:\n  .: {}\n  packages/app:\n    dependencies:\n      sibling:\n        specifier: link:../../sibling\n        version: link:../../sibling\n";
    std::fs::write(virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME), current)
        .expect("write current lockfile");
    std::fs::write(project_root.join(Lockfile::FILE_NAME), "not: [valid")
        .expect("write invalid member lockfile");
    let wanted_path = workspace_root.join(Lockfile::FILE_NAME);
    std::fs::write(&wanted_path, current).expect("write wanted lockfile");

    let mut config = Config::new();
    config.workspace_dir = Some(workspace_root.clone());
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();
    let included = pnpm_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: true,
    };
    let mut projects = std::collections::BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        workspace_state::ProjectEntry {
            name: Some("workspace-root".to_string()),
            version: Some("1.0.0".to_string()),
        },
    );
    projects.insert(
        project_root.to_string_lossy().into_owned(),
        workspace_state::ProjectEntry {
            name: Some("app".to_string()),
            version: Some("1.0.0".to_string()),
        },
    );
    let validated_at = 0;
    workspace_state::update_workspace_state(
        &workspace_root,
        &pnpm_workspace_state::WorkspaceState {
            last_validated_timestamp: validated_at,
            projects,
            pnpmfiles: Vec::new(),
            filtered_install: false,
            config_dependencies: None,
            settings: crate::optimistic_repeat_install::settings::current_settings(
                config,
                pnpm_config::NodeLinker::Isolated,
                included,
                None,
            ),
        },
    )
    .expect("seed workspace state");
    let check = UpToDateFastPathCheck {
        config,
        manifest: &manifest,
        dependency_groups: vec![
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
        ],
        node_linker: pnpm_config::NodeLinker::Isolated,
        supported_architectures: None,
    };

    assert_eq!(
        install_already_up_to_date(&check).map(|up_to_date| up_to_date.root).as_deref(),
        Some(&*workspace_root),
    );
    assert_eq!(std::fs::read_to_string(&wanted_path).expect("reread wanted lockfile"), current);

    std::fs::write(project_root.join(Lockfile::FILE_NAME), current).expect("write member lockfile");
    std::fs::write(&wanted_path, "not: [valid").expect("write invalid root lockfile");
    let mut per_project_config = config.clone();
    per_project_config.shared_workspace_lockfile = false;
    let per_project_config = Config::leak(per_project_config);
    let per_project_check = UpToDateFastPathCheck {
        config: per_project_config,
        manifest: &manifest,
        dependency_groups: vec![
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
        ],
        node_linker: pnpm_config::NodeLinker::Isolated,
        supported_architectures: None,
    };

    assert_eq!(
        install_already_up_to_date(&per_project_check).map(|up_to_date| up_to_date.root).as_deref(),
        Some(&*workspace_root),
    );
}
/// Regression: a single-project install with NO lockfile anywhere —
/// `pnpm-lock.yaml` is gone and the virtual store has no current
/// `lock.yaml` to stand in for it — must NOT short-circuit, even when
/// `node_modules` and the workspace-state file survive. There is
/// nothing to content-check the manifests against and nothing to
/// regenerate `pnpm-lock.yaml` from, so the full install must run —
/// the missing-lockfile case converts into `upToDate: false`. When
/// the current lockfile IS present, the fast path instead treats it
/// as the wanted lockfile —
/// see `regenerates_missing_wanted_lockfile_from_current_when_manifests_unchanged`
/// in the `optimistic_repeat_install` tests. Companion to the
/// workspace-mode tolerance proved by
/// [`returns_up_to_date_in_workspace_mode_without_lockfile`](crate::optimistic_repeat_install::tests::returns_up_to_date_in_workspace_mode_without_lockfile).
#[tokio::test]
pub(super) async fn optimistic_repeat_install_does_not_short_circuit_when_lockfile_missing() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dirs = InstallDirs::new();

    std::fs::create_dir_all(&dirs.project_root).expect("create project root");
    std::fs::create_dir_all(&dirs.modules_dir)
        .expect("create modules dirs.dir so the deps gate passes");
    let manifest_path = dirs.project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest.add_dependency("sibling", "link:../sibling", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    // Deliberately do NOT write `pnpm-lock.yaml` and do NOT seed a
    // current `lock.yaml` in the virtual store — that's the scenario
    // under test.

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let included = pnpm_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };

    // Seed `.modules.yaml` and a fresh workspace state — same shape
    // as the happy-path optimistic test above. The only difference
    // is the missing `pnpm-lock.yaml`.
    let seed_modules = Modules {
        layout_version: Some(LayoutVersion),
        node_linker: Some(NodeLinker::Isolated),
        included,
        hoist_pattern: config.hoist_pattern.clone(),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        store_dir: config.store_dir.display().to_string(),
        virtual_store_dir: config.effective_virtual_store_dir().to_string_lossy().into_owned(),
        virtual_store_dir_max_length: config.virtual_store_dir_max_length,
        ..Default::default()
    };
    write_modules_manifest::<Host>(&dirs.modules_dir, seed_modules).expect("seed .modules.yaml");

    let mut projects = std::collections::BTreeMap::new();
    projects.insert(
        dirs.project_root.to_string_lossy().into_owned(),
        workspace_state::ProjectEntry {
            name: Some("project".to_string()),
            version: Some("1.0.0".to_string()),
        },
    );
    let settings = crate::optimistic_repeat_install::settings::current_settings(
        config,
        pnpm_config::NodeLinker::Isolated,
        included,
        None,
    );
    workspace_state::update_workspace_state(
        &dirs.project_root,
        &pnpm_workspace_state::WorkspaceState {
            last_validated_timestamp: pnpm_testing_utils::fs::backdate_existing_files(
                &dirs.project_root,
            ),
            projects,
            pnpmfiles: Vec::new(),
            filtered_install: false,
            config_dependencies: None,
            settings,
        },
    )
    .expect("seed workspace state");

    // We're not testing the full install pipeline here — without a
    // lockfile on disk the fresh-resolve path would try to resolve
    // `link:../sibling` against a directory that doesn't exist. We
    // only need to prove the optimistic short-circuit did NOT fire,
    // so swallow the install result.
    let _ = Install {
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
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::Isolated,
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
    assert!(
        !captured.iter().any(|event| matches!(
            event,
            LogEvent::Pnpm(log) if log.message == "Already up to date"
        )),
        "the optimistic 'Already up to date' log MUST NOT fire when \
         no lockfile exists in a single-project install; got events: {captured:#?}",
    );
}
/// A fresh install records its lockfile-verification verdict, so a
/// repeat install that reaches the full path (the optimistic fast
/// path is disabled here — it would otherwise absorb the touched
/// manifest via the content re-check) hits the cache and never fans
/// out to the registry.
#[tokio::test]
async fn fresh_install_records_lockfile_verification_for_mtime_bypassed_noop() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    std::fs::create_dir_all(&project_root).expect("create project root");
    let manifest_path = project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.cache_dir = cache_dir.clone();
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
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
    .run::<SilentReporter>()
    .await
    .expect("first install must succeed");

    let lockfile_path = project_root.join(Lockfile::FILE_NAME);
    let wanted_lockfile =
        Lockfile::load_wanted_from_dir(&project_root).expect("load wanted lockfile").unwrap();

    drop(mock_instance);

    let manifest_text = std::fs::read_to_string(&manifest_path).expect("read package.json");
    std::fs::write(&manifest_path, manifest_text).expect("refresh package.json mtime");
    pnpm_testing_utils::fs::bump_mtime(&manifest_path);
    let touched_manifest = PackageManifest::from_path(manifest_path).expect("reload manifest");

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let mut second_config = Config::new();
    second_config.cache_dir = cache_dir;
    second_config.store_dir = store_dir.into();
    second_config.modules_dir = modules_dir;
    second_config.virtual_store_dir = virtual_store_dir;
    second_config.registry = "http://127.0.0.1:9/".to_string();
    second_config.optimistic_repeat_install = false;
    let second_config = second_config.leak();

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config: second_config,
        manifest: &touched_manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&wanted_lockfile)),
        lockfile_path: Some(&lockfile_path),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
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
    .expect("second install must no-op without contacting the stopped registry");

    let captured = EVENTS.lock().unwrap();
    assert!(
        captured.iter().any(|event| matches!(
            event,
            LogEvent::Pnpm(log)
                if log.message == "Lockfile is up to date, resolution step is skipped"
        )),
        "second install must reach the modules/current-lockfile no-op path; got {captured:#?}",
    );
    let verification_messages: Vec<_> = captured
        .iter()
        .filter_map(|event| match event {
            LogEvent::LockfileVerification(log) => Some(&log.message),
            _ => None,
        })
        .collect();
    assert!(
        matches!(verification_messages.as_slice(), [LockfileVerificationMessage::Cached { .. }]),
        "verification cache hit must skip the fan-out and announce the reused verdict; got {captured:#?}",
    );

    drop(dir);
}
/// A repeat install with `pnpm-lock.yaml` deleted but `node_modules`
/// intact must short-circuit offline by treating the current lockfile
/// (`<virtual_store_dir>/lock.yaml`) as the wanted one, and must
/// restore `pnpm-lock.yaml` byte-identically. Guards the
/// current-as-wanted fallback end-to-end: a regression into the full
/// pipeline (resolution or the verification fan-out against an empty
/// cache) fails on the dead registry.
#[tokio::test]
async fn optimistic_repeat_install_restores_missing_lockfile_offline() {
    let (dir, offline_config, manifest) = install_then_go_offline().await;
    let project_root = manifest.path().parent().unwrap().to_path_buf();
    let lockfile_path = project_root.join(Lockfile::FILE_NAME);
    let original_lockfile_bytes =
        std::fs::read(&lockfile_path).expect("read pnpm-lock.yaml written by the first install");
    std::fs::remove_file(&lockfile_path).expect("delete pnpm-lock.yaml");
    let touched_manifest = touch_manifest(&manifest);

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config: offline_config,
        manifest: &touched_manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
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
    .expect("repeat install with a deleted pnpm-lock.yaml must not need the registry");

    let captured = EVENTS.lock().unwrap();
    assert!(
        captured.iter().any(|event| matches!(
            event,
            LogEvent::Pnpm(log) if log.message == "Already up to date"
        )),
        "the deleted-lockfile repeat install must take the fast path; got {captured:#?}",
    );
    let pipeline_emits = captured
        .iter()
        .filter(|event| {
            matches!(
                event,
                LogEvent::Context(_) | LogEvent::Stage(_) | LogEvent::LockfileVerification(_),
            )
        })
        .count();
    assert_eq!(
        pipeline_emits, 0,
        "the fast path must not run any install-setup step; got {captured:#?}",
    );

    let regenerated_bytes =
        std::fs::read(&lockfile_path).expect("pnpm-lock.yaml must be regenerated");
    assert_eq!(
        regenerated_bytes, original_lockfile_bytes,
        "the regenerated pnpm-lock.yaml must be byte-identical to the one the install wrote",
    );

    drop(dir);
}
#[tokio::test]
async fn fresh_lockfile_applies_overrides_to_direct_dependencies() {
    let (_dir, lockfile) = fresh_lockfile_only_with_overrides(
        &[("@pnpm.e2e/foo", "^100.0.0")],
        &[("@pnpm.e2e/foo@^100.0.0", "100.0.0")],
        None,
    )
    .await;

    assert_package_present(&lockfile, "@pnpm.e2e/foo@100.0.0");
    assert_package_absent(&lockfile, "@pnpm.e2e/foo@100.1.0");
}
#[tokio::test]
async fn fresh_lockfile_applies_overrides_to_transitive_dependencies() {
    let (_dir, lockfile) = fresh_lockfile_only_with_overrides(
        &[("@pnpm.e2e/has-foo-100.0.0-range-dep", "1.0.0")],
        &[("@pnpm.e2e/foo@^100.0.0", "100.0.0")],
        None,
    )
    .await;

    assert_package_present(&lockfile, "@pnpm.e2e/has-foo-100.0.0-range-dep@1.0.0");
    assert_package_present(&lockfile, "@pnpm.e2e/foo@100.0.0");
    assert_package_absent(&lockfile, "@pnpm.e2e/foo@100.1.0");
}
// The `afterAllResolved` hook receives the resolved
// lockfile object and its return value is what gets written, so an arbitrary
// added key must survive to pnpm-lock.yaml.
#[tokio::test]
async fn after_all_resolved_hook_modifies_written_lockfile() {
    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();

    install_with_pnpmfile(
        registry.url(),
        dir.path(),
        &[("@pnpm.e2e/pkg-with-1-dep", "100.0.0")],
        r"module.exports = { hooks: { afterAllResolved (lockfile) {
  lockfile.foo = 'foo';
  return lockfile;
} } }",
    )
    .await
    .expect("install should succeed");

    let lockfile_text = std::fs::read_to_string(dir.path().join("pnpm-lock.yaml")).unwrap();
    eprintln!("{lockfile_text}");
    assert!(
        lockfile_text.contains("foo: foo"),
        "the afterAllResolved addition must be written to pnpm-lock.yaml",
    );
    // The lockfile is still a valid lockfile carrying the resolved package.
    assert!(lockfile_text.contains("@pnpm.e2e/pkg-with-1-dep"));
}
