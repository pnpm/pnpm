use super::{
    super::{
        Install, InstallError, ProjectMutation, UpToDateFastPathCheck, install_already_up_to_date,
    },
    InstallDirs, assert_purge_diagnostic, fresh_lockfile_only_with_compatibility_db,
    install_then_go_offline, recorded_verified_file_integrity_report, touch_manifest,
};
use crate::PolicyExcludes;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::{Host, LayoutVersion, Modules, NodeLinker, write_modules_manifest};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, Reporter, SilentReporter};
use pnpm_store_dir::VerifiedFileIntegrity;
use pnpm_testing_utils::registry::TestRegistry;
use pnpm_workspace_state as workspace_state;
use std::{sync::Mutex, time::Duration};
use tempfile::tempdir;
use text_block_macros::text_block;

/// The `optimisticRepeatInstall` short-circuit. When nothing
/// has changed since the previous successful install, `Install::run`
/// must emit pnpm's `name: "pnpm"` "Already up to date" log and
/// return without ever calling `verify_lockfile_resolutions` or
/// reading the lockfile.
///
/// Closes pnpm/pnpm#11940.
#[tokio::test]
async fn optimistic_repeat_install_skips_entire_pipeline_when_state_is_fresh() {
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

    // Single-project optimistic-repeat-install requires `pnpm-lock.yaml`
    // on disk (a missing lockfile triggers `throwLockfileNotFound`).
    // Write a minimal v9 lockfile next to the manifest so the freshness
    // gate passes — the fast path only checks existence, not contents.
    std::fs::write(dirs.project_root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n")
        .expect("seed pnpm-lock.yaml");

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
    .expect("parse minimal v9 lockfile with one link dep");

    let included = pnpm_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };

    // Seed `.modules.yaml` and the workspace state so the optimistic
    // check sees a previous install.
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
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        // trust_lockfile=false so verification would normally run.
        // The optimistic short-circuit must beat it.
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
    .await
    .expect("install must succeed via the optimistic short-circuit");

    let captured = EVENTS.lock().unwrap();
    assert!(
        captured.iter().any(|event| matches!(
            event,
            LogEvent::Pnpm(log) if log.message == "Already up to date"
        )),
        r#"expected `name: "pnpm" / level: "info"` 'Already up to date' log; got events: {captured:#?}"#,
    );

    // The optimistic path runs before any of the install setup, so
    // none of these events should fire:
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
        "no install-setup events must fire on the optimistic short-circuit; got events: {captured:#?}",
    );
}
/// The synchronous pre-runtime twin of the short-circuit
/// ([`install_already_up_to_date`]) must reach the same verdict from
/// the same on-disk state — and flip to `None` (fall through to the
/// full install) as soon as a manifest outdates the recorded
/// validation timestamp.
#[test]
fn sync_fast_path_matches_optimistic_short_circuit() {
    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");

    std::fs::create_dir_all(&modules_dir).expect("create modules dir so the deps gate passes");
    let manifest_path = project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest.add_dependency("sibling", "link:../sibling", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    std::fs::write(project_root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n")
        .expect("seed pnpm-lock.yaml");

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    let included = pnpm_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };
    let mut projects = std::collections::BTreeMap::new();
    projects.insert(
        project_root.to_string_lossy().into_owned(),
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
        &project_root,
        &pnpm_workspace_state::WorkspaceState {
            last_validated_timestamp: pnpm_testing_utils::fs::backdate_existing_files(
                &project_root,
            ),
            projects,
            pnpmfiles: Vec::new(),
            filtered_install: false,
            config_dependencies: None,
            settings,
        },
    )
    .expect("seed workspace state");

    let check = UpToDateFastPathCheck {
        config,
        manifest: &manifest,
        dependency_groups: vec![DependencyGroup::Prod],
        node_linker: pnpm_config::NodeLinker::Isolated,
        supported_architectures: None,
    };
    let root = install_already_up_to_date(&check);
    assert_eq!(
        root.map(|up_to_date| up_to_date.root).as_deref(),
        Some(&*project_root),
        "fresh state must short-circuit",
    );

    // Outdate the manifest relative to the recorded timestamp: the
    // fast path must decline and leave the decision to the full
    // install.
    pnpm_testing_utils::fs::bump_mtime(&manifest_path);
    // The manifest content still matches no lockfile (config.lockfile
    // is off and no current lockfile exists), so the content re-check
    // cannot vouch for it either.
    assert!(install_already_up_to_date(&check).is_none(), "modified manifest must fall through");
}
/// `add` / `remove` mutate the manifest in memory and persist it only
/// after `Install::run` returns, so the on-disk mtimes the optimistic
/// check reads still describe the pre-mutation project. A partial
/// install (a `mutation` that is not a full install) must therefore never take the
/// optimistic short-circuit — otherwise a fresh workspace state would
/// read as "already up to date" and the mutation would never be
/// resolved or materialized. The dependency-status check runs only
/// for the plain-install mutation.
#[tokio::test]
async fn partial_install_disables_optimistic_short_circuit() {
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
    // difference between the two is the mutation.
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
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: true,
        update_checksums: false,
        // The only difference vs the optimistic test above.
        mutation: ProjectMutation::InstallSome,
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
    .expect("partial install must still succeed via the regular dispatch");

    let captured = EVENTS.lock().unwrap();
    assert!(
        !captured.iter().any(|event| matches!(
            event,
            LogEvent::Pnpm(log) if log.message == "Already up to date"
        )),
        "the optimistic 'Already up to date' log MUST NOT fire for a partial install; got events: {captured:#?}",
    );
}
/// A repeat install whose manifest was rewritten with identical
/// content (newer mtime) must short-circuit offline: no resolver, no
/// lockfile-verification fan-out, no install pipeline. Guards the
/// modified-manifests content re-check end-to-end through
/// `Install::run`'s dispatch ordering — the fast path has to run
/// *before* the verification gate for this to pass with a dead
/// registry and an empty packument/verdict cache.
#[tokio::test]
async fn optimistic_repeat_install_short_circuits_offline_when_touched_manifest_is_unchanged() {
    let (dir, offline_config, manifest) = install_then_go_offline().await;
    let project_root = manifest.path().parent().unwrap().to_path_buf();
    let touched_manifest = touch_manifest(&manifest);
    let lockfile_path = project_root.join(Lockfile::FILE_NAME);
    let wanted_lockfile =
        Lockfile::load_wanted_from_dir(&project_root).expect("load wanted lockfile").unwrap();

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
    .expect("repeat install with an unchanged-content manifest must not need the registry");

    let captured = EVENTS.lock().unwrap();
    assert!(
        captured.iter().any(|event| matches!(
            event,
            LogEvent::Pnpm(log) if log.message == "Already up to date"
        )),
        "the touched-but-unchanged manifest must take the fast path; got {captured:#?}",
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

    drop(dir);
}
#[tokio::test]
async fn fresh_install_applies_builtin_compatibility_db_to_dependency_manifest() {
    let (_dir, lockfile) = fresh_lockfile_only_with_compatibility_db(false).await;
    let metadata = lockfile
        .packages
        .as_ref()
        .and_then(|packages| packages.get(&"debug@4.0.0".parse().unwrap()))
        .expect("debug package metadata recorded");
    assert_eq!(
        metadata
            .peer_dependencies_meta
            .as_ref()
            .and_then(|meta| meta.get("supports-color"))
            .map(|meta| meta.optional),
        Some(true),
    );
    assert_eq!(lockfile.package_extensions_checksum, None);
}
#[tokio::test]
async fn fresh_install_skips_builtin_compatibility_db_when_ignored() {
    let (_dir, lockfile) = fresh_lockfile_only_with_compatibility_db(true).await;
    let metadata = lockfile
        .packages
        .as_ref()
        .and_then(|packages| packages.get(&"debug@4.0.0".parse().unwrap()))
        .expect("debug package metadata recorded");
    assert!(metadata.peer_dependencies_meta.is_none());
    assert_eq!(lockfile.package_extensions_checksum, None);
}
/// `packageExtensions` adds entries to a dependency's manifest at
/// resolve time and the resulting lockfile records the merged shape.
///
/// Scenario: manifests are extended with fields specified by
/// `packageExtensions`.
/// Covers both the resolution-side effect (the extension's
/// `peerDependencies` entry must land in the package's lockfile
/// metadata) and the lockfile-side `packageExtensionsChecksum` write
/// (the prefixed `sha256-…` checksum must be recorded so a subsequent
/// frozen install can detect drift via
/// [`crate::FreshnessCheckError::Stale`]).
#[tokio::test]
async fn fresh_install_applies_package_extensions_to_dependency_manifest() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    config.registry = mock_instance.url();
    // Add a `peerDependencies` entry to the resolved manifest of
    // `@pnpm.e2e/hello-world-js-bin`, marked optional so the missing
    // peer never escalates to a fetch error during this minimal test.
    let mut peers = std::collections::BTreeMap::new();
    peers.insert("synthetic-peer".to_string(), "*".to_string());
    let mut peers_meta = std::collections::BTreeMap::new();
    peers_meta.insert(
        "synthetic-peer".to_string(),
        pnpm_config::PeerDependencyMeta { optional: Some(true) },
    );
    let mut extensions = indexmap::IndexMap::new();
    extensions.insert(
        "@pnpm.e2e/hello-world-js-bin".to_string(),
        pnpm_config::PackageExtension {
            peer_dependencies: Some(peers),
            peer_dependencies_meta: Some(peers_meta),
            ..Default::default()
        },
    );
    config.package_extensions = Some(extensions);
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
    .expect("install should succeed");

    let lockfile_path = dirs.path().join(Lockfile::FILE_NAME);
    let content = std::fs::read_to_string(&lockfile_path).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&content).expect("parse fresh lockfile");

    let packages = lockfile.packages.as_ref().expect("packages map populated");
    let pkg_key: pnpm_lockfile::PackageKey = "@pnpm.e2e/hello-world-js-bin@1.0.0".parse().unwrap();
    let metadata = packages.get(&pkg_key).expect("packages entry recorded");
    let peers = metadata
        .peer_dependencies
        .as_ref()
        .expect("packageExtensions added peerDependencies must be recorded");
    assert_eq!(peers.get("synthetic-peer").map(String::as_str), Some("*"));

    // The lockfile must also carry the `packageExtensionsChecksum`
    // (sha256-prefixed) so a subsequent frozen install can detect
    // drift.
    let checksum = lockfile
        .package_extensions_checksum
        .as_deref()
        .expect("packageExtensionsChecksum must be recorded");
    assert!(
        checksum.starts_with("sha256-"),
        "checksum must use the sha256-prefixed wire shape; got {checksum:?}",
    );

    drop((dirs.dir, mock_instance));
}
/// A tie rounds up, in the direction JavaScript's `toFixed` takes it:
/// 2.25s must not render as `2.2s` here and `2.3s` in pnpm.
#[test]
fn a_tie_in_the_seconds_rounds_up() {
    let messages = recorded_verified_file_integrity_report(VerifiedFileIntegrity {
        files: 7,
        duration: Duration::from_millis(2250),
    });
    assert_eq!(messages, vec!["The integrity of 7 files was checked in 2.3s.".to_string()]);
}
#[test]
fn remove_modules_dir_names_the_entry_and_carries_the_diagnostic_code() {
    let path = std::path::PathBuf::from("project").join("node_modules").join("left-pad");
    let error = InstallError::RemoveModulesDir {
        path: path.clone(),
        error: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
    };

    assert_purge_diagnostic(&error, &path);
}
#[test]
fn read_modules_dir_names_the_directory_and_carries_the_diagnostic_code() {
    let path = std::path::PathBuf::from("project").join("node_modules");
    let error = InstallError::ReadModulesDir {
        path: path.clone(),
        error: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
    };

    assert_purge_diagnostic(&error, &path);
}
/// Paths reach both variants from a canonicalized modules directory, so
/// on Windows they carry a `\\?\` prefix that must not reach the user.
#[test]
#[cfg_attr(not(windows), ignore = "verbatim prefixes only exist on Windows")]
fn the_purge_diagnostics_render_copy_pasteable_windows_paths() {
    let io_error = || std::io::Error::from_raw_os_error(5);
    let errors = [
        InstallError::RemoveModulesDir {
            path: std::path::PathBuf::from(r"\\?\C:\project\node_modules\is-odd"),
            error: io_error(),
        },
        InstallError::ReadModulesDir {
            path: std::path::PathBuf::from(r"\\?\C:\project\node_modules"),
            error: io_error(),
        },
    ];

    for error in errors {
        let rendered = error.to_string();
        assert!(rendered.contains(r"C:\project\node_modules"), "got: {rendered}");
        assert!(!rendered.contains(r"\\?\"), "verbatim prefix must not reach the user: {rendered}");
        assert!(!rendered.contains(r"\\"), "separators must not be escaped: {rendered}");
    }
}
