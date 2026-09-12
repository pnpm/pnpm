use super::{
    super::{Install, InstallError, ProjectMutation},
    InstallDirs, PARTIAL_INSTALL_LOCKFILE,
};
use crate::PolicyExcludes;
use pipe_trait::Pipe;
use pnpm_config::{Config, NodePackageMapType};
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::{Host, LayoutVersion, Modules, NodeLinker, read_modules_manifest};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{
    LogEvent, PackageManifestLog, PackageManifestMessage, ProgressLog, ProgressMessage, Reporter,
    SilentReporter, Stage, StageLog, StatsLog, StatsMessage,
};
use pnpm_store_dir::STORE_VERSION;
use pnpm_testing_utils::{
    fs::{get_all_folders, is_symlink_or_junction},
    registry::TestRegistry,
};
use std::sync::Mutex;
use tempfile::tempdir;
use text_block_macros::text_block;

#[test]
fn package_map_writer_is_gated_to_supported_pacquet_mode() {
    let mut config = Config::new();
    assert!(!crate::should_write_package_map(&config, pnpm_config::NodeLinker::Isolated));
    assert!(!crate::should_write_hoisted_package_map(&config));

    config.node_experimental_package_map = true;
    assert!(crate::should_write_package_map(&config, pnpm_config::NodeLinker::Isolated));
    assert!(!crate::should_write_package_map(&config, pnpm_config::NodeLinker::Hoisted));
    assert!(!crate::should_write_package_map(&config, pnpm_config::NodeLinker::Pnp));
    // The hoisted linker writes the map from its own writer, so it
    // answers the setting through its own predicate.
    assert!(crate::should_write_hoisted_package_map(&config));

    // `virtualStoreOnly` writes no `node_modules` to put a map in.
    config.virtual_store_only = true;
    assert!(!crate::should_write_package_map(&config, pnpm_config::NodeLinker::Isolated));
    assert!(!crate::should_write_hoisted_package_map(&config));

    config.virtual_store_only = false;
    config.node_package_map_type = NodePackageMapType::Loose;
    assert!(crate::should_write_package_map(&config, pnpm_config::NodeLinker::Isolated));
}
#[tokio::test]
async fn should_install_dependencies() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(event.clone());
        }
    }

    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules"); // TODO: we shouldn't have to define this
    let virtual_store_dir = modules_dir.join(".pacquet"); // TODO: we shouldn't have to define this

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();

    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.add_dependency("@pnpm/xyz", "1.0.0", DependencyGroup::Dev).unwrap();

    manifest.save().unwrap();

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
    .run::<RecordingReporter>()
    .await
    .expect("install should succeed");

    let captured = EVENTS.lock().unwrap();
    let expected_manifest = manifest.value();
    let manifest_indices: Vec<_> = captured
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            matches!(
                event,
                LogEvent::PackageManifest(PackageManifestLog {
                    message: PackageManifestMessage::Initial { initial, .. },
                    ..
                }) if initial == expected_manifest,
            )
            .then_some(index)
        })
        .collect();
    assert_eq!(
        manifest_indices.len(),
        1,
        "install must report the input package manifest exactly once; events={captured:#?}",
    );
    assert!(
        captured.iter().any(|event| matches!(
            event,
            LogEvent::Stats(StatsLog {
                message: StatsMessage::Added { added, .. },
                ..
            }) if *added > 0
        )),
        "install must report a positive added count; events={captured:#?}",
    );
    assert!(
        captured.iter().any(|event| matches!(
            event,
            LogEvent::Stats(StatsLog { message: StatsMessage::Removed { removed: 0, .. }, .. })
        )),
        "install must report that it removed no packages; events={captured:#?}",
    );
    let importing_done_indices: Vec<_> = captured
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            matches!(event, LogEvent::Stage(StageLog { stage: Stage::ImportingDone, .. }))
                .then_some(index)
        })
        .collect();
    assert_eq!(
        importing_done_indices.len(),
        1,
        "install must close the importing stage exactly once; events={captured:#?}",
    );
    let resolved_indices: Vec<_> = captured
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            matches!(
                event,
                LogEvent::Progress(ProgressLog {
                    message: ProgressMessage::Resolved { package_id, .. },
                    ..
                }) if package_id == "@pnpm.e2e/hello-world-js-bin@1.0.0",
            )
            .then_some(index)
        })
        .collect();
    assert_eq!(
        resolved_indices.len(),
        1,
        "install must report the resolved direct dependency exactly once; events={captured:#?}",
    );
    assert!(
        manifest_indices[0] < resolved_indices[0]
            && resolved_indices[0] < importing_done_indices[0],
        "install events must report the manifest, resolve the dependency, then close importing; events={captured:#?}",
    );
    drop(captured);

    let path = project_root.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    eprintln!("path={path:?} symlink_or_junction={:?}", is_symlink_or_junction(&path));
    assert!(is_symlink_or_junction(&path).unwrap());
    let path = project_root.join("node_modules/.pacquet/@pnpm.e2e+hello-world-js-bin@1.0.0");
    eprintln!("path={path:?} exists={}", path.exists());
    assert!(path.exists());
    let path = project_root.join("node_modules/@pnpm/xyz");
    eprintln!("path={path:?} symlink_or_junction={:?}", is_symlink_or_junction(&path));
    assert!(is_symlink_or_junction(&path).unwrap());
    // `@pnpm/xyz@1.0.0` has peer dependencies on `@pnpm/x`, `@pnpm/y`,
    // and `@pnpm/z`, so the resolver produces a peer-suffixed
    // depPath and the layout lands the slot at
    // `@pnpm+xyz@1.0.0_@pnpm+x@1.0.0_@pnpm+y@1.0.0_@pnpm+z@1.0.0` —
    // matching the snapshot key shape `pnpm install` would write
    // to `pnpm-lock.yaml` and the slot the frozen-lockfile
    // path materialises into.
    let path = project_root
        .join("node_modules/.pacquet/@pnpm+xyz@1.0.0_@pnpm+x@1.0.0_@pnpm+y@1.0.0_@pnpm+z@1.0.0");
    eprintln!("path={path:?} is_dir={}", path.is_dir());
    assert!(path.is_dir());

    insta::assert_debug_snapshot!(get_all_folders(&project_root));

    drop((dir, mock_instance));
}
/// A first install (no prior `.modules.yaml`, so the prune throttle
/// always fires) sweeps a surplus `.pacquet` directory the wanted
/// lockfile doesn't reference, while keeping the slot it does. Drives
/// the [`crate::prune_virtual_store`] wiring through the real install
/// path, exercising the surplus-cleanup behavior.
#[tokio::test]
async fn install_prunes_surplus_virtual_store_dir() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    // Seed a surplus virtual-store directory that no lockfile entry
    // references. The install must sweep it.
    let surplus = dirs.virtual_store_dir.join("surplus-pkg@9.9.9");
    std::fs::create_dir_all(&surplus).unwrap();

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
        dirs.virtual_store_dir.join("@pnpm.e2e+hello-world-js-bin@1.0.0").exists(),
        "the installed package's virtual-store dirs.dir must survive the prune",
    );
    assert!(!surplus.exists(), "the surplus virtual-store dirs.dir must be pruned on install");

    drop((dirs.dir, mock_instance));
}
/// Issue [#312](https://github.com/pnpm/pacquet/issues/312): an npm-alias dependency
/// (`"<key>": "npm:<real>@<range>"`) used to panic during install
/// because the whole `npm:...` spec was fed to
/// `node_semver::Range::parse`. Assert that:
///
/// * the install completes,
/// * the virtual-store directory uses the *real* package name, and
/// * the symlink under `node_modules/` uses the alias key.
#[tokio::test]
async fn npm_alias_dependency_installs_under_alias_key() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();

    manifest
        .add_dependency(
            "hello-world-alias",
            "npm:@pnpm.e2e/hello-world-js-bin@1.0.0",
            DependencyGroup::Prod,
        )
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
    .expect("npm-alias install should succeed");

    let alias_link = dirs.project_root.join("node_modules/hello-world-alias");
    assert!(
        is_symlink_or_junction(&alias_link).unwrap(),
        "expected alias symlink at {alias_link:?}",
    );
    assert!(
        !dirs.project_root.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "the real package name must not be exposed alongside an unrelated alias",
    );

    let virtual_store_path =
        dirs.project_root.join("node_modules/.pacquet/@pnpm.e2e+hello-world-js-bin@1.0.0");
    assert!(virtual_store_path.is_dir(), "expected real-name virtual store dirs.dir");
    assert!(virtual_store_path.join("node_modules/@pnpm.e2e/hello-world-js-bin").is_dir());

    drop((dirs.dir, mock_instance));
}
/// Issue [#312], unversioned variant: `"foo": "npm:bar"` (no `@<range>`)
/// must default to `latest` without panicking. `resolve_registry_dependency`
/// turns `"npm:bar"` into `("bar", "latest")`; the previous code then
/// fed `"latest"` to `package.pinned_version()` which panics because
/// `node_semver::Range` cannot parse the string. The fix is to route
/// `"latest"` (and any `PackageTag`-parseable value) through
/// `PackageVersion::fetch_from_registry` directly.
///
/// We use the same scoped test package as the pinned-version test above
/// but omit the `@1.0.0` suffix to trigger the default-to-`latest` path.
///
/// [#312]: https://github.com/pnpm/pacquet/issues/312
#[tokio::test]
async fn unversioned_npm_alias_defaults_to_latest() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();

    // No `@<version>` — should resolve to the `latest` tag.
    manifest
        .add_dependency(
            "hello-world-alias",
            "npm:@pnpm.e2e/hello-world-js-bin",
            DependencyGroup::Prod,
        )
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
    .expect("unversioned npm-alias install should succeed (defaults to latest)");

    let alias_link = dirs.project_root.join("node_modules/hello-world-alias");
    assert!(
        is_symlink_or_junction(&alias_link).unwrap(),
        "expected alias symlink at {alias_link:?}",
    );
    assert!(
        !dirs.project_root.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "the real package name must not be exposed alongside the alias",
    );

    // Virtual-store directory uses the real package name (version resolved
    // at runtime from `latest` — just assert the real name prefix exists).
    let virtual_store_dir_path = dirs.project_root.join("node_modules/.pacquet");
    let has_real_name_dir =
        std::fs::read_dir(&virtual_store_dir_path).unwrap().flatten().any(|entry| {
            entry.file_name().to_string_lossy().starts_with("@pnpm.e2e+hello-world-js-bin@")
        });
    assert!(has_real_name_dir, "expected real-name virtual store directory");

    drop((dirs.dir, mock_instance));
}
/// A successful install must persist `<modules_dir>/.modules.yaml`.
/// Asserts the on-disk fields a follow-up install (or third-
/// party tool) keys off: `layoutVersion`, `nodeLinker`, the
/// `included` set derived from the dispatched dependency groups, the
/// store and virtual-store directories, and the `default` registry.
#[tokio::test]
pub(super) async fn install_writes_modules_yaml() {
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

    // Empty v9 lockfile drives the cheapest successful install path,
    // which is enough to prove `.modules.yaml` is written on success.
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
        // Drive a non-default `included`: prod + optional, no dev,
        // so the assertion below pins the mapping of dispatched
        // groups to the on-disk `included` field.
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

    let Modules {
        layout_version,
        node_linker,
        included,
        store_dir: emitted_store_dir,
        virtual_store_dir: emitted_virtual_store_dir,
        virtual_store_dir_max_length,
        package_manager,
        ..
    } = dirs
        .modules_dir
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read .modules.yaml")
        .expect("modules manifest exists");

    assert_eq!(layout_version, Some(LayoutVersion));
    assert_eq!(node_linker, Some(NodeLinker::Isolated));
    assert!(included.dependencies);
    assert!(!included.dev_dependencies);
    assert!(included.optional_dependencies);
    assert_eq!(emitted_store_dir, dirs.store_dir.join(STORE_VERSION).display().to_string());
    // `read_modules_manifest` resolves `virtualStoreDir` against
    // `dirs.modules_dir`, so a relative on-disk value round-trips back
    // to the absolute install-time path.
    assert_eq!(emitted_virtual_store_dir, dirs.virtual_store_dir.to_string_lossy());
    assert_eq!(virtual_store_dir_max_length, pnpm_config::default_virtual_store_dir_max_length());
    assert_eq!(package_manager, format!("pnpm@{}", pnpm_config::PNPM_VERSION));

    drop(dirs.dir);
}
/// Scenario: do not fail on an optional dependency that has a
/// non-optional dependency with a failing postinstall script.
///
/// Resolves `@pnpm.e2e/has-failing-postinstall-dep@1.0.0` as an
/// optional dependency through the live registry-mock instance. The
/// transitive `@pnpm.e2e/failing-postinstall@1.0.0` has a
/// `postinstall` that exits non-zero. Pacquet's
/// `frozen_lockfile=false` path stops at extraction (script execution
/// lives behind `BuildModules` in the frozen-lockfile branch —
/// `BuildModules` itself is unit-tested against the same fixture in
/// `crate::build_modules::tests::do_not_fail_on_optional_dep_with_failing_postinstall`).
/// This test pins the fetch + extract behavior on the optional edge:
/// both packages must land in the virtual store and the install must
/// NOT abort.
#[tokio::test]
async fn install_optional_failing_postinstall_dep_via_registry_mock_succeeds() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/has-failing-postinstall-dep", "1.0.0", DependencyGroup::Optional)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    config.registry = mock_instance.url();
    // Allow the transitive `failing-postinstall` build to actually run so
    // the optional-failure tolerance is exercised (an ignored build would
    // instead trip `strictDepBuilds`, which is unrelated to this test).
    config.dangerously_allow_all_builds = true;
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
    .expect("optional dep with failing transitive postinstall must NOT abort the install");

    // Both the wrapper and the transitive must reach the virtual store.
    assert!(
        is_symlink_or_junction(
            &dirs.project_root.join("node_modules/@pnpm.e2e/has-failing-postinstall-dep"),
        )
        .unwrap(),
        "wrapper symlink missing",
    );
    assert!(
        dirs.project_root
            .join("node_modules/.pacquet/@pnpm.e2e+has-failing-postinstall-dep@1.0.0")
            .is_dir(),
        "wrapper virtual-store dirs.dir missing",
    );
    assert!(
        dirs.project_root
            .join("node_modules/.pacquet/@pnpm.e2e+failing-postinstall@1.0.0")
            .is_dir(),
        "transitive `failing-postinstall` must be extracted to the virtual store",
    );

    drop((dirs.dir, mock_instance));
}
/// `--ignore-manifest-check` (`Install::ignore_manifest_check = true`)
/// bypasses the [`satisfies_package_manifest`] gate, so an install
/// whose manifest has drifted from the lockfile proceeds past the
/// freshness check. Same setup as
/// [`frozen_lockfile_errors_when_manifest_drifts_from_lockfile`], just
/// with the flag flipped: we now expect the install to reach the
/// fetch site and fail there (network / integrity error against the
/// bogus tarball URL) rather than abort early with `OutdatedLockfile`.
#[tokio::test]
async fn ignore_manifest_check_bypasses_manifest_freshness_gate() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    // Deliberately leave the `placeholder` dep out — same drift the
    // sibling test exercises. With `ignore_manifest_check: true` the
    // install must accept the drift and move on to materialization.
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
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
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: true,
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

    let err = result.expect_err("bogus tarball URL must still surface a downstream error");
    assert!(
        !matches!(err, InstallError::OutdatedLockfile { .. }),
        "ignore_manifest_check should bypass the freshness gate, got OutdatedLockfile: {err:?}",
    );

    drop(dirs.dir);
}
