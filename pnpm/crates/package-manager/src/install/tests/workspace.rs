use super::{
    super::{
        Install, ProjectMutation, load_workspace_projects, project_requires_lifecycle_scripts,
    },
    InstallDirs, empty_test_lockfile, install_with_pnpmfile,
    install_workspace_member_with_pnpmfile,
};
use crate::PolicyExcludes;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::Modules;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, Reporter, SilentReporter};
use pnpm_testing_utils::registry::TestRegistry;
use pnpm_workspace_state::{
    self as workspace_state, NodeLinker as WorkspaceStateNodeLinker, load_workspace_state,
};
use std::{fs, sync::Mutex, time::Duration};
use tempfile::tempdir;
use text_block_macros::text_block;

#[test]
fn project_lifecycle_detection_includes_scripts_and_binding_gyp_fallback() {
    let temp = tempdir().unwrap();
    let project_dir = temp.path();
    let scriptless = PackageManifest::from_value(
        project_dir.join("package.json"),
        serde_json::json!({ "name": "project" }),
    );
    assert!(!project_requires_lifecycle_scripts(project_dir, &scriptless));

    fs::write(project_dir.join("binding.gyp"), "{}").unwrap();
    assert!(project_requires_lifecycle_scripts(project_dir, &scriptless));

    fs::remove_file(project_dir.join("binding.gyp")).unwrap();
    let with_prepare = PackageManifest::from_value(
        project_dir.join("package.json"),
        serde_json::json!({ "scripts": { "prepare": "node prepare.js" } }),
    );
    assert!(project_requires_lifecycle_scripts(project_dir, &with_prepare));
}
#[test]
fn workspace_without_packages_field_enumerates_root_only() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"root","version":"0.0.0","scripts":{"prepare":"node root.js"}}"#,
    )
    .expect("write root package.json");
    let nested = dir.path().join("test-e2e/fixtures/vendor/preact/.cache/10.10.2");
    fs::create_dir_all(&nested).expect("mkdir vendored package");
    fs::write(
        nested.join("package.json"),
        r#"{"name":"preact","version":"10.10.2","scripts":{"prepare":"run-s build"}}"#,
    )
    .expect("write vendored package.json");
    fs::write(dir.path().join("pnpm-workspace.yaml"), "allowBuilds:\n  esbuild: false\n")
        .expect("write settings-only workspace manifest");

    let manifest = pnpm_workspace::read_workspace_manifest(dir.path())
        .expect("read workspace manifest")
        .expect("workspace manifest present");

    let projects = load_workspace_projects(dir.path(), Some(&manifest))
        .expect("load workspace projects")
        .expect("workspace projects");
    let names: Vec<&str> = projects
        .iter()
        .filter_map(|project| project.manifest.value().get("name").and_then(|name| name.as_str()))
        .collect();

    assert_eq!(names, vec!["root"]);
}
/// Loose-mode counterpart of the strict test above —
/// [pnpm/pnpm#13687](https://github.com/pnpm/pnpm/issues/13687).
#[tokio::test]
async fn fresh_install_persists_loose_minimum_release_age_picks_to_workspace_manifest() {
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
    .expect("loose mode lets the immature pick through");

    assert!(dir.path().join("pnpm-lock.yaml").exists());
    let workspace = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml"))
        .expect("install must create pnpm-workspace.yaml with the persisted excludes");
    assert!(
        workspace.contains("minimumReleaseAgeExclude:")
            && workspace.contains("- '@pnpm.e2e/hello-world-js-bin@1.0.0'"),
        "unexpected workspace manifest: {workspace}",
    );
}
/// `pnpm run`'s `verifyDepsBeforeRun` gate bails to "outdated" the
/// moment `<workspaceDir>/node_modules/.pnpm-workspace-state-v1.json`
/// is missing. Pacquet must write it on every install so pnpm can
/// fast-path the check after pacquet has materialized the modules
/// tree — that's the gap behind the
/// `pnpm_config_verify_deps_before_run: false` workaround in pnpm's
/// own CI.
#[tokio::test]
async fn install_writes_workspace_state() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    pnpm_testing_utils::fs::set_mtime(
        manifest.path(),
        std::time::SystemTime::UNIX_EPOCH + Duration::new(1_700_000_000, 500_000),
    );

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

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
        // Same `included` shape as `install_writes_modules_yaml` so the
        // dev/optional/production assertions below line up with the
        // dispatched groups.
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
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
    .run::<SilentReporter>()
    .await
    .expect("frozen-lockfile install should succeed");

    let state = load_workspace_state(dirs.path())
        .expect("read workspace state")
        .expect("workspace state file exists after install");

    assert!(
        state.last_validated_timestamp > 0,
        "lastValidatedTimestamp should be populated, got {}",
        state.last_validated_timestamp,
    );
    let manifest_mtime = crate::optimistic_repeat_install::file_mtime(manifest.path())
        .expect("manifest should have an mtime");
    assert!(
        !crate::optimistic_repeat_install::modified_at_or_after(
            manifest_mtime,
            state.last_validated_timestamp,
        ),
        "fresh install state should cover the validated manifest mtime",
    );

    // The state must record the project that pacquet just installed
    // so pnpm's `allProjects.length !== Object.keys(projects).length`
    // check passes. Single-project install → exactly one entry, keyed
    // on the workspace dirs.dir.
    assert_eq!(state.projects.len(), 1);
    let project_key = dirs.path().to_string_lossy().into_owned();
    let project = state
        .projects
        .get(&project_key)
        .unwrap_or_else(|| panic!("project entry for {project_key:?} should exist"));
    assert_eq!(
        project,
        &workspace_state::ProjectEntry {
            // `PackageManifest::create_if_needed` seeds `name` from the
            // parent dirs.dir's basename and `version` from `"1.0.0"`. The
            // test pins the round-trip of both fields so a regression
            // that loses them (e.g. switching to a non-string serde
            // shape) trips here.
            name: Some(
                dirs.path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .expect("tmpdir has a UTF-8 basename")
                    .to_string()
            ),
            version: Some("1.0.0".to_string()),
        },
    );

    assert!(!state.filtered_install);
    assert!(state.pnpmfiles.is_empty());

    let settings = &state.settings;
    assert_eq!(settings.node_linker, Some(WorkspaceStateNodeLinker::Isolated));
    assert_eq!(settings.dev, Some(false));
    assert_eq!(settings.optional, Some(true));
    assert_eq!(settings.production, Some(true));
    assert_eq!(settings.auto_install_peers, Some(true));
    assert_eq!(settings.dedupe_peer_dependents, Some(true));
    assert_eq!(settings.dedupe_peers, Some(false));
    assert_eq!(settings.prefer_workspace_packages, Some(false));
    assert_eq!(settings.hoist_workspace_packages, Some(true));
    assert_eq!(settings.hoist_pattern.as_deref(), Some(&["*".to_string()][..]));

    drop(dirs.dir);
}
#[test]
fn filtered_modules_metadata_preserves_only_retained_unselected_entries() {
    let package_key = |name: &str, version: &str| {
        pnpm_lockfile::PackageKey::new(
            pnpm_lockfile::PkgName::parse(name).unwrap(),
            version.parse::<pnpm_lockfile::PkgVerPeer>().unwrap(),
        )
    };
    let retained = "retained@1.0.0";
    let selected = "selected@2.0.0";
    let shared = "shared@1.0.0";
    let stale_selected = "selected@1.0.0";
    let retained_file = package_key("retained-file", "file:packages/retained-source");
    let selected_file = package_key("selected-file", "file:packages/selected-source");
    let shared_file = package_key("shared-file", "file:packages/shared-source");
    let current = Lockfile {
        snapshots: Some(std::collections::HashMap::from([
            (package_key("retained", "1.0.0"), Default::default()),
            (package_key("selected", "2.0.0"), Default::default()),
            (package_key("shared", "1.0.0"), Default::default()),
            (retained_file, Default::default()),
            (selected_file.clone(), Default::default()),
            (shared_file.clone(), Default::default()),
        ])),
        ..empty_test_lockfile()
    };
    let selected_current = Lockfile {
        snapshots: Some(std::collections::HashMap::from([
            (package_key("selected", "2.0.0"), Default::default()),
            (package_key("shared", "1.0.0"), Default::default()),
            (selected_file, Default::default()),
            (shared_file, Default::default()),
        ])),
        ..empty_test_lockfile()
    };
    let previous = Modules {
        hoisted_dependencies: indexmap::IndexMap::from([
            (
                retained.to_string(),
                indexmap::IndexMap::from([(
                    "retained-alias".to_string(),
                    pnpm_modules_yaml::HoistKind::Private,
                )]),
            ),
            (
                shared.to_string(),
                indexmap::IndexMap::from([(
                    "stale-shared-alias".to_string(),
                    pnpm_modules_yaml::HoistKind::Private,
                )]),
            ),
            (
                stale_selected.to_string(),
                indexmap::IndexMap::from([(
                    "stale-selected-alias".to_string(),
                    pnpm_modules_yaml::HoistKind::Private,
                )]),
            ),
        ]),
        hoisted_locations: Some(std::collections::BTreeMap::from([
            (retained.to_string(), vec!["retained/location".to_string()]),
            (shared.to_string(), vec!["stale/shared/location".to_string()]),
        ])),
        pending_builds: vec![retained.to_string(), shared.to_string(), stale_selected.to_string()],
        ignored_builds: Some(
            [retained, shared, stale_selected]
                .into_iter()
                .map(|value| pnpm_modules_yaml::DepPath::from(value.to_string()))
                .collect(),
        ),
        skipped: vec![retained.to_string(), shared.to_string(), stale_selected.to_string()],
        injected_deps: Some(std::collections::BTreeMap::from([
            ("packages/retained-source".to_string(), vec!["retained/target".to_string()]),
            ("packages/selected-source".to_string(), vec!["stale/selected/target".to_string()]),
            ("packages/shared-source".to_string(), vec!["stale/shared/target".to_string()]),
        ])),
        ..Default::default()
    };
    let mut next = Modules {
        hoisted_dependencies: indexmap::IndexMap::from([(
            selected.to_string(),
            indexmap::IndexMap::from([(
                "selected-alias".to_string(),
                pnpm_modules_yaml::HoistKind::Private,
            )]),
        )]),
        hoisted_locations: Some(std::collections::BTreeMap::from([(
            selected.to_string(),
            vec!["selected/location".to_string()],
        )])),
        pending_builds: vec![selected.to_string()],
        ignored_builds: Some(
            std::iter::once(pnpm_modules_yaml::DepPath::from(selected.to_string())).collect(),
        ),
        skipped: vec![selected.to_string()],
        injected_deps: Some(std::collections::BTreeMap::from([
            ("packages/selected-source".to_string(), vec!["selected/target".to_string()]),
            ("packages/shared-source".to_string(), vec!["shared/target".to_string()]),
        ])),
        ..Default::default()
    };

    super::super::merge_filtered_modules_metadata(
        &mut next,
        &previous,
        &current,
        &selected_current,
    );

    assert!(next.hoisted_dependencies.contains_key(retained));
    assert!(next.hoisted_dependencies.contains_key(selected));
    assert!(!next.hoisted_dependencies.contains_key(shared));
    assert!(!next.hoisted_dependencies.contains_key(stale_selected));
    assert_eq!(
        next.hoisted_locations.as_ref().unwrap(),
        &std::collections::BTreeMap::from([
            (retained.to_string(), vec!["retained/location".to_string()]),
            (selected.to_string(), vec!["selected/location".to_string()]),
        ]),
    );
    assert_eq!(next.pending_builds, [retained.to_string(), selected.to_string()]);
    assert_eq!(
        next.ignored_builds
            .as_ref()
            .unwrap()
            .iter()
            .map(pnpm_modules_yaml::DepPath::as_str)
            .collect::<Vec<_>>(),
        [retained, selected],
    );
    assert_eq!(next.skipped, [retained.to_string(), selected.to_string()]);
    assert_eq!(
        next.injected_deps.as_ref().unwrap(),
        &std::collections::BTreeMap::from([
            ("packages/retained-source".to_string(), vec!["retained/target".to_string()],),
            ("packages/selected-source".to_string(), vec!["selected/target".to_string()],),
            ("packages/shared-source".to_string(), vec!["shared/target".to_string()],),
        ]),
    );
}
#[test]
fn filtered_modules_metadata_keeps_empty_optional_maps_omitted() {
    let current = empty_test_lockfile();
    let previous = Modules {
        hoisted_locations: Some(std::collections::BTreeMap::from([(
            "stale@1.0.0".to_string(),
            vec!["stale/location".to_string()],
        )])),
        ignored_builds: Some(
            std::iter::once(pnpm_modules_yaml::DepPath::from("stale@1.0.0".to_string())).collect(),
        ),
        injected_deps: Some(std::collections::BTreeMap::from([(
            "packages/stale".to_string(),
            vec!["stale/target".to_string()],
        )])),
        ..Default::default()
    };
    let mut next = Modules::default();

    super::super::merge_filtered_modules_metadata(&mut next, &previous, &current, &current);

    assert!(next.hoisted_locations.is_none());
    assert!(next.ignored_builds.is_none());
    assert!(next.injected_deps.is_none());
}
/// Round-trip the optimistic short-circuit end-to-end on a real
/// single-project install (no `pnpm-workspace.yaml`):
///
/// 1. First [`Install::run`] resolves through the registry mock,
///    writes `pnpm-lock.yaml` next to the manifest, lays out
///    `node_modules`, and records `.pnpm-workspace-state-v1.json`.
/// 2. A second [`Install::run`] against the same manifest must hit
///    the optimistic fast path — emit `Already up to date` and skip
///    every install-setup event (`pnpm:context`, `pnpm:stage`,
///    `pnpm:lockfile-verification`).
///
/// Proves the single-project lockfile gate added in this commit
/// doesn't break the warm-reinstall fast path it's intended to
/// preserve. Companion to
/// [`optimistic_repeat_install_does_not_short_circuit_when_lockfile_missing`]
/// (which covers the negative direction).
#[tokio::test]
async fn optimistic_repeat_install_round_trips_on_single_project_install() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    std::fs::create_dir_all(&dirs.project_root).expect("create project root");
    let manifest_path = dirs.project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    config.registry = mock_instance.url();
    let config = config.leak();

    // First install: fresh-resolve path. Writes `pnpm-lock.yaml` next
    // to the manifest (via `install_with_fresh_lockfile`) and the
    // workspace state next to `node_modules` (via
    // `Install::run`'s end-of-run `update_workspace_state` call).
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

    // Sanity check the first install left both artifacts the
    // optimistic check keys off on disk.
    assert!(
        dirs.project_root.join("pnpm-lock.yaml").exists(),
        "first install must write pnpm-lock.yaml next to the manifest",
    );
    assert!(
        load_workspace_state(&dirs.project_root).expect("read workspace state").is_some(),
        "first install must record .pnpm-workspace-state-v1.json",
    );

    // Now run the second install against the same manifest. Capture
    // events to prove the fast path fired.
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
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        // Don't pass the in-memory lockfile — the optimistic check
        // doesn't need it, and we want to prove the fast path runs
        // *before* the lockfile is even loaded. (Matching pnpm's
        // dispatch ordering: `checkDepsStatus` runs before any
        // lockfile parse.)
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
    .expect("second install must succeed via the optimistic short-circuit");

    let captured = EVENTS.lock().unwrap();
    assert!(
        captured.iter().any(|event| matches!(
            event,
            LogEvent::Pnpm(log) if log.message == "Already up to date"
        )),
        "second install must emit `Already up to date`; got events: {captured:#?}",
    );

    // The fast path runs before any of the install setup, so none
    // of these events should fire on the second install.
    let install_emits = captured
        .iter()
        .filter(|event| {
            matches!(
                event,
                LogEvent::Context(_) | LogEvent::Stage(_) | LogEvent::LockfileVerification(_),
            )
        })
        .count();
    assert_eq!(
        install_emits, 0,
        "the second install must not run any install-setup steps; got events: {captured:#?}",
    );

    drop((dirs.dir, mock_instance));
}
// The `readPackage` hook rewrites the project's OWN dependency range,
// and both the resolution and the importer entry the lockfile records
// follow the hook rather than the on-disk `package.json`. Declaring
// `^100.0.0` would resolve to 100.1.0; the hook pins it to 100.0.0.
#[tokio::test]
async fn read_package_hook_rewrites_the_project_own_specifier() {
    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();

    install_with_pnpmfile(
        registry.url(),
        dir.path(),
        &[("@pnpm.e2e/pkg-with-1-dep", "^100.0.0")],
        r"module.exports = { hooks: { readPackage (pkg) {
  if (pkg.dependencies && pkg.dependencies['@pnpm.e2e/pkg-with-1-dep']) {
    pkg.dependencies['@pnpm.e2e/pkg-with-1-dep'] = '100.0.0';
  }
  return pkg;
} } }",
    )
    .await
    .expect("install should succeed");

    let content =
        std::fs::read_to_string(dir.path().join(Lockfile::FILE_NAME)).expect("read pnpm-lock.yaml");
    let lockfile: Lockfile = serde_saphyr::from_str(&content).expect("parse pnpm-lock.yaml");
    let root_deps = lockfile
        .root_project()
        .expect("root importer recorded")
        .dependencies
        .as_ref()
        .expect("dependencies map");
    let key = pnpm_lockfile::PkgName::parse("@pnpm.e2e/pkg-with-1-dep").unwrap();
    let recorded = root_deps.get(&key).expect("pkg-with-1-dep recorded at root");
    assert_eq!(recorded.specifier, "100.0.0");
    assert_eq!(recorded.version.to_string(), "100.0.0");

    drop((dir, registry));
}
// Declaring `^100.0.0` would resolve to 100.1.0; the hook pins it to
// 100.0.0, this time on a workspace member rather than the root.
#[tokio::test]
async fn read_package_hook_rewrites_a_workspace_member_own_specifier() {
    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();
    let pnpmfile = r"module.exports = { hooks: { readPackage (pkg) {
  if (pkg.dependencies && pkg.dependencies['@pnpm.e2e/pkg-with-1-dep']) {
    pkg.dependencies['@pnpm.e2e/pkg-with-1-dep'] = '100.0.0';
  }
  return pkg;
} } }";

    install_workspace_member_with_pnpmfile(
        registry.url(),
        dir.path(),
        &[("@pnpm.e2e/pkg-with-1-dep", "^100.0.0")],
        pnpmfile,
    )
    .await
    .expect("install should succeed");

    let key = pnpm_lockfile::PkgName::parse("@pnpm.e2e/pkg-with-1-dep").unwrap();
    let member_dependency = |lockfile: &Lockfile| {
        lockfile.importers["packages/member"].dependencies.as_ref().expect("member dependencies")
            [&key]
            .clone()
    };
    let read_lockfile = || {
        let content = std::fs::read_to_string(dir.path().join(Lockfile::FILE_NAME))
            .expect("read pnpm-lock.yaml");
        serde_saphyr::from_str::<Lockfile>(&content).expect("parse pnpm-lock.yaml")
    };

    let recorded = member_dependency(&read_lockfile());
    assert_eq!(recorded.specifier, "100.0.0");
    assert_eq!(recorded.version.to_string(), "100.0.0");

    install_workspace_member_with_pnpmfile(
        registry.url(),
        dir.path(),
        &[("@pnpm.e2e/pkg-with-1-dep", "^100.0.0")],
        pnpmfile,
    )
    .await
    .expect("the repeat install must not see the raw range as drift");
    assert_eq!(
        member_dependency(&read_lockfile()),
        recorded,
        "the repeat install rewrites nothing",
    );

    drop((dir, registry));
}
#[test]
fn workspace_packages_map_prefers_the_dependency_manifest() {
    let root_dir = std::path::PathBuf::from("/ws/component");
    let manifest_path = root_dir.join("package.json");
    let importer_view = PackageManifest::from_value(
        manifest_path.clone(),
        serde_json::json!({ "name": "component", "version": "1.2.3" }),
    );
    let dependency_view = PackageManifest::from_value(
        manifest_path,
        serde_json::json!({
            "name": "component",
            "version": "1.2.3",
            "dependencies": { "sibling": "workspace:*" },
        }),
    );
    let projects = [pnpm_workspace::Project {
        root_dir,
        manifest: importer_view,
        dependency_manifest: Some(dependency_view),
    }];

    let map = crate::build_workspace_packages_map(Some(&projects)).expect("map for projects");
    let package = map.get("component").and_then(|by_version| by_version.get("1.2.3")).unwrap();
    assert_eq!(
        package.manifest.get("dependencies"),
        Some(&serde_json::json!({ "sibling": "workspace:*" })),
    );
}
