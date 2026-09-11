use super::{
    super::{Install, ProjectMutation},
    InstallDirs,
};
use crate::PolicyExcludes;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::SilentReporter;
use pnpm_testing_utils::{fs::is_symlink_or_junction, registry::TestRegistry};
use std::fs;
use text_block_macros::text_block;

/// `Install` with `UpdateSeedPolicy::DropAll` is the update path
/// programmatic callers reach through napi `install({ update: true })`,
/// separate from the `update` subcommand's `Update` type — so it needs
/// its own coverage that `DropAll` drops the lockfile pin rather than
/// reusing it.
#[tokio::test]
async fn install_with_drop_all_seed_policy_bumps_dependency_within_range() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();
    fs::create_dir_all(&dirs.project_root).unwrap();

    let manifest_path = dirs.project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // Pin 100.0.0 exactly so the first install writes that version, even
    // though 100.1.0 exists and satisfies the widened range used below.
    manifest
        .add_dependency("@pnpm.e2e/dep-of-pkg-with-1-dep", "100.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = true;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    config.registry = mock_instance.url();
    let config = config.leak();

    let lockfile_path = dirs.project_root.join("pnpm-lock.yaml");

    // Pass 1: a plain install pins 100.0.0.
    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        lockfile_path: Some(&lockfile_path),
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
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
    .expect("first install should succeed");
    assert!(
        dirs.virtual_store_dir.join("@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0").exists(),
        "the pinned install should materialize 100.0.0",
    );

    // Widen the range and reload the lockfile (which now pins 100.0.0).
    manifest
        .add_dependency("@pnpm.e2e/dep-of-pkg-with-1-dep", "^100.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();
    let lockfile = Lockfile::load_current_from_virtual_store_dir(&dirs.virtual_store_dir)
        .expect("read the written current lockfile")
        .expect("a lockfile should have been written");

    // Pass 2: install with `DropAll` (the `update: true` seed policy) must
    // drop the 100.0.0 pin and re-resolve to the highest in-range 100.1.0.
    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        lockfile_path: Some(&lockfile_path),
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
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
        update_seed_policy: crate::UpdateSeedPolicy::drop_all(),
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
    .expect("update install should succeed");
    assert!(
        dirs.virtual_store_dir.join("@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0").exists(),
        "install with DropAll should bump the dependency to the highest in-range version",
    );

    drop((dirs.dir, mock_instance));
}
/// Regression for pnpm/pnpm#11934: `peerDependenciesMeta` must be
/// preserved end-to-end so optional peers are not auto-installed.
#[tokio::test]
async fn auto_install_peers_does_not_cascade_optional_peers() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/abc-optional-peers", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
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
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
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
    .expect("install with optional peers should succeed");

    let virtual_store_slots: Vec<String> = std::fs::read_dir(&dirs.virtual_store_dir)
        .expect("read virtual store dirs.dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();

    assert!(
        virtual_store_slots.iter().any(|name| name.starts_with("@pnpm.e2e+peer-a@")),
        "required peer `peer-a` must be auto-installed under \
         `autoInstallPeers: true`; virtual-store slots: {virtual_store_slots:?}",
    );

    for optional_peer in ["peer-b", "peer-c"] {
        let slot_prefix = format!("@pnpm.e2e+{optional_peer}@");
        let cascaded: Vec<&String> =
            virtual_store_slots.iter().filter(|name| name.starts_with(&slot_prefix)).collect();
        assert!(
            cascaded.is_empty(),
            "optional peer `{optional_peer}` must NOT reach the virtual store; \
             found {cascaded:?}",
        );
    }

    assert!(
        virtual_store_slots
            .iter()
            .any(|name| name.starts_with("@pnpm.e2e+abc-optional-peers@1.0.0")),
        "abc-optional-peers must reach the virtual store; \
         virtual-store slots: {virtual_store_slots:?}",
    );
    assert!(
        is_symlink_or_junction(
            &dirs.project_root.join("node_modules/@pnpm.e2e/abc-optional-peers")
        )
        .unwrap(),
        "abc-optional-peers must be symlinked at the importer level",
    );

    drop((dirs.dir, mock_instance));
}
/// Companion to [`auto_install_peers_does_not_cascade_optional_peers`]:
/// `@pnpm.e2e/abc-optional-peers-meta-only@1.0.0` declares `peer-b` and
/// `peer-c` **only** through `peerDependenciesMeta`, with no matching
/// `peerDependencies` entry. Such entries are treated as optional
/// peers with implicit range `*`; the install must still keep them out
/// of the tree when no other consumer requests them.
///
/// Scenario: a warning is not reported when an optional peer
/// dependency (specified by meta field only) cannot be resolved.
#[tokio::test]
async fn meta_only_optional_peers_absent_from_the_graph_are_not_installed() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/abc-optional-peers-meta-only", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
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
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
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
    .expect("install with meta-only optional peers should succeed");

    let virtual_store_slots: Vec<String> = std::fs::read_dir(&dirs.virtual_store_dir)
        .expect("read virtual store dirs.dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();

    // peer-a is declared in `peerDependencies` so it stays required and
    // gets auto-installed; peer-b and peer-c are implied optional peers
    // (declared *only* in `peerDependenciesMeta`) — an optional peer is
    // never fetched, only deduped onto a version already in the graph,
    // and no version of either is in this graph.
    assert!(
        virtual_store_slots.iter().any(|name| name.starts_with("@pnpm.e2e+peer-a@")),
        "required peer `peer-a` must be auto-installed; \
         virtual-store slots: {virtual_store_slots:?}",
    );
    for optional_peer in ["peer-b", "peer-c"] {
        let slot_prefix = format!("@pnpm.e2e+{optional_peer}@");
        let cascaded: Vec<&String> =
            virtual_store_slots.iter().filter(|name| name.starts_with(&slot_prefix)).collect();
        assert!(
            cascaded.is_empty(),
            "meta-only optional peer `{optional_peer}` must NOT reach the virtual store; \
             found {cascaded:?}",
        );
    }

    drop((dirs.dir, mock_instance));
}
/// Mirror of the TS test "a root dependency does not override the
/// peers provided inside a self-contained subtree"
/// (`deps-installer/test/install/autoInstallPeers.ts`).
///
/// `@pnpm.e2e/closure-plugins` provides every peer of its own subtree
/// (`closure-lib-a` and `closure-lib-b` peer-depend on each other and
/// on `closure-peer-x`, and all three are its regular dependencies).
/// The root project additionally depends on the incompatible
/// `closure-peer-x@2.0.0`. With `autoInstallPeers` (the default), the
/// peers resolved inside the subtree are attached to the root project
/// for reuse — but they must not be peer-resolved again in the root
/// context, where `closure-peer-x@2.0.0` is the nearest provider, or
/// the subtree's peer graph gets a mix of both versions.
#[tokio::test]
async fn root_dependency_does_not_override_peers_of_self_contained_subtree() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest.add_dependency("@pnpm.e2e/closure-plugins", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.add_dependency("@pnpm.e2e/closure-peer-x", "2.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
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
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
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

    let content = std::fs::read_to_string(dirs.path().join(Lockfile::FILE_NAME))
        .expect("read pnpm-lock.yaml");
    assert!(
        !content.contains("(@pnpm.e2e/closure-peer-x@2.0.0)"),
        "no peer inside the self-contained subtree may bind to the root's \
         closure-peer-x@2.0.0; lockfile:\n{content}",
    );
    assert!(
        content.contains("(@pnpm.e2e/closure-peer-x@1.0.0)"),
        "the subtree's peers must bind to its own closure-peer-x@1.0.0; lockfile:\n{content}",
    );

    // The root keeps its own explicitly declared version.
    let lockfile: Lockfile = serde_saphyr::from_str(&content).expect("parse pnpm-lock.yaml");
    let root_deps = lockfile
        .root_project()
        .expect("root importer recorded")
        .dependencies
        .as_ref()
        .expect("dependencies map");
    let peer_x_key = pnpm_lockfile::PkgName::parse("@pnpm.e2e/closure-peer-x").unwrap();
    let root_peer_x = root_deps.get(&peer_x_key).expect("closure-peer-x recorded at root");
    assert_eq!(root_peer_x.version.to_string(), "2.0.0");

    drop((dirs.dir, mock_instance));
}
/// Specifiers recorded into each importer-level entry mirror the
/// user-written `package.json` value, not the resolved version:
/// a `^1.0.0` declared spec round-trips through the lockfile as
/// `specifier: ^1.0.0` even when the resolved version is exact
/// (`1.0.0`).
#[tokio::test]
async fn fresh_install_records_user_written_specifier() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "^1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
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
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
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

    let importer = lockfile.root_project().expect("root importer");
    let deps = importer.dependencies.as_ref().expect("prod deps");
    let key = pnpm_lockfile::PkgName::parse("@pnpm.e2e/hello-world-js-bin").unwrap();
    let entry = deps.get(&key).expect("hello-world-js-bin entry");
    assert_eq!(entry.specifier, "^1.0.0", "specifier must echo the manifest declaration");

    drop((dirs.dir, mock_instance));
}
#[tokio::test]
async fn test_install_resolve_only_ignores_layout_mismatch() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.project_root.join("package.json");
    std::fs::create_dir_all(&dirs.project_root).unwrap();
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config_isolated = Config::new();
    config_isolated.lockfile = false;
    config_isolated.store_dir = dirs.store_dir.clone().into();
    config_isolated.modules_dir = dirs.modules_dir.clone();
    config_isolated.virtual_store_dir = dirs.virtual_store_dir.clone();

    let mut config_hoisted = Config::new();
    config_hoisted.lockfile = false;
    config_hoisted.store_dir = dirs.store_dir.clone().into();
    config_hoisted.modules_dir = dirs.modules_dir.clone();
    config_hoisted.virtual_store_dir = dirs.virtual_store_dir.clone();
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

    let canary_path = dirs.modules_dir.join("canary.txt");
    std::fs::create_dir_all(&dirs.modules_dir).unwrap();
    std::fs::write(&canary_path, "canary").unwrap();
    assert!(canary_path.exists());

    // 2nd install: Hoisted node linker, but dry_run is true
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
        dry_run: true,
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

    // Canary should still exist because dry_run doesn't mutate node_modules
    assert!(canary_path.exists(), "canary was deleted despite dry_run: true");
}
