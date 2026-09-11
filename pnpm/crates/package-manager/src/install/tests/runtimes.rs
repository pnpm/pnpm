use super::{
    super::{Install, ProjectMutation},
    InstallDirs, is_modules_yaml_consistent, run_purge_regression_install,
};
use crate::PolicyExcludes;
use pipe_trait::Pipe;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::{
    DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH, Host, LayoutVersion, Modules, NodeLinker,
    read_modules_manifest, write_modules_manifest,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::SilentReporter;
use pnpm_testing_utils::registry::TestRegistry;
use std::fs;
use tempfile::tempdir;
use text_block_macros::text_block;

/// The prune deletes directories under `virtual_store_dir`, which can be
/// set by repo-controlled workspace config. When that path escapes the
/// project's `node_modules` (here, a sibling directory), the sweep is
/// refused so a malicious config can't redirect destructive deletes
/// outside the managed tree. Regression guard for the path-containment
/// check in [`crate::prune_virtual_store::prune_target_within_modules`].
#[tokio::test]
async fn install_skips_prune_when_virtual_store_escapes_node_modules() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    // The virtual store is pointed *outside* node_modules, as a malicious
    // workspace config could do.
    let virtual_store_dir = dir.path().join("escaped-store");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    // A surplus entry the wanted lockfile doesn't reference. Because the
    // store escapes node_modules, the sweep must leave it untouched.
    let surplus = virtual_store_dir.join("surplus-pkg@9.9.9");
    std::fs::create_dir_all(&surplus).unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
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
    .expect("install should succeed");

    assert!(
        surplus.exists(),
        "prune must be skipped when the virtual store is outside node_modules",
    );

    drop((dir, mock_instance));
}
/// Hoisted install must NOT create the virtual-store slot
/// directories the isolated linker would write — that's the whole
/// point of skipping [`crate::CreateVirtualDirBySnapshot`] under
/// hoisted. With no snapshots in the lockfile the assertion is
/// vacuous (the directory is empty regardless of linker choice),
/// but pinning the absence here documents the contract so a
/// future regression that re-enables slot writes under hoisted
/// surfaces immediately.
///
/// `node_modules/.pacquet/` not being present is the proof: the
/// virtual-store root only gets created on demand by
/// [`CreateVirtualDirBySnapshot::run`]; under hoisted that helper
/// is never called, so the directory is never materialized.
#[tokio::test]
async fn hoisted_node_linker_does_not_create_virtual_store_root() {
    let dirs = InstallDirs::new();

    std::fs::create_dir_all(&dirs.project_root).expect("create project root");
    let manifest_path = dirs.project_root.join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

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
        node_linker: pnpm_config::NodeLinker::Hoisted,
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
    .expect("hoisted-linker install should succeed");

    // `<project>/node_modules/.pacquet` only gets created when
    // CreateVirtualDirBySnapshot lays down a slot. Hoisted skips
    // that helper, so the dirs.dir must remain absent.
    assert!(
        !dirs.virtual_store_dir.exists(),
        "hoisted install must not materialize the virtual-store root at {:?}",
        dirs.virtual_store_dir,
    );

    drop(dirs.dir);
}
/// `nodeLinker: hoisted` on the fresh-lockfile path (no lockfile,
/// not frozen) installs successfully and records the hoisted linker
/// in `.modules.yaml`. With an empty manifest there is nothing to
/// materialize, so the assertion focuses on the dispatch reaching
/// the hoisted-linker pipeline rather than bailing.
#[tokio::test]
async fn fresh_install_hoisted_node_linker_records_modules_yaml() {
    let dirs = InstallDirs::new();

    std::fs::create_dir_all(&dirs.project_root).expect("create project root");
    let manifest_path = dirs.project_root.join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
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
        node_linker: pnpm_config::NodeLinker::Hoisted,
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
    .expect("fresh hoisted-linker install should succeed");

    let written = dirs
        .modules_dir
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read .modules.yaml")
        .expect("modules manifest exists");

    assert_eq!(written.node_linker, Some(NodeLinker::Hoisted));
    // Empty manifest → no packages, so the hoisted linker records no
    // locations. The field is `None`-when-empty so a stale
    // `hoistedLocations: {}` key isn't written.
    assert!(
        written.hoisted_locations.is_none(),
        "empty manifest produces no hoisted_locations: {:?}",
        written.hoisted_locations,
    );
    // Hoisted skips the virtual store entirely.
    assert!(
        !dirs.virtual_store_dir.exists(),
        "hoisted install must not materialize the virtual-store root at {:?}",
        dirs.virtual_store_dir,
    );

    drop(dirs.dir);
}
/// A fresh install must not be refused for carrying `--no-runtime`
/// (`config.skip_runtimes = true`); the runtime-skipping behavior
/// itself is covered by the `install_runtimes` integration tests.
#[tokio::test]
async fn fresh_install_honors_skip_runtimes() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let result = Install {
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
        skip_runtimes: true,
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

    result.expect("fresh install with skip_runtimes should succeed");
    let _ = dirs.virtual_store_dir;
    assert!(dirs.modules_dir.join(".modules.yaml").exists(), "modules manifest written");

    drop(dirs.dir);
}
/// `nodeLinker` drift between `.modules.yaml` and the current config
/// disqualifies the up-to-date short-circuit — a different linker
/// forces a full rebuild of `node_modules` rather than a fast no-op.
#[test]
fn is_modules_yaml_consistent_returns_false_when_node_linker_drifts() {
    let dir = tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    let seed = Modules {
        layout_version: Some(LayoutVersion),
        node_linker: Some(NodeLinker::Hoisted),
        hoist_pattern: config.hoist_pattern.clone(),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        store_dir: config.store_dir.display().to_string(),
        virtual_store_dir: config.effective_virtual_store_dir().to_string_lossy().into_owned(),
        virtual_store_dir_max_length: config.virtual_store_dir_max_length,
        ..Default::default()
    };
    write_modules_manifest::<Host>(&modules_dir, seed).expect("seed .modules.yaml");

    assert!(!is_modules_yaml_consistent(
        &modules_dir,
        config,
        pnpm_config::NodeLinker::Isolated,
        pnpm_modules_yaml::IncludedDependencies::default(),
    ));
}
/// Install-level regression for the purge guard: switching `--prod` <-> full
/// (an `included` drift) must keep a user's own non-pnpm entry in
/// `node_modules`, while a real layout drift still recreates the directory
/// from scratch. Without the `modules_layout_consistent_with` split, the
/// included drift would purge the directory and delete the user's file.
///
/// The skipped purge must not leave the excluded dev dep behind either:
/// the targeted prune ([`crate::prune_direct_deps_excluded_by_groups`])
/// removes its `node_modules` link and its `.bin` shim while everything
/// else stays.
#[tokio::test]
async fn included_drift_keeps_user_node_modules_entry_while_layout_drift_wipes_it() {
    let mock_instance = TestRegistry::start();
    let dirs = InstallDirs::new();

    fs::create_dir_all(&dirs.project_root).unwrap();
    let manifest_path = dirs.project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("@pnpm.e2e/console-log", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Dev).unwrap();
    manifest.save().unwrap();

    let full = || vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
    let prod_only = || vec![DependencyGroup::Prod];

    // 1. A full install creates node_modules + .modules.yaml (included = full).
    run_purge_regression_install(
        &dirs.store_dir,
        &dirs.modules_dir,
        &dirs.virtual_store_dir,
        mock_instance.url(),
        &manifest,
        full(),
        DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH,
    )
    .await;

    let prod_link = dirs.modules_dir.join("@pnpm.e2e/console-log");
    let dev_link = dirs.modules_dir.join("@pnpm.e2e/hello-world-js-bin");
    let dev_shim = dirs.modules_dir.join(".bin/hello-world-js-bin");
    assert!(dev_link.symlink_metadata().is_ok(), "full install links the dev dep");
    assert!(dev_shim.exists(), "full install shims the dev dep's bin");

    // The user drops their own non-pnpm file directly into node_modules.
    let vendored = dirs.modules_dir.join("vendored-by-user.txt");
    fs::write(&vendored, b"keep me").unwrap();

    // 2. Switching to --prod is an included drift only, so the file survives —
    // but the now-excluded dev dep's link and bin shim must be pruned.
    run_purge_regression_install(
        &dirs.store_dir,
        &dirs.modules_dir,
        &dirs.virtual_store_dir,
        mock_instance.url(),
        &manifest,
        prod_only(),
        DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH,
    )
    .await;
    assert!(vendored.exists(), "included drift must not purge the user's node_modules entry");
    assert!(
        dev_link.symlink_metadata().is_err(),
        "the excluded dev dep's link must be pruned on an included drift",
    );
    assert!(
        !dev_shim.exists(),
        "the excluded dev dep's bin shim must be pruned on an included drift",
    );
    assert!(prod_link.symlink_metadata().is_ok(), "the prod dep stays linked");

    // 3. A real layout drift (virtual-store-dirs.dir-max-length) still wipes it.
    run_purge_regression_install(
        &dirs.store_dir,
        &dirs.modules_dir,
        &dirs.virtual_store_dir,
        mock_instance.url(),
        &manifest,
        prod_only(),
        DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH - 1,
    )
    .await;
    assert!(!vendored.exists(), "layout drift must still recreate node_modules from scratch");
}
#[tokio::test]
async fn test_install_purges_node_modules_on_layout_mismatch() {
    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    // Where the default store lands — see [`pnpm_config::store_path`].
    let store_dir = modules_dir.join(".pnpm-store");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = project_root.join("package.json");
    std::fs::create_dir_all(&project_root).unwrap();
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config_isolated = Config::new();
    config_isolated.lockfile = false;
    config_isolated.store_dir = store_dir.clone().into();
    config_isolated.modules_dir = modules_dir.clone();
    config_isolated.virtual_store_dir = virtual_store_dir.clone();

    let mut config_hoisted = Config::new();
    config_hoisted.lockfile = false;
    config_hoisted.store_dir = store_dir.clone().into();
    config_hoisted.modules_dir = modules_dir.clone();
    config_hoisted.virtual_store_dir = virtual_store_dir.clone();
    config_hoisted.hoist_pattern = Some(vec![]);

    let config_isolated = config_isolated.leak();
    let config_hoisted = config_hoisted.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies: {}"
        "packages: {}"
        "snapshots: {}"
    })
    .unwrap();

    // 1st install: Isolated node linker (default)
    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config: config_isolated,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        lockfile_path: None,
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
    .run::<SilentReporter>()
    .await
    .expect("1st install success");

    let canary_path = modules_dir.join("canary.txt");
    std::fs::create_dir_all(&modules_dir).unwrap();
    std::fs::write(&canary_path, "canary").unwrap();
    assert!(canary_path.exists());

    let store_marker = store_dir.join("marker");
    std::fs::create_dir_all(&store_dir).unwrap();
    std::fs::write(&store_marker, "keep").unwrap();

    // 2nd install: Hoisted node linker
    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config: config_hoisted,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        lockfile_path: None,
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
        node_linker: pnpm_config::NodeLinker::Hoisted,
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
    .expect("2nd install success");

    assert!(!canary_path.exists(), "node_modules should be purged due to mismatch");
    assert_eq!(
        std::fs::read_to_string(&store_marker).ok().as_deref(),
        Some("keep"),
        "a store inside node_modules should survive the purge",
    );
}
