use super::{
    super::{Install, ProjectMutation},
    InstallDirs, PARTIAL_INSTALL_LOCKFILE, scoped_package_body,
    seed_placeholder_virtual_store_slot,
};
use crate::PolicyExcludes;
use pipe_trait::Pipe;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::{Host, NodeLinker, read_modules_manifest};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, Reporter, SilentReporter};
use pnpm_testing_utils::registry::TestRegistry;
use std::sync::Mutex;
use tempfile::tempdir;
use text_block_macros::text_block;

#[tokio::test]
async fn lockfile_only_routes_scoped_packages_to_configured_scoped_registry() {
    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    std::fs::create_dir_all(&project_root).unwrap();

    let manifest_path = project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("@private/foo", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut default_registry = mockito::Server::new_async().await;
    let default_packument = default_registry
        .mock("GET", "/@private%2Ffoo")
        .with_status(500)
        .expect(0)
        .create_async()
        .await;

    let mut scoped_registry = mockito::Server::new_async().await;
    let scoped_registry_url = format!("{}/", scoped_registry.url());
    let scoped_packument = scoped_registry
        .mock("GET", "/@private%2Ffoo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(scoped_package_body(&scoped_registry_url))
        .expect_at_least(1)
        .create_async()
        .await;

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.registry = format!("{}/", default_registry.url());
    config.registries_by_scope.insert("@private".to_string(), scoped_registry_url);
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
        lockfile_only: true,
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
    .expect("lockfile-only install should resolve scoped package through scoped registry");

    default_packument.assert_async().await;
    scoped_packument.assert_async().await;

    drop(dir);
}
/// Section B of pnpm/pacquet#433: a snapshot whose wiring and
/// integrity match the current lockfile *and* whose virtual-store
/// slot exists on disk is dropped from the install graph entirely.
/// We prove this by pointing the lockfile at a bogus tarball URL —
/// any code path that reaches the fetch site would fail, so a
/// successful install demonstrates the skip path took over.
#[tokio::test]
pub(super) async fn warm_reinstall_skips_snapshot_when_current_lockfile_matches() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // Manifest must match `PARTIAL_INSTALL_LOCKFILE` — the freshness
    // check (<https://github.com/pnpm/pacquet/issues/447>) rejects any drift between the on-disk manifest and
    // the lockfile importer entry.
    manifest.add_dependency("placeholder", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    // Opt out of the (now-default) global virtual store: the
    // `seed_placeholder_virtual_store_slot` helper writes the legacy
    // `<dirs.virtual_store_dir>/<flat-name>` shape, which only matches the
    // skip-probe path when `VirtualStoreLayout` is in legacy mode.
    // The partial-install behaviour under test (skip when the
    // current lockfile matches + slot exists) is independent of the
    // GVS layout; the GVS-on equivalent is exercised by the
    // `frozen_lockfile_under_gvs_*` tests below.
    config.enable_global_virtual_store = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

    // Pre-seed the previous-install state: write the current lockfile
    // identical to the wanted lockfile, and materialize the virtual-
    // store slot the skip check stats against.
    std::fs::create_dir_all(&dirs.virtual_store_dir).unwrap();
    lockfile
        .save_current_to_virtual_store_dir(&dirs.virtual_store_dir)
        .expect("seed current lockfile");
    seed_placeholder_virtual_store_slot(&dirs.virtual_store_dir);

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
    .run::<SilentReporter>()
    .await
    .expect(
        "skip path must short-circuit the fetch for the placeholder snapshot \
         (bogus integrity + URL would otherwise fail the install)",
    );

    // `lock.yaml` survives the install — the end-of-install write
    // persists the wanted lockfile back to disk.
    let written = Lockfile::load_current_from_virtual_store_dir(&dirs.virtual_store_dir)
        .expect("read written current lockfile")
        .expect("current lockfile should be written");
    assert_eq!(written.snapshots.as_ref().map(std::collections::HashMap::len), Some(1));

    drop(dirs.dir);
}
/// Section A + D of pnpm/pacquet#433: a second install observes
/// `pnpm:context.currentLockfileExists: true` once the first install
/// has written `<virtual_store_dir>/lock.yaml`. Drives the read site
/// (`Install::run` → `load_current_from_virtual_store_dir`) on real
/// disk state produced by the matching write site.
#[tokio::test]
pub(super) async fn context_log_reflects_current_lockfile_after_first_install() {
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
    // Manifest must match the fixture lockfile below — the freshness
    // check (<https://github.com/pnpm/pacquet/issues/447>) rejects any drift between the on-disk manifest and
    // the lockfile importer entry.
    manifest.add_dependency("placeholder", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    // Non-empty lockfile with no snapshots: the root importer lists
    // one dependency so `Lockfile::is_empty` returns `false` (and
    // the end-of-install write persists the file rather than
    // deleting it), but the empty `snapshots:` map means
    // `CreateVirtualStore::run` has no fetches to attempt. The
    // dangling symlink that `SymlinkDirectDependencies` creates is
    // fine — `link_direct_dep_bins` swallows `NotFound` on the
    // target's `package.json`. This keeps the test off the mock
    // registry while still driving the read-after-write loop.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      placeholder:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages: {}"
        "snapshots: {}"
    })
    .expect("parse minimal v9 lockfile");
    assert!(!lockfile.is_empty(), "fixture must be non-empty so the write path persists it");

    // First install: `lock.yaml` does not exist yet.
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
    .expect("first install should succeed");

    let first_context = EVENTS
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            LogEvent::Context(c) => Some(c.clone()),
            _ => None,
        })
        .expect("first install emitted a context event");
    assert!(!first_context.current_lockfile_exists);

    // The first install must have persisted the lockfile under the
    // virtual store. If `save_current_to_virtual_store_dir` regressed
    // for non-empty lockfiles, this check fails — and so does the
    // false→true assertion below, which is the whole point of pinning
    // the read-after-write loop.
    let lock_yaml = dirs.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
    assert!(
        lock_yaml.is_file(),
        "non-empty wanted lockfile must be persisted under <dirs.virtual_store_dir>/lock.yaml; found nothing at {lock_yaml:?}",
    );

    // Second install: identical inputs. The skip filter has nothing
    // to skip (no snapshots), but the read-after-write loop still
    // fires `current_lockfile_exists: true` because the first
    // install's `lock.yaml` is now on disk.
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
    .expect("second install should succeed");

    let second_context = EVENTS
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            LogEvent::Context(c) => Some(c.clone()),
            _ => None,
        })
        .expect("second install emitted a context event");
    assert!(
        second_context.current_lockfile_exists,
        "context.currentLockfileExists must flip to true once lock.yaml is on disk",
    );

    drop(dirs.dir);
}
/// Wiring proof for the new `nodeLinker: hoisted` install branch
/// (umbrella [#438] slice 6). Empty lockfile drives the cheapest
/// successful install path:
///
/// 1. `Install::run` dispatches into `InstallFrozenLockfile::run`.
/// 2. `is_hoisted` flips on, the slot-creation in
///    [`crate::CreateVirtualStore`] is skipped, and the
///    [`crate::SymlinkDirectDependencies`] +
///    [`crate::LinkVirtualStoreBins`] passes are bypassed.
/// 3. [`crate::lockfile_to_hoisted_dep_graph`] returns an empty
///    walker result against the empty `snapshots:` map.
/// 4. [`crate::link_hoisted_modules()`] is called with an empty
///    graph (no-op).
/// 5. `BuildModules` is skipped under hoisted (slice 7 retargets
///    it onto `hoistedLocations`).
/// 6. `.modules.yaml` is written with `nodeLinker: hoisted` and
///    `hoisted_locations: None` (the field is dropped when empty
///    so an isolated install never produces a hoisted-only key).
///
/// The empty-lockfile shape exercises every branch on `is_hoisted`
/// without needing a real package fetch — proving the wiring
/// composes with the existing pipeline phases. End-to-end coverage
/// against the registry-mock with a real package is left to a
/// follow-up CLI integration test.
///
/// [#438]: https://github.com/pnpm/pacquet/issues/438
#[tokio::test]
async fn hoisted_node_linker_empty_lockfile_writes_modules_yaml() {
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
    .expect("hoisted-linker install with empty lockfile should succeed");

    let written = dirs
        .modules_dir
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read .modules.yaml")
        .expect("modules manifest exists");

    assert_eq!(written.node_linker, Some(NodeLinker::Hoisted));
    // Empty walker output → no hoisted_locations to persist. The
    // field is `None`-when-empty so the isolated linker doesn't
    // accidentally write a stale `hoistedLocations: {}` key.
    assert!(
        written.hoisted_locations.is_none(),
        "empty lockfile produces no hoisted_locations: {:?}",
        written.hoisted_locations,
    );
    assert!(
        written.hoisted_dependencies.is_empty(),
        "hoisted-linker leaves hoisted_dependencies empty (no isolated-mode adapter shape): {:?}",
        written.hoisted_dependencies,
    );

    drop(dirs.dir);
}
/// Disk-side wire format: a fresh-install lockfile is valid YAML
/// loadable back into [`Lockfile`] and round-trips byte-stable
/// through `serialize_yaml`. Functions as a tripwire on
/// `dependencies_graph_to_lockfile` accidentally producing a value
/// that doesn't survive serialization (a regression the unit tests
/// don't catch because they assert on the in-memory shape).
#[tokio::test]
async fn fresh_install_lockfile_round_trips_through_load_save_load() {
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
    let first = std::fs::read_to_string(&lockfile_path).expect("read first");
    let parsed: Lockfile = serde_saphyr::from_str(&first).expect("parse first");
    let second_path = dirs.path().join("pnpm-lock.round-trip.yaml");
    parsed.save_to_path(&second_path).expect("save round-trip lockfile");
    let second = std::fs::read_to_string(&second_path).expect("read second");
    let reparsed: Lockfile = serde_saphyr::from_str(&second).expect("parse second");

    assert_eq!(parsed, reparsed, "lockfile round-trip must preserve every field");

    drop((dirs.dir, mock_instance));
}
/// `config.lockfile = false` opt-out skips the lockfile write but
/// keeps the install running (lockfile is ignored when
/// `lockfile = false`): no `pnpm-lock.yaml` on disk, but
/// `node_modules/` materialized.
#[tokio::test]
async fn fresh_install_with_lockfile_disabled_does_not_write_a_lockfile() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
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
    assert!(
        !lockfile_path.exists(),
        "config.lockfile = false must suppress the write (file should not exist)",
    );
    // Sanity: materialization still happened.
    assert!(
        dirs.project_root.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "node_modules must still be populated even when the lockfile is skipped",
    );

    drop((dirs.dir, mock_instance));
}
/// A fresh install also writes `<virtual_store_dir>/lock.yaml` so the
/// next install's slot-skip optimization has something to diff
/// against. The current-lockfile write runs at the tail of the
/// install pipeline. The contents round-trip
/// through `Lockfile`, so a subsequent `pacquet install
/// --frozen-lockfile` can read it back without a parse error.
#[tokio::test]
async fn fresh_install_also_writes_current_lockfile_under_virtual_store() {
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

    let current_lockfile_path = dirs.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
    assert!(
        current_lockfile_path.is_file(),
        "current-lockfile must be written under the virtual store dirs.dir",
    );

    let content = std::fs::read_to_string(&current_lockfile_path).expect("read current lockfile");
    let current_lockfile: Lockfile =
        serde_saphyr::from_str(&content).expect("parse current lockfile");
    assert_eq!(current_lockfile.lockfile_version.major, 9);
    let importer = current_lockfile.root_project().expect("root importer");
    let key = pnpm_lockfile::PkgName::parse("@pnpm.e2e/hello-world-js-bin").unwrap();
    assert!(
        importer.dependencies.as_ref().is_some_and(|deps| deps.contains_key(&key)),
        "current-lockfile reflects the resolved direct dep",
    );

    // The wanted-lockfile and the current-lockfile describe the same
    // resolved graph in the fresh-install path (no install-time skip
    // set to filter against), so the two files should parse to the
    // same shape.
    let wanted_path = dirs.path().join(Lockfile::FILE_NAME);
    let wanted_content = std::fs::read_to_string(&wanted_path).expect("read wanted lockfile");
    let wanted_lockfile: Lockfile =
        serde_saphyr::from_str(&wanted_content).expect("parse wanted lockfile");
    assert_eq!(
        wanted_lockfile, current_lockfile,
        "wanted and current lockfiles must match in the fresh-install path",
    );

    drop((dirs.dir, mock_instance));
}
/// `config.lockfile = false` opts out of *both* lockfile writes (the
/// wanted `pnpm-lock.yaml` and the per-virtual-store `lock.yaml`) —
/// the `useLockfile` setting is all-or-nothing.
#[tokio::test]
async fn fresh_install_with_lockfile_disabled_skips_current_lockfile_too() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
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

    assert!(
        !dirs.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME).exists(),
        "current-lockfile must also be skipped when config.lockfile = false",
    );

    drop((dirs.dir, mock_instance));
}
