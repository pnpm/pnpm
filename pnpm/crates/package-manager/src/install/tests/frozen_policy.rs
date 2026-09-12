use super::{
    super::{Install, InstallError, ProjectMutation},
    InstallDirs,
};
use crate::PolicyExcludes;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::{Host, LayoutVersion, Modules, NodeLinker, write_modules_manifest};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, Reporter, SilentReporter};
use pnpm_workspace_state as workspace_state;
use std::sync::Mutex;
use tempfile::tempdir;
use text_block_macros::text_block;

/// `--frozen-lockfile` disables the optimistic short-circuit because
/// a headless install must always fail loudly on a missing or stale
/// lockfile (matching pnpm's `installDeps` not calling
/// `checkDepsStatus` in that mode). The install proceeds through the
/// regular dispatch and the existing `frozen_install_short_circuits...`
/// no-op path still fires.
#[tokio::test]
async fn frozen_lockfile_disables_optimistic_short_circuit() {
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
    let manifest_path = dirs.project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("sibling", "link:../sibling", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

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
        "    dependencies:"
        "      sibling:"
        "        specifier: link:../sibling"
        "        version: link:../sibling"
        "packages: {}"
        "snapshots: {}"
    })
    .expect("parse lockfile");

    let included = pnpm_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };

    // Seed the same state the optimistic test uses, so the only
    // difference between the two is `frozen_lockfile`.
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
    lockfile
        .save_current_to_virtual_store_dir(&dirs.virtual_store_dir)
        .expect("seed current lockfile");

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
        // The only difference vs the optimistic test above.
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: true,
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
    .await
    .expect("frozen install must still succeed via the legacy no-op path");

    let captured = EVENTS.lock().unwrap();
    assert!(
        !captured.iter().any(|event| matches!(
            event,
            LogEvent::Pnpm(log) if log.message == "Already up to date"
        )),
        "the optimistic 'Already up to date' log MUST NOT fire under --frozen-lockfile; got events: {captured:#?}",
    );
    // The existing no-op short-circuit still does fire on the frozen
    // path (the previous `frozen_install_short_circuits...` test
    // covers that emit), and downstream code asserts on its
    // presence; we only assert here that the *optimistic* log is
    // absent so the polarity of the gate is clear.
}
/// `packageExtensions` drift between the lockfile-recorded checksum
/// and the freshly-computed value from `Config::package_extensions`
/// surfaces as `OutdatedLockfile` with a
/// `StalenessReason::PackageExtensionsChecksumChanged` payload under
/// `--frozen-lockfile`.
#[tokio::test]
async fn frozen_lockfile_errors_when_package_extensions_drift_from_lockfile() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    // Config declares an extension the lockfile doesn't carry → drift.
    let mut deps = std::collections::BTreeMap::new();
    deps.insert("dep-a".to_string(), "1.0.0".to_string());
    let mut extensions = indexmap::IndexMap::new();
    extensions.insert(
        "foo".to_string(),
        pnpm_config::PackageExtension { dependencies: Some(deps), ..Default::default() },
    );
    config.package_extensions = Some(extensions);
    let config = config.leak();

    // Lockfile fixture has *no* `packageExtensionsChecksum` key.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .: {}"
    })
    .expect("parse minimal lockfile");

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
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
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

    let err = result.expect_err("packageExtensions drift must surface as a config mismatch");
    match err {
        InstallError::LockfileConfigMismatch { setting: "packageExtensionsChecksum" } => {}
        other => {
            panic!("expected LockfileConfigMismatch for `packageExtensionsChecksum`, got {other:?}")
        }
    }

    drop(dirs.dir);
}
#[tokio::test]
async fn frozen_lockfile_errors_when_pnpmfile_checksum_drifts() {
    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    std::fs::create_dir_all(&project_root).unwrap();
    let manifest = PackageManifest::create_if_needed(project_root.join("package.json")).unwrap();

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .: {}"
    })
    .expect("parse minimal lockfile");

    std::fs::write(
        project_root.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { readPackage: pkg => pkg } }\n",
    )
    .unwrap();

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
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
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

    assert!(matches!(
        result,
        Err(InstallError::LockfileConfigMismatch { setting: "pnpmfileChecksum" })
    ));
}
