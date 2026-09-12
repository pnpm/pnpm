use super::{
    super::{Install, ProjectMutation},
    InstallDirs,
};
use crate::{AllowBuildPolicy, PolicyExcludes, VirtualStoreLayout};
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::{Host, read_modules_manifest};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, Reporter, SilentReporter};
use pnpm_store_dir::STORE_VERSION;
use pnpm_testing_utils::{fs::is_symlink_or_junction, registry::TestRegistry};
use std::sync::Mutex;
use tempfile::tempdir;
use text_block_macros::text_block;

#[tokio::test]
async fn fresh_partial_install_preserves_optional_link_in_warm_gvs_slot() {
    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let peer_root = dir.path().join("peer-c");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    std::fs::create_dir_all(&project_root).expect("create project root");
    std::fs::create_dir_all(&peer_root).expect("create peer root");
    std::fs::write(
        peer_root.join("package.json"),
        serde_json::json!({ "name": "@pnpm.e2e/peer-c", "version": "1.0.0" }).to_string(),
    )
    .expect("write peer manifest");
    let manifest_path = project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/abc-optional-peers", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.add_dependency("@pnpm.e2e/peer-c", "link:../peer-c", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.enable_global_virtual_store = true;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.global_virtual_store_dir = config.store_dir.links();
    config.registry = registry.url();
    let config = config.leak();
    let http_client = std::sync::Arc::new(pnpm_network::ThrottledClient::default());

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &http_client,
        http_client_arc: std::sync::Arc::clone(&http_client),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: Some(false),
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
        disable_optimistic_repeat_install: true,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("full install should materialize the optional peer link");

    let lockfile = Lockfile::load_wanted_from_dir(&project_root)
        .expect("load wanted lockfile")
        .expect("wanted lockfile exists");
    let snapshot_key = lockfile
        .snapshots
        .as_ref()
        .expect("snapshots exist")
        .keys()
        .find(|key| key.name.to_string() == "@pnpm.e2e/abc-optional-peers")
        .expect("optional-peers snapshot")
        .clone();
    let allow_build_policy = AllowBuildPolicy::default();
    let layout = VirtualStoreLayout::new(
        config,
        None,
        lockfile.snapshots.as_ref(),
        lockfile.packages.as_ref(),
        Some(&allow_build_policy),
        Some(&project_root),
    );
    let linked_peer = layout.slot_dir(&snapshot_key).join("node_modules").join("@pnpm.e2e/peer-c");
    assert!(
        is_symlink_or_junction(&linked_peer).unwrap(),
        "full install must create the optional peer link at {linked_peer:?}",
    );

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &http_client,
        http_client_arc: std::sync::Arc::clone(&http_client),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: Some(false),
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        mutation: ProjectMutation::InstallSome,
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
        disable_optimistic_repeat_install: true,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("partial fresh install should succeed");

    assert!(
        is_symlink_or_junction(&linked_peer).unwrap(),
        "partial install without the optional direct group must preserve {linked_peer:?}",
    );

    drop((dir, registry));
}
/// Under GVS, the `virtualStoreDir` value pacquet persists in
/// `.modules.yaml` must equal the path pnpm writes — i.e.
/// `<storeDir>/v11/links` — not the project-local `node_modules/.pnpm`
/// path pacquet keeps internally in [`Config::virtual_store_dir`]. If
/// they diverge, the next `pnpm install` reads the manifest, recomputes
/// `ctx.virtualStoreDir` from the GVS-on path, and trips its
/// `checkCompatibility` with `ERR_PNPM_UNEXPECTED_VIRTUAL_STORE_DIR`
/// for every project, forcing the "modules directories will be
/// reinstalled from scratch" prompt on every invocation. The same
/// value is also emitted on the `pnpm:context` channel that
/// `@pnpm/cli.default-reporter` parses, so the same parity rule
/// applies there.
#[tokio::test]
async fn gvs_persists_global_virtual_store_dir_in_modules_yaml_and_context_log() {
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
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    // GVS on — the whole point of the test.
    config.enable_global_virtual_store = true;
    config.lockfile = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    // Keep `dirs.virtual_store_dir` at the project-local path. Pacquet's
    // internal layout consumers still read this field; the parity
    // requirement is only that the externally-observed value (the one
    // pnpm sees in `.modules.yaml` / `pnpm:context`) routes through
    // `global_virtual_store_dir` via `effective_virtual_store_dir`.
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    // Source the GVS root from `dirs.store_dir.links()` so the assertion
    // below targets the same v11-suffixed path the
    // [`From<PathBuf> for StoreDir`] impl produces in production. Hard-
    // coding `dirs.store_dir.join("links")` here would drop the `v11`
    // segment and turn the test into a tautology.
    config.global_virtual_store_dir = config.store_dir.links();
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
    .run::<RecordingReporter>()
    .await
    .expect("frozen-lockfile install under GVS should succeed");

    // The path pnpm would have written. `StoreDir::from` appends the
    // [`STORE_VERSION`] suffix to the configured root, so the live
    // value is `<dirs.store_dir>/v11/links` even though the test handed
    // `Config::dirs.store_dir` the un-suffixed root.
    let expected_resolved = dirs.store_dir.join(STORE_VERSION).join("links");

    // Ensure the GVS root exists on disk so `dunce::canonicalize` can
    // resolve it. An empty-lockfile install doesn't link anything into
    // `<dirs.store_dir>/v11/links/`, so the dirs.dir would otherwise be absent.
    std::fs::create_dir_all(&expected_resolved)
        .expect("create GVS links dirs.dir for canonicalize");
    let expected_canonical =
        dunce::canonicalize(&expected_resolved).expect("canonicalize GVS links dirs.dir");

    // `.modules.yaml` is what `pnpm install` reads on the *next*
    // invocation; this is the round-trip pnpm's `checkCompatibility`
    // sees. `read_modules_manifest` normalises the stored relative
    // path back to absolute against `dirs.modules_dir`, so a successful
    // assertion proves both halves: pacquet wrote the GVS path, and
    // the relative form on disk re-resolves to it. Canonicalize the
    // result because `read_modules_manifest`'s
    // `dirs.modules_dir.join(relative)` keeps `..` segments verbatim, while
    // pnpm's `path.relative(modules.virtualStoreDir, opts.virtualStoreDir)`
    // check reduces them before comparing.
    let read_back = read_modules_manifest::<Host>(&dirs.modules_dir)
        .expect("read .modules.yaml")
        .expect("present");
    assert_eq!(
        dunce::canonicalize(&read_back.virtual_store_dir)
            .expect("canonicalize read-back virtualStoreDir"),
        expected_canonical,
        "modules.yaml virtualStoreDir must round-trip to <storeDir>/{STORE_VERSION}/links under GVS",
    );

    // The `pnpm:context` event the default reporter prints in the
    // install header. Same parity rule, different channel — pnpm
    // emits `ctx.virtualStoreDir` (the GVS-mutated value); pacquet
    // must too.
    let captured = EVENTS.lock().unwrap();
    let context_log = captured
        .iter()
        .find_map(|event| match event {
            LogEvent::Context(log) => Some(log),
            _ => None,
        })
        .expect("install emits exactly one pnpm:context event");
    assert_eq!(
        dunce::canonicalize(&context_log.virtual_store_dir)
            .expect("canonicalize context virtualStoreDir"),
        expected_canonical,
        "pnpm:context virtualStoreDir must report the GVS path, matching pnpm's default reporter",
    );

    drop(dirs.dir);
}
