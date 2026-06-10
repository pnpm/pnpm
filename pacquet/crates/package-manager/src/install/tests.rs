#![expect(
    clippy::default_trait_access,
    reason = "struct-literal test fixtures; field types are evident from the literal and naming each would force ~20 imports"
)]

use super::{Install, InstallError};
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_modules_yaml::{
    DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH, Host, LayoutVersion, Modules, NodeLinker,
    read_modules_manifest, write_modules_manifest,
};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::{
    BrokenModulesLog, ContextLog, HookLog, IgnoredScriptsLog, LogEvent, PackageManifestLog,
    PackageManifestMessage, ProgressLog, ProgressMessage, Reporter, SilentReporter, Stage,
    StageLog, StatsLog, StatsMessage, SummaryLog,
};
use pacquet_store_dir::STORE_VERSION;
use pacquet_testing_utils::{
    fs::{get_all_folders, is_symlink_or_junction},
    registry::TestRegistry,
};
use pacquet_workspace_state::{
    self as workspace_state, NodeLinker as WorkspaceStateNodeLinker, load_workspace_state,
};
use pipe_trait::Pipe;
use std::sync::Mutex;
use tempfile::tempdir;
use text_block_macros::text_block;

#[tokio::test]
async fn should_install_dependencies() {
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    // Make sure the package is installed
    let path = project_root.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    eprintln!("path={path:?} symlink_or_junction={:?}", is_symlink_or_junction(&path));
    assert!(is_symlink_or_junction(&path).unwrap());
    let path = project_root.join("node_modules/.pacquet/@pnpm.e2e+hello-world-js-bin@1.0.0");
    eprintln!("path={path:?} exists={}", path.exists());
    assert!(path.exists());
    // Make sure we install dev-dependencies as well
    let path = project_root.join("node_modules/@pnpm/xyz");
    eprintln!("path={path:?} symlink_or_junction={:?}", is_symlink_or_junction(&path));
    assert!(is_symlink_or_junction(&path).unwrap());
    // `@pnpm/xyz@1.0.0` has peer dependencies on `@pnpm/x`, `@pnpm/y`,
    // and `@pnpm/z`, so the resolver produces a peer-suffixed
    // depPath and the layout lands the slot at
    // `@pnpm+xyz@1.0.0_@pnpm+x@1.0.0_@pnpm+y@1.0.0_@pnpm+z@1.0.0` —
    // matching the snapshot key shape `pnpm install` would write
    // to `pnpm-lock.yaml` and the slot upstream's frozen-lockfile
    // path materialises into.
    let path = project_root
        .join("node_modules/.pacquet/@pnpm+xyz@1.0.0_@pnpm+x@1.0.0_@pnpm+y@1.0.0_@pnpm+z@1.0.0");
    eprintln!("path={path:?} is_dir={}", path.is_dir());
    assert!(path.is_dir());

    insta::assert_debug_snapshot!(get_all_folders(&project_root));

    drop((dir, mock_instance)); // cleanup
}

#[tokio::test]
async fn should_error_when_frozen_lockfile_is_requested_but_none_exists() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = true;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    assert!(matches!(result, Err(InstallError::NoLockfile)));
    drop(dir);
}

#[tokio::test]
async fn should_error_when_frozen_lockfile_and_update_checksums_are_both_set() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = true;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: true,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    assert!(matches!(result, Err(InstallError::FrozenLockfileWithUpdateChecksums)));
    drop(dir);
}

/// `--frozen-lockfile` passed on the CLI must take precedence over
/// `config.lockfile=false`. Before this fix the dispatch matched on
/// `(config.lockfile, frozen_lockfile, lockfile)` in an order that
/// treated `config.lockfile=false` as "skip lockfile entirely",
/// silently dropping the CLI flag and resolving from the registry
/// instead — the very regression the integrated benchmark was
/// measuring. Pin the new priority: frozen flag + lockfile present
/// → `InstallFrozenLockfile`, regardless of `config.lockfile`.
///
/// We don't need the full install to succeed here — any error that
/// *isn't* `NoLockfile` proves the dispatch picked the frozen path.
/// Passing a malformed lockfile integrity surfaces as
/// `FrozenLockfile(...)`.
#[tokio::test]
async fn frozen_lockfile_flag_overrides_config_lockfile_false() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    // Explicitly disabled — this is the pacquet default today. The
    // CLI flag must still take over.
    config.lockfile = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    // Minimal v9 lockfile with no snapshots — the frozen path will
    // run through `CreateVirtualStore` with an empty snapshot set,
    // which is a successful no-op. That's enough to prove we took
    // the frozen branch.
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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("--frozen-lockfile + empty lockfile should succeed via InstallFrozenLockfile");

    drop(dir);
}

/// Issue [#312](https://github.com/pnpm/pacquet/issues/312): an npm-alias dependency
/// (`"<key>": "npm:<real>@<range>"`) used to panic during install
/// because the whole `npm:...` spec was fed to
/// `node_semver::Range::parse`. Assert that:
///
/// * the install completes,
/// * the virtual-store directory uses the *real* package name, and
/// * the symlink under `node_modules/` uses the alias key.
///
/// Mirrors pnpm's `parseBareSpecifier`. Reference:
/// <https://github.com/pnpm/pnpm/blob/1819226b51/resolving/npm-resolver/src/parseBareSpecifier.ts>
#[tokio::test]
async fn npm_alias_dependency_installs_under_alias_key() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("npm-alias install should succeed");

    // Symlink lives under the alias key, *not* the real package name.
    let alias_link = project_root.join("node_modules/hello-world-alias");
    assert!(
        is_symlink_or_junction(&alias_link).unwrap(),
        "expected alias symlink at {alias_link:?}",
    );
    assert!(
        !project_root.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "the real package name must not be exposed alongside an unrelated alias",
    );

    // Virtual-store directory uses the real package name.
    let virtual_store_path =
        project_root.join("node_modules/.pacquet/@pnpm.e2e+hello-world-js-bin@1.0.0");
    assert!(virtual_store_path.is_dir(), "expected real-name virtual store dir");
    assert!(virtual_store_path.join("node_modules/@pnpm.e2e/hello-world-js-bin").is_dir());

    drop((dir, mock_instance));
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

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("unversioned npm-alias install should succeed (defaults to latest)");

    // Symlink lives under the alias key, not the real package name.
    let alias_link = project_root.join("node_modules/hello-world-alias");
    assert!(
        is_symlink_or_junction(&alias_link).unwrap(),
        "expected alias symlink at {alias_link:?}",
    );
    assert!(
        !project_root.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "the real package name must not be exposed alongside the alias",
    );

    // Virtual-store directory uses the real package name (version resolved
    // at runtime from `latest` — just assert the real name prefix exists).
    let virtual_store_dir_path = project_root.join("node_modules/.pacquet");
    let has_real_name_dir =
        std::fs::read_dir(&virtual_store_dir_path).unwrap().flatten().any(|entry| {
            entry.file_name().to_string_lossy().starts_with("@pnpm.e2e+hello-world-js-bin@")
        });
    assert!(has_real_name_dir, "expected real-name virtual store directory");

    drop((dir, mock_instance));
}

/// Symmetric negative: `--frozen-lockfile` with no lockfile
/// loadable must surface `NoLockfile`, even when `config.lockfile`
/// is `false` (which used to fall through to the no-lockfile path
/// and silently succeed).
#[tokio::test]
async fn frozen_lockfile_flag_with_no_lockfile_errors() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    assert!(matches!(result, Err(InstallError::NoLockfile)));
    drop(dir);
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

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<RecordingReporter>()
    .await
    .expect("empty-lockfile frozen install should succeed");

    let captured = EVENTS.lock().unwrap();

    // Event ordering matches pnpm: manifest snapshot, context,
    // importing_started, the `pnpm:stats` added/removed pair from
    // `CreateVirtualStore::run`, then `importing_done` once extraction
    // and symlink linking are complete (mirrors upstream `link.ts:167`),
    // followed by the `pnpm:ignored-scripts` summary that
    // `BuildModules::run` produces, then summary closing the run. The
    // empty snapshot map still triggers the stats emit (`added: 0`,
    // `removed: 0`), matching pnpm's unconditional emit at link time.
    // The empty lockfile produces no ignored builds, so
    // `ignored-scripts` carries an empty list.
    assert!(
        matches!(
            captured.as_slice(),
            [
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
    let LogEvent::IgnoredScripts(IgnoredScriptsLog { package_names, .. }) = &captured[6] else {
        unreachable!("ignored-scripts at index 6, asserted above");
    };
    assert!(package_names.is_empty(), "no builds in empty lockfile: {package_names:?}");

    let expected_prefix = manifest.path().parent().unwrap().to_string_lossy().into_owned();

    // Manifest event carries the on-disk JSON unchanged so consumers
    // can diff `initial` vs a later `updated` byte-for-byte.
    let LogEvent::PackageManifest(PackageManifestLog {
        message: PackageManifestMessage::Initial { prefix: manifest_prefix, initial },
        ..
    }) = &captured[0]
    else {
        unreachable!("first event is package-manifest, asserted above");
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
    }) = &captured[1]
    else {
        unreachable!("second event is context, asserted above");
    };
    assert!(!current_lockfile_exists);
    assert_eq!(emitted_store_dir, &store_dir.join(STORE_VERSION).display().to_string());
    assert_eq!(emitted_virtual_store_dir, &virtual_store_dir.to_string_lossy().into_owned());

    // Summary's `prefix` must equal the manifest-parent value
    // `Install::run` derives, since pnpm's reporter keys its
    // accumulated root-events by prefix to render the diff.
    let LogEvent::Summary(SummaryLog { prefix: summary_prefix, .. }) = captured.last().unwrap()
    else {
        unreachable!("last event is summary, asserted above");
    };
    assert_eq!(summary_prefix, &expected_prefix);

    drop(dir);
}

/// A successful install must persist `<modules_dir>/.modules.yaml`,
/// matching pnpm's
/// [`writeModulesManifest`](https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/deps-installer/src/install/index.ts#L1608-L1630)
/// call. Asserts the on-disk fields a follow-up install (or third-
/// party tool) keys off: `layoutVersion`, `nodeLinker`, the
/// `included` set derived from the dispatched dependency groups, the
/// store and virtual-store directories, and the `default` registry.
#[tokio::test]
async fn install_writes_modules_yaml() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
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
        lockfile: Some(&lockfile),
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
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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
        registries,
        package_manager,
        ..
    } = modules_dir
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read .modules.yaml")
        .expect("modules manifest exists");

    assert_eq!(layout_version, Some(LayoutVersion));
    assert_eq!(node_linker, Some(NodeLinker::Isolated));
    assert!(included.dependencies);
    assert!(!included.dev_dependencies);
    assert!(included.optional_dependencies);
    assert_eq!(emitted_store_dir, store_dir.join(STORE_VERSION).display().to_string());
    // `read_modules_manifest` resolves `virtualStoreDir` against
    // `modules_dir`, so a relative on-disk value round-trips back
    // to the absolute install-time path.
    assert_eq!(emitted_virtual_store_dir, virtual_store_dir.to_string_lossy());
    assert_eq!(virtual_store_dir_max_length, DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH);
    assert_eq!(
        registries.as_ref().and_then(|r| r.get("default")).map(String::as_str),
        Some(config.registry.as_str()),
    );
    assert!(
        package_manager.starts_with("pacquet@"),
        "expected `pacquet@<version>`, got {package_manager:?}",
    );

    drop(dir);
}

/// `pnpm run`'s `verifyDepsBeforeRun` gate at
/// <https://github.com/pnpm/pnpm/blob/7ff112bac6/deps/status/src/checkDepsStatus.ts#L80-L86>
/// bails to "outdated" the moment
/// `<workspaceDir>/node_modules/.pnpm-workspace-state-v1.json` is
/// missing. Pacquet must write it on every install so pnpm can fast-path
/// the check after pacquet has materialized the modules tree — that's
/// the gap behind the
/// [`pnpm_config_verify_deps_before_run: false`](https://github.com/pnpm/pnpm/commit/7ff112bac6)
/// workaround in pnpm's own CI.
#[tokio::test]
async fn install_writes_workspace_state() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
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
        lockfile: Some(&lockfile),
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
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("frozen-lockfile install should succeed");

    let state = load_workspace_state(dir.path())
        .expect("read workspace state")
        .expect("workspace state file exists after install");

    assert!(
        state.last_validated_timestamp > 0,
        "lastValidatedTimestamp should be populated, got {}",
        state.last_validated_timestamp,
    );

    // The state must record the project that pacquet just installed
    // so pnpm's `allProjects.length !== Object.keys(projects).length`
    // check passes. Single-project install → exactly one entry, keyed
    // on the workspace dir.
    assert_eq!(state.projects.len(), 1);
    let project_key = dir.path().to_string_lossy().into_owned();
    let project = state
        .projects
        .get(&project_key)
        .unwrap_or_else(|| panic!("project entry for {project_key:?} should exist"));
    assert_eq!(
        project,
        &workspace_state::ProjectEntry {
            // `PackageManifest::create_if_needed` seeds `name` from the
            // parent dir's basename and `version` from `"1.0.0"`. The
            // test pins the round-trip of both fields so a regression
            // that loses them (e.g. switching to a non-string serde
            // shape) trips here.
            name: Some(
                dir.path()
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

    drop(dir);
}

/// Unit tests for [`super::build_projects_map`] / [`super::build_workspace_state`].
/// Ports the cases in upstream's
/// [`createWorkspaceState.test.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/workspace/state/test/createWorkspaceState.test.ts):
/// the `projects` map must contain one entry per project in the list,
/// keyed on the project root dir. Pacquet's `build_projects_map`
/// derives the list directly from `project_manifests` — same shape as
/// pnpm's `createWorkspaceState` taking `allProjects` — so a fresh
/// install that hasn't written a `pnpm-lock.yaml` yet still records
/// every workspace project, not just the root.
mod build_workspace_state_tests {
    use super::super::build_workspace_state;
    use pacquet_config::Config;
    use pacquet_modules_yaml::IncludedDependencies;
    use pacquet_package_manifest::PackageManifest;
    use pacquet_workspace_state::ConfigDependency;
    use std::{collections::BTreeMap, path::PathBuf};
    use tempfile::tempdir;

    fn write_manifest(dir: &std::path::Path, name: &str, version: &str) -> PackageManifest {
        let manifest_path = dir.join("package.json");
        std::fs::write(&manifest_path, format!(r#"{{"name":"{name}","version":"{version}"}}"#))
            .unwrap();
        PackageManifest::from_path(manifest_path).unwrap()
    }

    /// Ports `createWorkspaceState() on empty list`: a zero-project
    /// input produces an empty `projects` map but still populates the
    /// timestamp.
    #[test]
    fn empty_project_list_produces_empty_projects_map() {
        let config = Config::new();
        let state = build_workspace_state(
            &config,
            pacquet_config::NodeLinker::default(),
            IncludedDependencies::default(),
            &[],
        );
        assert!(state.projects.is_empty());
        assert!(state.last_validated_timestamp > 0);
    }

    /// Ports `createWorkspaceState() on non-empty list`: every project
    /// in the list lands in `state.projects` keyed by its `root_dir`.
    /// Regression catch for the bug where a workspace fresh install
    /// (no `pnpm-lock.yaml` on disk) recorded only the root importer.
    #[test]
    fn records_every_workspace_project_keyed_by_root_dir() {
        let dir = tempdir().unwrap();
        let packages = ["a", "b", "c", "d"];
        let manifests: Vec<(PathBuf, PackageManifest)> = packages
            .iter()
            .map(|name| {
                let project_dir = dir.path().join("packages").join(name);
                std::fs::create_dir_all(&project_dir).unwrap();
                let manifest = write_manifest(&project_dir, name, "1.0.0");
                (project_dir, manifest)
            })
            .collect();
        let project_manifests: Vec<(PathBuf, &PackageManifest)> =
            manifests.iter().map(|(p, m)| (p.clone(), m)).collect();

        let config = Config::new();
        let state = build_workspace_state(
            &config,
            pacquet_config::NodeLinker::default(),
            IncludedDependencies::default(),
            &project_manifests,
        );

        assert_eq!(state.projects.len(), packages.len());
        for (project_dir, _) in &manifests {
            let key = project_dir.to_string_lossy().into_owned();
            let entry = state
                .projects
                .get(&key)
                .unwrap_or_else(|| panic!("project entry for {key:?} should exist"));
            assert_eq!(entry.version.as_deref(), Some("1.0.0"));
            assert!(packages.contains(&entry.name.as_deref().unwrap_or_default(),));
        }
    }

    /// pnpm's `createWorkspaceState` records `configDependencies`
    /// verbatim. When pacquet is the install engine for a project that
    /// declares one (the `@pnpm/pacquet` configDependency itself), the
    /// written state must carry the same map — otherwise pnpm's
    /// `checkDepsStatus` reads a missing value, treats the install as
    /// stale, and reinstalls on every `pnpm run` / `pnpm node`.
    #[test]
    fn records_config_dependencies_from_config() {
        let mut config = Config::new();
        config.config_dependencies = Some(BTreeMap::from([(
            "@pnpm/pacquet".to_string(),
            ConfigDependency::VersionWithIntegrity("0.2.2-14".to_string()),
        )]));
        let state = build_workspace_state(
            &config,
            pacquet_config::NodeLinker::default(),
            IncludedDependencies::default(),
            &[],
        );
        assert_eq!(state.config_dependencies, config.config_dependencies);
    }
}

/// Ports `'do not fail on an optional dependency that has a non-optional
/// dependency with a failing postinstall script'` at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-installer/test/install/optionalDependencies.ts#L563-L572>.
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
/// NOT abort, matching the upstream expectation that `addDependenciesToPackage`
/// resolves.
#[tokio::test]
async fn install_optional_failing_postinstall_dep_via_registry_mock_succeeds() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/has-failing-postinstall-dep", "1.0.0", DependencyGroup::Optional)
        .unwrap();
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("optional dep with failing transitive postinstall must NOT abort the install");

    // Both the wrapper and the transitive must reach the virtual store.
    assert!(
        is_symlink_or_junction(
            &project_root.join("node_modules/@pnpm.e2e/has-failing-postinstall-dep"),
        )
        .unwrap(),
        "wrapper symlink missing",
    );
    assert!(
        project_root
            .join("node_modules/.pacquet/@pnpm.e2e+has-failing-postinstall-dep@1.0.0")
            .is_dir(),
        "wrapper virtual-store dir missing",
    );
    assert!(
        project_root.join("node_modules/.pacquet/@pnpm.e2e+failing-postinstall@1.0.0").is_dir(),
        "transitive `failing-postinstall` must be extracted to the virtual store",
    );

    drop((dir, mock_instance));
}

/// Regression for pnpm/pnpm#11934: `peerDependenciesMeta` must be
/// preserved end-to-end so optional peers are not auto-installed.
/// Ported from upstream's
/// [`peerDependencies.ts:1181-1255`](https://github.com/pnpm/pnpm/blob/1fb8a2d5d8/installing/deps-installer/test/install/peerDependencies.ts#L1181-L1255).
#[tokio::test]
async fn auto_install_peers_does_not_cascade_optional_peers() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/abc-optional-peers", "1.0.0", DependencyGroup::Prod)
        .unwrap();
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install with optional peers should succeed");

    let virtual_store_slots: Vec<String> = std::fs::read_dir(&virtual_store_dir)
        .expect("read virtual store dir")
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
        is_symlink_or_junction(&project_root.join("node_modules/@pnpm.e2e/abc-optional-peers"))
            .unwrap(),
        "abc-optional-peers must be symlinked at the importer level",
    );

    drop((dir, mock_instance));
}

/// Companion to [`auto_install_peers_does_not_cascade_optional_peers`]:
/// `@pnpm.e2e/abc-optional-peers-meta-only@1.0.0` declares `peer-b` and
/// `peer-c` **only** through `peerDependenciesMeta`, with no matching
/// `peerDependencies` entry. Upstream and pacquet treat such entries
/// as optional peers with implicit range `*`; the install must still
/// keep them out of the tree when no other consumer requests them.
///
/// Ported from upstream's "warning is not reported when cannot resolve
/// optional peer dependency (specified by meta field only)" at
/// [`installing/deps-installer/test/install/peerDependencies.ts`](https://github.com/pnpm/pnpm/blob/1fb8a2d5d8/installing/deps-installer/test/install/peerDependencies.ts#L1257-L1323).
#[tokio::test]
async fn auto_install_peers_skips_meta_only_optional_peers() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/abc-optional-peers-meta-only", "1.0.0", DependencyGroup::Prod)
        .unwrap();
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install with meta-only optional peers should succeed");

    let virtual_store_slots: Vec<String> = std::fs::read_dir(&virtual_store_dir)
        .expect("read virtual store dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();

    // peer-a is declared in `peerDependencies` so it stays required and
    // gets auto-installed; peer-b and peer-c are declared *only* in
    // `peerDependenciesMeta` with `optional: true` and stay out of the
    // tree.
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

    drop((dir, mock_instance));
}

/// A v9 lockfile fixture pinned to a placeholder package whose
/// integrity is bogus on purpose. Pacquet enforces tarball integrity
/// on the install path, so any test that lets the install reach the
/// fetch site would fail — meaning a successful install with this
/// fixture is *proof* that the per-snapshot skip path (issue [#433]
/// section B) short-circuited the fetch entirely.
///
/// [#433]: https://github.com/pnpm/pacquet/issues/433
const PARTIAL_INSTALL_LOCKFILE: &str = text_block! {
    "lockfileVersion: '9.0'"
    "importers:"
    "  .:"
    "    dependencies:"
    "      placeholder:"
    "        specifier: 1.0.0"
    "        version: 1.0.0"
    "packages:"
    "  placeholder@1.0.0:"
    "    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA, tarball: 'http://invalid.local/placeholder.tgz'}"
    "snapshots:"
    "  placeholder@1.0.0: {}"
};

/// Pre-populate the virtual-store slot that `PARTIAL_INSTALL_LOCKFILE`
/// describes so the skip path has a directory to point at. Just the
/// `<virtual_store_dir>/placeholder@1.0.0/node_modules/placeholder`
/// dirent is enough — the skip check only stats the directory, it
/// doesn't read CAS contents.
fn seed_placeholder_virtual_store_slot(virtual_store_dir: &std::path::Path) {
    let slot = virtual_store_dir.join("placeholder@1.0.0").join("node_modules").join("placeholder");
    std::fs::create_dir_all(&slot).expect("create placeholder virtual-store slot");
}

/// Section B of pnpm/pacquet#433: a snapshot whose wiring and
/// integrity match the current lockfile *and* whose virtual-store
/// slot exists on disk is dropped from the install graph entirely.
/// We prove this by pointing the lockfile at a bogus tarball URL —
/// any code path that reaches the fetch site would fail, so a
/// successful install demonstrates the skip path took over.
#[tokio::test]
async fn warm_reinstall_skips_snapshot_when_current_lockfile_matches() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // Manifest must match `PARTIAL_INSTALL_LOCKFILE` — the freshness
    // check (<https://github.com/pnpm/pacquet/issues/447>) rejects any drift between the on-disk manifest and
    // the lockfile importer entry.
    manifest.add_dependency("placeholder", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    // Opt out of the (now-default) global virtual store: the
    // `seed_placeholder_virtual_store_slot` helper writes the legacy
    // `<virtual_store_dir>/<flat-name>` shape, which only matches the
    // skip-probe path when `VirtualStoreLayout` is in legacy mode.
    // The partial-install behaviour under test (skip when the
    // current lockfile matches + slot exists) is independent of the
    // GVS layout; the GVS-on equivalent is exercised by the
    // `frozen_lockfile_under_gvs_*` tests below.
    config.enable_global_virtual_store = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

    // Pre-seed the previous-install state: write the current lockfile
    // identical to the wanted lockfile, and materialize the virtual-
    // store slot the skip check stats against.
    std::fs::create_dir_all(&virtual_store_dir).unwrap();
    lockfile.save_current_to_virtual_store_dir(&virtual_store_dir).expect("seed current lockfile");
    seed_placeholder_virtual_store_slot(&virtual_store_dir);

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: true, // fixture pins a tripwire tarball URL; skip resolution verification so the tarball-URL check doesn't flag it before the path under test
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect(
        "skip path must short-circuit the fetch for the placeholder snapshot \
         (bogus integrity + URL would otherwise fail the install)",
    );

    // `lock.yaml` survives the install — the end-of-install write
    // persists the wanted lockfile back to disk.
    let written = Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        .expect("read written current lockfile")
        .expect("current lockfile should be written");
    assert_eq!(written.snapshots.as_ref().map(std::collections::HashMap::len), Some(1));

    drop(dir);
}

/// When the cached directory is gone but the cache key still matches,
/// pacquet emits `pnpm:_broken_node_modules` (mirroring upstream's
/// debug emit at `lockfileToDepGraph.ts:258`) and falls through to the
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

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // Manifest must match `PARTIAL_INSTALL_LOCKFILE` — the freshness
    // check (<https://github.com/pnpm/pacquet/issues/447>) rejects any drift between the on-disk manifest and
    // the lockfile importer entry.
    manifest.add_dependency("placeholder", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    // Opt out of the GVS layout — see the rationale on
    // [`warm_reinstall_skips_snapshot_when_current_lockfile_matches`].
    // The pre-seeded `<virtual_store_dir>/<flat-name>` slot is the
    // legacy shape the probe matches; the BrokenModules emit fires
    // identically under either layout once the slot is missing.
    config.enable_global_virtual_store = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
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
    std::fs::create_dir_all(&virtual_store_dir).unwrap();
    lockfile.save_current_to_virtual_store_dir(&virtual_store_dir).expect("seed current lockfile");

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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: true, // fixture pins a tripwire tarball URL; skip resolution verification so the tarball-URL check doesn't flag it before the path under test
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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

    drop(dir);
}

/// Section A + D of pnpm/pacquet#433: a second install observes
/// `pnpm:context.currentLockfileExists: true` once the first install
/// has written `<virtual_store_dir>/lock.yaml`. Drives the read site
/// (`Install::run` → `load_current_from_virtual_store_dir`) on real
/// disk state produced by the matching write site.
#[tokio::test]
async fn context_log_reflects_current_lockfile_after_first_install() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // Manifest must match the fixture lockfile below — the freshness
    // check (<https://github.com/pnpm/pacquet/issues/447>) rejects any drift between the on-disk manifest and
    // the lockfile importer entry.
    manifest.add_dependency("placeholder", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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
    let lock_yaml = virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
    assert!(
        lock_yaml.is_file(),
        "non-empty wanted lockfile must be persisted under <virtual_store_dir>/lock.yaml; found nothing at {lock_yaml:?}",
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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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

    drop(dir);
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

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // Manifest must match `PARTIAL_INSTALL_LOCKFILE` — the freshness
    // check (<https://github.com/pnpm/pacquet/issues/447>) rejects any drift between the on-disk manifest and
    // the lockfile importer entry.
    manifest.add_dependency("placeholder", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    // Opt out of the GVS layout — the pre-seeded
    // `<virtual_store_dir>/<flat-name>` slot is the legacy shape the
    // skip probe matches under
    // [`warm_reinstall_skips_snapshot_when_current_lockfile_matches`].
    config.enable_global_virtual_store = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

    std::fs::create_dir_all(&virtual_store_dir).unwrap();
    lockfile.save_current_to_virtual_store_dir(&virtual_store_dir).expect("seed current lockfile");
    seed_placeholder_virtual_store_slot(&virtual_store_dir);

    EVENTS.lock().unwrap().clear();
    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: true, // fixture pins a tripwire tarball URL; skip resolution verification so the tarball-URL check doesn't flag it before the path under test
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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

    drop(dir);
}

/// Issue [#447]: a `--frozen-lockfile` install where the on-disk
/// `package.json` has drifted from the lockfile importer entry must
/// fail with `OutdatedLockfile` *before* any fetch or link work
/// starts. Mirrors upstream's `ERR_PNPM_OUTDATED_LOCKFILE` thrown
/// from `pkg-manager/core/src/install/index.ts:823` — CI-correctness
/// guarantee that pacquet can't silently install the wrong shape of
/// `node_modules` when the manifest and lockfile diverge.
///
/// We use the partial-install fixture (bogus tarball URL) and *omit*
/// adding the placeholder dep to the manifest. If the check fails to
/// fire, the install reaches the fetch site and errors with a
/// network / integrity failure — distinguishable from the early
/// `OutdatedLockfile` we expect.
///
/// `trust_lockfile` is on so lockfile-resolution verification is
/// skipped: the unconditional tarball-URL binding check would otherwise
/// flag the fixture's tripwire tarball URL as a `TARBALL_URL_MISMATCH`
/// before the drift gate runs. Verification is orthogonal to the drift
/// check this test exercises.
///
/// [#447]: https://github.com/pnpm/pacquet/issues/447
#[tokio::test]
async fn frozen_lockfile_errors_when_manifest_drifts_from_lockfile() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    // Deliberately do NOT add the `placeholder` dep — this is the
    // drift case the check has to catch.
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: true,
        update_checksums: false,
        is_full_install: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    let err = result.expect_err("drifted manifest must surface as OutdatedLockfile");
    assert!(
        matches!(err, InstallError::OutdatedLockfile { .. }),
        "expected OutdatedLockfile, got {err:?}",
    );

    drop(dir);
}

/// `--ignore-manifest-check` (`Install::ignore_manifest_check = true`)
/// bypasses the [`satisfies_package_manifest`] gate, so an install
/// whose manifest has drifted from the lockfile proceeds past the
/// freshness check. Same setup as
/// `frozen_lockfile_errors_when_manifest_drifts_from_lockfile`, just
/// with the flag flipped: we now expect the install to reach the
/// fetch site and fail there (network / integrity error against the
/// bogus tarball URL) rather than abort early with `OutdatedLockfile`.
///
/// Issue context: <https://github.com/pnpm/pnpm/issues/11797>.
#[tokio::test]
async fn ignore_manifest_check_bypasses_manifest_freshness_gate() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    // Deliberately leave the `placeholder` dep out — same drift the
    // sibling test exercises. With `ignore_manifest_check: true` the
    // install must accept the drift and move on to materialization.
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: true,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    let err = result.expect_err("bogus tarball URL must still surface a downstream error");
    assert!(
        !matches!(err, InstallError::OutdatedLockfile { .. }),
        "ignore_manifest_check should bypass the freshness gate, got OutdatedLockfile: {err:?}",
    );

    drop(dir);
}

/// `pnpm.overrides` drift between the lockfile-recorded map and the
/// current config surfaces as `OutdatedLockfile` with a
/// `StalenessReason::OverridesChanged` payload. Mirrors upstream's
/// `getOutdatedLockfileSetting → 'overrides'` branch firing
/// `LockfileConfigMismatchError` under `--frozen-lockfile`.
#[tokio::test]
async fn frozen_lockfile_errors_when_overrides_drift_from_lockfile() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    // Config declares an override the lockfile doesn't carry → drift.
    let mut overrides = indexmap::IndexMap::new();
    overrides.insert("placeholder".to_string(), "9.9.9".to_string());
    config.overrides = Some(overrides);
    let config = config.leak();

    // Lockfile fixture has *no* `overrides:` key.
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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    let err = result.expect_err("overrides drift must surface as OutdatedLockfile");
    match err {
        InstallError::OutdatedLockfile {
            reason: pacquet_lockfile::StalenessReason::OverridesChanged { .. },
        } => {}
        other => panic!("expected OutdatedLockfile::OverridesChanged, got {other:?}"),
    }

    drop(dir);
}

/// When `pnpm.overrides` is set, the freshness check applies overrides
/// to a clone of the manifest before comparing against the lockfile.
/// Without this step, the lockfile's post-override specifier and the
/// manifest's pre-override specifier would always disagree, failing
/// every frozen install with `SpecifiersDiffer`. This test pins that
/// behavior: a manifest declaring `foo: ^1` plus an override
/// `foo: 2.0.0` lines up with a lockfile that records `foo: 2.0.0` —
/// after override application — and the install proceeds.
#[tokio::test]
async fn frozen_lockfile_applies_overrides_to_manifest_before_freshness_check() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    seed_placeholder_virtual_store_slot(&virtual_store_dir);

    let manifest_path = dir.path().join("package.json");
    // Manifest lists `placeholder: ^9` (pre-override). Without
    // override application this would trip the freshness check
    // because the lockfile records `placeholder: 1.0.0`.
    std::fs::write(
        &manifest_path,
        r#"{"name":"my-app","version":"1.0.0","dependencies":{"placeholder":"^9"}}"#,
    )
    .unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    let mut overrides = indexmap::IndexMap::new();
    overrides.insert("placeholder".to_string(), "1.0.0".to_string());
    config.overrides = Some(overrides);
    let config = config.leak();

    // Lockfile carries the SAME override map so the drift check
    // passes; the importer's specifier reflects the post-override
    // value `1.0.0`.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "overrides:"
        "  placeholder: 1.0.0"
        "importers:"
        "  .:"
        "    dependencies:"
        "      placeholder:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages:"
        "  placeholder@1.0.0:"
        "    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA, tarball: 'http://invalid.local/placeholder.tgz'}"
        "snapshots:"
        "  placeholder@1.0.0: {}"
    })
    .expect("parse fixture lockfile with overrides");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    // The install should not fail with `OutdatedLockfile` — the
    // overrider rewrites `placeholder: ^9 → 1.0.0` on the cloned
    // manifest, lining up with the lockfile. (The install may still
    // fail later for unrelated reasons in this minimal fixture, but
    // the freshness gate must pass.)
    if let Err(InstallError::OutdatedLockfile { reason }) = &result {
        panic!("unexpected OutdatedLockfile after override application: {reason:?}");
    }

    drop(dir);
}

/// `pnpm.overrides` values can reference a workspace catalog via the
/// `catalog:` protocol; pnpm resolves them against `catalogs:` in
/// `pnpm-workspace.yaml` and writes the *resolved* specifier to
/// `pnpm-lock.yaml#overrides`. The freshness check must therefore
/// resolve `catalog:` on the config side too before comparing — a
/// raw string compare would treat `catalog:` ≠ `<concrete>` on every
/// install. Mirrors pnpm's
/// [`parseOverrides(overrides, catalogs)`](https://github.com/pnpm/pnpm/blob/4a36b9a110/config/parse-overrides/src/index.ts#L20-L44)
/// →
/// [`createOverridesMapFromParsed`](https://github.com/pnpm/pnpm/blob/4a36b9a110/lockfile/settings-checker/src/createOverridesMapFromParsed.ts)
/// pipeline. Regression test for the case the user hit on a workspace
/// whose `pnpm.overrides` declared catalog-backed entries.
#[tokio::test]
async fn frozen_lockfile_resolves_catalog_protocol_in_overrides_before_freshness_check() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    std::fs::create_dir_all(&project_root).expect("create project root");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    seed_placeholder_virtual_store_slot(&virtual_store_dir);

    // The catalog lives in `pnpm-workspace.yaml` next to the manifest;
    // `Install::run` walks up from the manifest dir to find it.
    std::fs::write(
        project_root.join("pnpm-workspace.yaml"),
        text_block! {
            "catalogs:"
            "  default:"
            "    placeholder: 1.0.0"
        },
    )
    .unwrap();

    let manifest_path = project_root.join("package.json");
    std::fs::write(
        &manifest_path,
        r#"{"name":"my-app","version":"1.0.0","dependencies":{"placeholder":"^9"}}"#,
    )
    .unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    // Override value is `catalog:`, which must resolve to the
    // catalog's `placeholder: 1.0.0` entry before the freshness
    // comparison. The lockfile records the *resolved* `1.0.0`, so a
    // raw compare would fail on every install.
    let mut overrides = indexmap::IndexMap::new();
    overrides.insert("placeholder".to_string(), "catalog:".to_string());
    config.overrides = Some(overrides);
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "overrides:"
        "  placeholder: 1.0.0"
        "importers:"
        "  .:"
        "    dependencies:"
        "      placeholder:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages:"
        "  placeholder@1.0.0:"
        "    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA, tarball: 'http://invalid.local/placeholder.tgz'}"
        "snapshots:"
        "  placeholder@1.0.0: {}"
    })
    .expect("parse fixture lockfile with overrides");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    // The freshness check must accept the install; before the fix
    // pacquet would surface `OutdatedLockfile::OverridesChanged`
    // because `catalog:` ≠ `1.0.0` as raw strings. (The install may
    // still fail later for unrelated reasons in this minimal fixture,
    // but the overrides gate must pass.)
    if let Err(InstallError::OutdatedLockfile { reason }) = &result {
        panic!("unexpected OutdatedLockfile after catalog resolution: {reason:?}");
    }

    drop(dir);
}

/// Negative-case: lockfile loads successfully but has no
/// `importers["."]` entry for the project being installed. Distinct
/// from `NoLockfile` (file missing entirely) — here the file is
/// well-formed but doesn't describe this project. Should surface as
/// `NoImporter`, also before any fetch attempt.
#[tokio::test]
async fn frozen_lockfile_errors_when_lockfile_has_no_root_importer() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    // Empty-importers lockfile — valid v9 shape, but no entry for
    // the root project.
    let lockfile: Lockfile =
        serde_saphyr::from_str("lockfileVersion: '9.0'\n").expect("parse minimal lockfile");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    let err = result.expect_err("missing root importer must surface as NoImporter");
    assert!(
        matches!(err, InstallError::NoImporter { ref importer_id } if importer_id == "."),
        "expected NoImporter for `.`, got {err:?}",
    );

    drop(dir);
}

/// GVS-on frozen-lockfile install. With
/// `enable_global_virtual_store: true` (an explicit opt-in;
/// pacquet's default is `false`, matching pnpm v11's effective
/// default for non-`--global` installs — see
/// [`pacquet_config::default_enable_global_virtual_store`]),
/// `Install::run` registers the project at
/// `<store_dir>/projects/<short-hash>` (mirroring upstream's
/// [`registerProject`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/projectRegistry.ts))
/// and routes every per-snapshot slot through
/// [`crate::VirtualStoreLayout`]. The empty-snapshot lockfile here is
/// enough to prove the wiring runs end-to-end without panicking and
/// that the registry entry actually lands on disk; the GVS-shaped
/// per-package path layout itself is unit-tested inside the
/// [`crate::VirtualStoreLayout`] module, and the e2e port of
/// upstream's `globalVirtualStore.ts` cases (with non-empty
/// snapshots) is tracked as a follow-up.
#[tokio::test]
async fn frozen_lockfile_under_gvs_registers_project_and_runs_clean() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    // Place the manifest *inside* `project_root` — `Install::run`
    // derives the registry target from `manifest.path().parent()`,
    // so a manifest at `<tmp>/package.json` would register `<tmp>`
    // and the symlink-resolves-to-project_root assertion below
    // would silently pass for the wrong reason.
    std::fs::create_dir_all(&project_root).expect("create project root");
    let manifest_path = project_root.join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    // Pin GVS on explicitly — pacquet's default is `false`, so this
    // test would test the wrong path otherwise.
    config.enable_global_virtual_store = true;
    config.lockfile = false;
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    // Pin the GVS root to a known location under the test temp dir
    // so any future assertions can target it without walking the
    // SmartDefault'd cwd-based fallback.
    config.global_virtual_store_dir = store_dir.join("links");
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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("frozen-lockfile install under GVS should succeed");

    // `register_project` wrote `<store_dir>/v11/projects/<short-hash>`
    // pointing back at the project dir. Canonicalize the *entry
    // path* (not `read_link`'s output) so the kernel follows the
    // symlink — pacquet, like upstream pnpm, writes the target as
    // a path relative to the link's parent, so canonicalizing the
    // raw `read_link` string from the CWD would never resolve.
    let projects_dir = store_dir.join("v11/projects");
    assert!(projects_dir.is_dir(), "GVS-on install must create <store_dir>/v11/projects/");
    let entries: Vec<_> =
        std::fs::read_dir(&projects_dir).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(entries.len(), 1, "exactly one project entry per `Install::run` invocation");
    assert_eq!(
        dunce::canonicalize(entries[0].path()).expect("canonicalize registry entry"),
        dunce::canonicalize(&project_root).expect("canonicalize project root"),
        "registry symlink must resolve back to the install's project root",
    );

    drop(dir);
}

/// Under GVS, the `virtualStoreDir` value pacquet persists in
/// `.modules.yaml` must equal the path upstream pnpm writes — i.e.
/// `<storeDir>/v11/links` — not the project-local `node_modules/.pnpm`
/// path pacquet keeps internally in [`Config::virtual_store_dir`]. If
/// they diverge, the next `pnpm install` reads the manifest, recomputes
/// `ctx.virtualStoreDir` from the GVS-on path, and trips upstream's
/// [`checkCompatibility`](https://github.com/pnpm/pnpm/blob/f2a4d2caef/installing/deps-installer/src/install/checkCompatibility/index.ts#L37-L43)
/// with `ERR_PNPM_UNEXPECTED_VIRTUAL_STORE_DIR` for every project,
/// forcing the "modules directories will be reinstalled from scratch"
/// prompt on every invocation. The same value is also emitted on the
/// `pnpm:context` channel that `@pnpm/cli.default-reporter` parses, so
/// the same parity rule applies there.
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

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    std::fs::create_dir_all(&project_root).expect("create project root");
    let manifest_path = project_root.join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    // GVS on — the whole point of the test.
    config.enable_global_virtual_store = true;
    config.lockfile = false;
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    // Keep `virtual_store_dir` at the project-local path. Pacquet's
    // internal layout consumers still read this field; the parity
    // requirement is only that the externally-observed value (the one
    // pnpm sees in `.modules.yaml` / `pnpm:context`) routes through
    // `global_virtual_store_dir` via `effective_virtual_store_dir`.
    config.virtual_store_dir = virtual_store_dir.clone();
    // Source the GVS root from `store_dir.links()` so the assertion
    // below targets the same v11-suffixed path the
    // [`From<PathBuf> for StoreDir`] impl produces in production. Hard-
    // coding `store_dir.join("links")` here would drop the `v11`
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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<RecordingReporter>()
    .await
    .expect("frozen-lockfile install under GVS should succeed");

    // The path pnpm would have written. `StoreDir::from` appends the
    // [`STORE_VERSION`] suffix to the configured root, so the live
    // value is `<store_dir>/v11/links` even though the test handed
    // `Config::store_dir` the un-suffixed root.
    let expected_resolved = store_dir.join(STORE_VERSION).join("links");

    // Ensure the GVS root exists on disk so `dunce::canonicalize` can
    // resolve it. An empty-lockfile install doesn't link anything into
    // `<store_dir>/v11/links/`, so the dir would otherwise be absent.
    std::fs::create_dir_all(&expected_resolved).expect("create GVS links dir for canonicalize");
    let expected_canonical =
        dunce::canonicalize(&expected_resolved).expect("canonicalize GVS links dir");

    // `.modules.yaml` is what `pnpm install` reads on the *next*
    // invocation; this is the round-trip pnpm's `checkCompatibility`
    // sees. `read_modules_manifest` normalises the stored relative
    // path back to absolute against `modules_dir`, so a successful
    // assertion proves both halves: pacquet wrote the GVS path, and
    // the relative form on disk re-resolves to it. Canonicalize the
    // result because `read_modules_manifest`'s
    // `modules_dir.join(relative)` keeps `..` segments verbatim, while
    // pnpm's `path.relative(modules.virtualStoreDir, opts.virtualStoreDir)`
    // check at
    // [`checkCompatibility/index.ts:37-43`](https://github.com/pnpm/pnpm/blob/f2a4d2caef/installing/deps-installer/src/install/checkCompatibility/index.ts#L37-L43)
    // reduces them before comparing.
    let read_back =
        read_modules_manifest::<Host>(&modules_dir).expect("read .modules.yaml").expect("present");
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

    drop(dir);
}

/// GVS-off frozen-lockfile install. The dispatch path is the same,
/// but `Install::run` skips the project-registry write entirely.
/// Pins that turning off `enable_global_virtual_store` makes the
/// install behave like today — no `<store_dir>/projects/` directory
/// appears.
#[tokio::test]
async fn frozen_lockfile_with_gvs_off_skips_project_registry() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    std::fs::create_dir_all(&project_root).expect("create project root");
    let manifest_path = project_root.join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.enable_global_virtual_store = false;
    config.lockfile = false;
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("frozen-lockfile install with GVS off should succeed");

    assert!(
        !store_dir.join("v11/projects").exists(),
        "GVS-off install must NOT create the project-registry directory",
    );

    drop(dir);
}

/// Workspace install under GVS registers the workspace root once,
/// regardless of how many importers the workspace declares. Mirrors
/// upstream's
/// [`registerProject(opts.storeDir, opts.lockfileDir)`](https://github.com/pnpm/pnpm/blob/d8a79a9c30/installing/context/src/index.ts#L128)
/// call site in `getContext`, which fires exactly once per install
/// against the workspace root — store prune walks
/// `<workspace>/node_modules/.pnpm/` to find every installed package,
/// so one registry entry per workspace is enough.
#[tokio::test]
async fn frozen_lockfile_under_gvs_registers_workspace_root_only() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let workspace_root = dir.path().join("workspace");
    let modules_dir = workspace_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    // Workspace layout: root + one sub-importer. The sub-importer's
    // directory exists on disk because the lockfile reader needs it,
    // but registration only resolves the workspace root.
    let web_dir = workspace_root.join("packages/web");
    std::fs::create_dir_all(&web_dir).expect("create packages/web");
    let manifest_path = workspace_root.join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    std::fs::write(web_dir.join("package.json"), "{}").expect("write packages/web/package.json");

    let mut config = Config::new();
    config.enable_global_virtual_store = true;
    config.lockfile = false;
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    config.global_virtual_store_dir = store_dir.join("links");
    let config = config.leak();

    // Two importers: `.` and `packages/web`. Empty dep graph so the
    // install reaches the registry-write call without doing any actual
    // fetch/link work.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies: {}"
        "  packages/web:"
        "    dependencies: {}"
        "packages: {}"
        "snapshots: {}"
    })
    .expect("parse minimal v9 workspace lockfile");

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("workspace frozen-lockfile install under GVS should succeed");

    // Exactly one registry entry resolving back to the workspace
    // root, matching pnpm's once-per-install `registerProject(storeDir,
    // lockfileDir)` shape.
    let projects_dir = store_dir.join("v11/projects");
    assert!(
        projects_dir.is_dir(),
        "GVS-on workspace install must create <store_dir>/v11/projects/",
    );
    let entries: Vec<_> =
        std::fs::read_dir(&projects_dir).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(
        entries.len(),
        1,
        "workspace install registers the workspace root once, not once per importer",
    );
    assert_eq!(
        dunce::canonicalize(entries[0].path()).expect("canonicalize registry entry"),
        dunce::canonicalize(&workspace_root).expect("canonicalize workspace root"),
        "registry symlink must resolve back to the workspace root",
    );

    drop(dir);
}

/// `build_modules_manifest` serializes the install-time
/// [`SkippedSnapshots`] into `.modules.yaml.skipped` as a list of
/// depPath strings. Mirrors upstream's
/// `skipped: Array.from(ctx.skipped)` literal at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/index.ts#L1625>:
/// each entry is the snapshot's [`PackageKey`] `Display` form
/// (`name@version(peers)`), and ordering is handled by
/// `write_modules_manifest`'s sort-on-write.
///
/// An empty set produces an empty list — covers the fresh-install
/// case while pinning that the field is no longer
/// `..Default::default()`'d away from the manifest.
///
/// [`SkippedSnapshots`]: super::super::SkippedSnapshots
/// [`PackageKey`]: pacquet_lockfile::PackageKey
#[test]
fn build_modules_manifest_serializes_skipped_set() {
    use crate::SkippedSnapshots;
    use pacquet_lockfile::PackageKey;
    use pacquet_modules_yaml::IncludedDependencies;
    use std::collections::HashSet;

    let dir = tempdir().unwrap();
    let mut config = Config::new();
    config.store_dir = dir.path().join("store").into();
    config.modules_dir = dir.path().join("node_modules");
    config.virtual_store_dir = dir.path().join("node_modules/.pacquet");
    let config = config.leak();

    let key1: PackageKey = "darwin-only-pkg@1.0.0".parse().unwrap();
    let key2: PackageKey = "@scope/linux-only@2.3.4".parse().unwrap();
    let mut set = HashSet::new();
    set.insert(key1.clone());
    set.insert(key2.clone());
    let skipped = SkippedSnapshots::from_set(set);

    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: true,
    };
    let manifest = super::build_modules_manifest(
        config,
        pacquet_config::NodeLinker::default(),
        included,
        Default::default(),
        Default::default(),
        &skipped,
    );

    // Compare as sets — `build_modules_manifest` does not sort.
    // Sort-on-write happens later inside `write_modules_manifest`,
    // matching upstream's `saveModules.skipped.sort()` at
    // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/modules-yaml/src/index.ts#L121>;
    // the read-after-write order is covered by the integration
    // test on the full install path.
    let actual: HashSet<String> = manifest.skipped.iter().cloned().collect();
    let expected: HashSet<String> = [key1.to_string(), key2.to_string()].into_iter().collect();
    assert_eq!(actual, expected);
}

/// Empty `SkippedSnapshots` produces an empty `Modules.skipped`. The
/// common case — most installs have no platform-mismatched optional
/// deps — must keep the field present-but-empty so the on-disk
/// shape stays uniform.
#[test]
fn build_modules_manifest_skipped_is_empty_on_empty_set() {
    use crate::SkippedSnapshots;
    use pacquet_modules_yaml::IncludedDependencies;

    let dir = tempdir().unwrap();
    let mut config = Config::new();
    config.store_dir = dir.path().join("store").into();
    config.modules_dir = dir.path().join("node_modules");
    config.virtual_store_dir = dir.path().join("node_modules/.pacquet");
    let config = config.leak();

    let manifest = super::build_modules_manifest(
        config,
        pacquet_config::NodeLinker::default(),
        IncludedDependencies::default(),
        Default::default(),
        Default::default(),
        &SkippedSnapshots::new(),
    );
    assert!(manifest.skipped.is_empty());
    // Empty `hoisted_locations` is dropped to `None` so an
    // isolated install doesn't write a `hoistedLocations: {}` key
    // (which would falsely look like a stale hoisted-mode write).
    assert!(manifest.hoisted_locations.is_none());
}

/// End-to-end read → seed → write loop. Pre-write
/// `.modules.yaml.skipped` before a frozen-lockfile install runs;
/// confirm the install picks up the seed and re-writes the file
/// with the same key. The lockfile here has empty `snapshots: {}` so
/// the constraint-free fast path runs — that's the branch in
/// [`InstallFrozenLockfile::run`] that preserves the seed verbatim
/// without calling `compute_skipped_snapshots`. Together with the
/// unit tests on the slow path, this pins the full plumbing between
/// `read_modules_manifest`, `compute_skipped_snapshots`'s seed
/// arg, the threading out of [`InstallFrozenLockfileOutput`], and
/// `build_modules_manifest`'s serialization.
///
/// [`InstallFrozenLockfile::run`]: super::super::InstallFrozenLockfile::run
/// [`InstallFrozenLockfileOutput`]: super::super::InstallFrozenLockfileOutput
#[tokio::test]
async fn frozen_install_preserves_seeded_skipped_across_reinstall() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    // Pre-write `.modules.yaml` with a non-empty `skipped` list —
    // models the state left by a previous install that landed
    // platform-mismatched optional deps. Two entries: one bare
    // package and one scoped, so the parse-then-serialize round-trip
    // covers both `name@version` and `@scope/name@version`.
    let seeded_keys = ["previously-skipped@1.0.0", "@scope/also-skipped@2.3.4"];
    let seed_modules = Modules {
        layout_version: Some(LayoutVersion),
        node_linker: Some(NodeLinker::Isolated),
        store_dir: store_dir.display().to_string(),
        virtual_store_dir: virtual_store_dir.to_string_lossy().into_owned(),
        virtual_store_dir_max_length: DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH,
        skipped: seeded_keys.iter().map(|s| (*s).to_string()).collect(),
        ..Default::default()
    };
    write_modules_manifest::<Host>(&modules_dir, seed_modules).expect("seed .modules.yaml");

    // Empty lockfile drives the constraint-free fast path. The
    // seed must survive verbatim.
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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("frozen-lockfile install should succeed");

    let written = modules_dir
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read .modules.yaml")
        .expect("modules manifest exists");

    // Both seed entries survive — proves the read → seed → write
    // loop is fully wired. Order is the sort-on-write order:
    // `@scope/...` < `previously-...` lexically, so the scoped
    // entry leads.
    assert_eq!(written.skipped.len(), 2, "both seeded entries must survive");
    assert!(written.skipped.contains(&"previously-skipped@1.0.0".to_string()));
    assert!(written.skipped.contains(&"@scope/also-skipped@2.3.4".to_string()));
    let sorted: Vec<&str> = written.skipped.iter().map(String::as_str).collect();
    assert_eq!(
        sorted,
        ["@scope/also-skipped@2.3.4", "previously-skipped@1.0.0"],
        "write_modules_manifest must sort the list alphabetically",
    );

    drop(dir);
}

/// Port of upstream's
/// [`deps-restorer/test/index.ts:340`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/test/index.ts#L340-L360)
/// `skipping optional dependency if it cannot be fetched`. An
/// `optional: true` snapshot whose tarball URL is unreachable must
/// not abort the install — the failure is silently swallowed at
/// the per-snapshot fetch dispatch in `CreateVirtualStore`, mirroring
/// upstream's
/// [`lockfileToDepGraph.ts:294-298`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L294-L298)
/// catch site.
///
/// Asserts:
/// 1. The install resolves `Ok` (no abort).
/// 2. The broken snapshot's virtual-store slot was NOT created.
/// 3. The on-disk `.modules.yaml.skipped` does NOT contain the
///    broken snapshot — fetch failures are transient by upstream's
///    convention (the catch site never updates `opts.skipped`), so
///    a subsequent install retries the fetch.
#[tokio::test]
async fn frozen_install_silently_swallows_unreachable_optional_tarball() {
    // Lockfile with one `optional: true` snapshot whose `tarball` URL
    // dials `127.0.0.1:1` (a reserved port that always refuses) so
    // the fetch reliably fails without a network round-trip. The
    // integrity is arbitrary — we never get far enough to verify it.
    const BROKEN_OPTIONAL_LOCKFILE: &str = text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    optionalDependencies:"
        "      broken-pkg:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages:"
        "  broken-pkg@1.0.0:"
        "    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA, tarball: 'http://127.0.0.1:1/broken.tgz'}"
        "snapshots:"
        "  broken-pkg@1.0.0:"
        "    optional: true"
    };

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // Manifest must match the lockfile importer entry so the
    // freshness check (<https://github.com/pnpm/pacquet/issues/447>) doesn't reject the install before we
    // reach the fetch site.
    manifest.add_dependency("broken-pkg", "1.0.0", DependencyGroup::Optional).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    // Opt out of the GVS layout so the assertion below can stat the
    // legacy `<virtual_store_dir>/<flat-name>` slot directly. With
    // GVS on, the slot lives under `<store_dir>/links/...` and the
    // assertion would always pass regardless of whether the swallow
    // path actually fired.
    config.enable_global_virtual_store = false;
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    // Keep retries minimal — 127.0.0.1:1 fails immediately on every
    // try, but a long retry schedule would dominate the test runtime.
    config.fetch_retries = 0;
    config.minimum_release_age = None;
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(BROKEN_OPTIONAL_LOCKFILE)
        .expect("parse broken-optional fixture lockfile");

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        // The lockfile-resolution verifier is unrelated to what this test
        // exercises (the optional-tarball swallow path) and now always runs;
        // its fail-closed tarball-URL check would otherwise try to fetch
        // metadata for `broken-pkg` from the unreachable default registry and
        // abort the install before the optional-snapshot code path runs.
        // `trust_lockfile` is the opt-out that skips verification entirely.
        trust_lockfile: true,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install must NOT abort when an optional snapshot fails to fetch");

    // The broken snapshot's virtual-store slot must not have been
    // created — the cold-batch dispatch failed before extraction.
    let expected_slot = virtual_store_dir.join("broken-pkg@1.0.0").join("node_modules");
    assert!(
        !expected_slot.exists(),
        "broken optional snapshot's slot must not exist, found {expected_slot:?}",
    );

    // The fetch-failure entry must NOT have been persisted to
    // `.modules.yaml.skipped`. Mirrors upstream's silent catch site
    // that never updates `opts.skipped`, so a future install retries
    // the fetch (in case the URL becomes reachable again).
    let written = modules_dir
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read .modules.yaml")
        .expect("modules manifest exists");
    assert!(
        written.skipped.is_empty(),
        "fetch-failure entries must not land in .modules.yaml.skipped, got {:?}",
        written.skipped,
    );

    drop(dir);
}

/// The fetch-failure swallow is gated on `snapshot.optional`. A
/// non-optional snapshot whose tarball is unreachable must still
/// abort the install — mirrors upstream's `if (pkgSnapshot.optional)
/// return; throw err;` at
/// [`lockfileToDepGraph.ts:296-298`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L296-L298).
/// Same fixture as the swallow test but with `optional: true`
/// removed from the snapshot entry — confirms the polarity is
/// correct.
#[tokio::test]
async fn frozen_install_propagates_non_optional_fetch_failure() {
    const NON_OPTIONAL_BROKEN_LOCKFILE: &str = text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      broken-pkg:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages:"
        "  broken-pkg@1.0.0:"
        "    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA, tarball: 'http://127.0.0.1:1/broken.tgz'}"
        "snapshots:"
        "  broken-pkg@1.0.0: {}"
    };

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("broken-pkg", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    // Match the sister test's GVS-off setup so both swallow tests
    // route through the same layout — sidesteps any GVS-routing
    // path divergence affecting where the cold-batch dispatch even
    // runs.
    config.enable_global_virtual_store = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.fetch_retries = 0;
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(NON_OPTIONAL_BROKEN_LOCKFILE)
        .expect("parse non-optional broken-fixture lockfile");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    assert!(result.is_err(), "non-optional fetch failure must abort the install, got {result:?}");

    drop(dir);
}

/// Ports the `--no-optional` shape of
/// [`installing/deps-installer/test/install/optionalDependencies.ts:391`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/optionalDependencies.ts#L391)
/// and the frozen-install side of
/// [`installing/deps-restorer/test/index.ts:323`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/test/index.ts#L323).
///
/// The fixture is designed to discriminate slice 5 (`--no-optional`
/// filter) from slice 4 (fetch-failure swallow): a snapshot with
/// `optional: true` whose **metadata row is missing from
/// `packages:`**. Slice 4's swallow only covers `DownloadTarball`
/// and `GitFetch` — `MissingPackageMetadata` propagates even for
/// optional snapshots. So if slice 5's filter doesn't fire, the
/// missing-metadata error aborts the install regardless of slice
/// 4. A successful install therefore proves the snapshot was
/// dropped **before** cache-key derivation by the slice 5 gate.
///
/// Mirrors upstream's depNode filter at
/// [`link.ts:109-111`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/link.ts#L109-L111):
/// when `!include.optionalDependencies`, every depNode whose
/// `optional` flag is true is dropped from the install graph
/// before extraction / linking / building runs.
///
/// Also asserts that `--no-optional` exclusions are **not**
/// persisted to `.modules.yaml.skipped` — same convention as the
/// fetch-failure swallow (slice 4): the exclusion is transient,
/// so a later install without `--no-optional` brings the snapshot
/// back into the install graph.
#[tokio::test]
async fn frozen_install_no_optional_drops_optional_only_snapshots() {
    // Lockfile with one `optional: true` snapshot whose metadata
    // row is intentionally missing from `packages:`. Slice 4
    // (fetch-failure swallow) does NOT cover `MissingPackageMetadata`,
    // so reaching the cache-key derivation step would abort the
    // install. The `--no-optional` filter must drop the snapshot
    // before that step runs.
    const OPTIONAL_NO_METADATA_LOCKFILE: &str = text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    optionalDependencies:"
        "      drop-me:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages: {}"
        "snapshots:"
        "  drop-me@1.0.0:"
        "    optional: true"
    };

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // Manifest matches the lockfile importer entry so the
    // freshness check doesn't reject the install.
    manifest.add_dependency("drop-me", "1.0.0", DependencyGroup::Optional).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    // Opt out of GVS so the slot-path assertion targets the legacy
    // `<virtual_store_dir>/<flat-name>` layout. Same pattern as the
    // slice 4 swallow tests.
    config.enable_global_virtual_store = false;
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(OPTIONAL_NO_METADATA_LOCKFILE)
        .expect("parse optional-no-metadata fixture lockfile");

    // The dispatch list excludes `DependencyGroup::Optional` — same
    // shape `--no-optional` produces from
    // `InstallDependencyOptions::dependency_groups()` in
    // `crates/cli/src/cli_args/install.rs`.
    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install must succeed with --no-optional despite missing optional metadata");

    // The optional-only snapshot must not have been extracted.
    let expected_slot = virtual_store_dir.join("drop-me@1.0.0").join("node_modules");
    assert!(
        !expected_slot.exists(),
        "optional-only snapshot's slot must not exist, found {expected_slot:?}",
    );

    // Transient — must not bleed into the persistent
    // `.modules.yaml.skipped` set.
    let written = modules_dir
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read .modules.yaml")
        .expect("modules manifest exists");
    assert!(
        written.skipped.is_empty(),
        "--no-optional exclusions must not land in .modules.yaml.skipped, got {:?}",
        written.skipped,
    );

    drop(dir);
}

/// Polarity test for [`frozen_install_no_optional_drops_optional_only_snapshots`].
/// Same fixture, but the dispatch list **includes** `Optional`. With
/// the snapshot's metadata missing from `packages:`, the install
/// must now abort with `MissingPackageMetadata` — slice 4's
/// fetch-failure swallow doesn't cover that variant, so the
/// optional-ness alone doesn't save the install. Proves the
/// slice 5 filter is gated on the dispatch list rather than firing
/// unconditionally.
#[tokio::test]
async fn frozen_install_optional_included_surfaces_missing_metadata() {
    const OPTIONAL_NO_METADATA_LOCKFILE: &str = text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    optionalDependencies:"
        "      drop-me:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages: {}"
        "snapshots:"
        "  drop-me@1.0.0:"
        "    optional: true"
    };

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("drop-me", "1.0.0", DependencyGroup::Optional).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.enable_global_virtual_store = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(OPTIONAL_NO_METADATA_LOCKFILE)
        .expect("parse optional-no-metadata fixture lockfile");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    let err =
        result.expect_err("install must abort without --no-optional when metadata is missing");
    assert!(
        matches!(
            err,
            InstallError::FrozenLockfile(crate::InstallFrozenLockfileError::CreateVirtualStore(
                crate::CreateVirtualStoreError::MissingPackageMetadata { .. },
            ),),
        ),
        "expected FrozenLockfile(CreateVirtualStore(MissingPackageMetadata)), got {err:?}",
    );

    drop(dir);
}

/// Regression coverage for the shared-dependency case from
/// [`installing/deps-installer/test/install/optionalDependencies.ts:712`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/optionalDependencies.ts#L712)
/// (`dependency that is both optional and non-optional is installed,
/// when optional dependencies should be skipped`).
///
/// `SnapshotEntry::optional` is set by upstream's resolver only
/// when a snapshot is reachable **exclusively** through optional
/// edges. A snapshot reachable through any non-optional edge carries
/// `optional: false` and **must not** be dropped by `--no-optional`.
///
/// Fixture: a single snapshot `shared@1.0.0` with `optional: false`
/// (default) and metadata missing from `packages:`. With
/// `--no-optional`, the filter must skip this snapshot only if it
/// checks the `optional` flag — if it accidentally drops every
/// snapshot listed under `optionalDependencies` regardless of the
/// flag, the install would silently succeed (the missing-metadata
/// error wouldn't surface). Conversely, if the filter is correct,
/// the install aborts with `MissingPackageMetadata` because the
/// non-optional snapshot reaches cache-key derivation.
#[tokio::test]
async fn frozen_install_no_optional_keeps_shared_non_optional_snapshot() {
    const SHARED_NON_OPTIONAL_LOCKFILE: &str = text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    optionalDependencies:"
        "      shared:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages: {}"
        "snapshots:"
        "  shared@1.0.0: {}"
    };

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("shared", "1.0.0", DependencyGroup::Optional).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.enable_global_virtual_store = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(SHARED_NON_OPTIONAL_LOCKFILE)
        .expect("parse shared-non-optional fixture lockfile");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        // `--no-optional` shape: Optional NOT in the dispatch list.
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    let err =
        result.expect_err("snapshot with optional:false must NOT be dropped by --no-optional");
    assert!(
        matches!(
            err,
            InstallError::FrozenLockfile(crate::InstallFrozenLockfileError::CreateVirtualStore(
                crate::CreateVirtualStoreError::MissingPackageMetadata { .. },
            ),),
        ),
        "expected FrozenLockfile(CreateVirtualStore(MissingPackageMetadata)) — \
         proves the snapshot was kept; got {err:?}",
    );

    drop(dir);
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
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    std::fs::create_dir_all(&project_root).expect("create project root");
    let manifest_path = project_root.join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::Hoisted,
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("hoisted-linker install with empty lockfile should succeed");

    let written = modules_dir
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

    drop(dir);
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
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    std::fs::create_dir_all(&project_root).expect("create project root");
    let manifest_path = project_root.join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::Hoisted,
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("hoisted-linker install should succeed");

    // `<project>/node_modules/.pacquet` only gets created when
    // CreateVirtualDirBySnapshot lays down a slot. Hoisted skips
    // that helper, so the dir must remain absent.
    assert!(
        !virtual_store_dir.exists(),
        "hoisted install must not materialize the virtual-store root at {virtual_store_dir:?}",
    );

    drop(dir);
}

/// Frozen-lockfile install with a `VariationsResolution` whose
/// variants only target a platform pacquet CI never runs on
/// (`aix/ppc64`) must surface
/// [`crate::InstallPackageBySnapshotError::NoMatchingPlatformVariant`]
/// from the cold-batch dispatcher. Variant selection happens
/// before any network fetch, so the bogus URL on the variant is
/// never read — the test stays hermetic.
///
/// Closes the variant-mismatch checkbox of [#437] slice F.
///
/// [#437]: https://github.com/pnpm/pacquet/issues/437
#[tokio::test]
async fn frozen_lockfile_install_errors_when_no_variant_matches_host() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("node", "runtime:22.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    // Lockfile with a runtime entry whose variants only target a
    // platform we're not running on. `runtime:22.0.0` is preserved
    // through `PkgVerPeer`'s `Prefix::Runtime` (<https://github.com/pnpm/pacquet/issues/511> / <https://github.com/pnpm/pacquet/pull/512>); the
    // depPath round-trips correctly through pacquet's parser.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      node:"
        "        specifier: 'runtime:22.0.0'"
        "        version: 'runtime:22.0.0'"
        "packages:"
        "  'node@runtime:22.0.0':"
        "    hasBin: true"
        "    resolution:"
        "      type: variations"
        "      variants:"
        "        - resolution:"
        "            type: binary"
        "            url: 'https://example.test/node-aix-ppc64.tar.gz'"
        "            integrity: 'sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa=='"
        "            bin: 'bin/node'"
        "            archive: tarball"
        "          targets:"
        "            - os: aix"
        "              cpu: ppc64"
        "snapshots:"
        "  'node@runtime:22.0.0': {}"
    })
    .expect("parse variant-mismatch fixture lockfile");

    let err = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        resolved_packages: &Default::default(),
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect_err("variant-mismatch lockfile must surface a typed error");

    let rendered = format!("{err:?}");
    eprintln!("ERROR DEBUG:\n{rendered}");
    assert!(
        rendered.contains("NoMatchingPlatformVariant"),
        "expected NoMatchingPlatformVariant in the error chain, got: {rendered}",
    );
    let displayed = err.to_string();
    assert!(!displayed.is_empty(), "Display impl should produce a non-empty user-facing message");

    drop(dir);
}

/// Same lockfile + manifest shape as
/// [`frozen_lockfile_install_errors_when_no_variant_matches_host`],
/// but with `skip_runtimes: true`. The `--no-runtime` filter
/// iterates importer-direct deps, builds `node@runtime:22.0.0`
/// from `(alias, version)`, sees the `@runtime:` substring, and
/// adds the snapshot to the skip set — so variant selection
/// never runs and the unmatchable-platform variant doesn't fail
/// the install.
///
/// Closes the `--no-runtime` checkbox of [#437] slice F.
///
/// [#437]: https://github.com/pnpm/pacquet/issues/437
#[tokio::test]
async fn frozen_lockfile_install_skips_runtime_when_skip_runtimes_set() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("node", "runtime:22.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      node:"
        "        specifier: 'runtime:22.0.0'"
        "        version: 'runtime:22.0.0'"
        "packages:"
        "  'node@runtime:22.0.0':"
        "    hasBin: true"
        "    resolution:"
        "      type: variations"
        "      variants:"
        "        - resolution:"
        "            type: binary"
        "            url: 'https://example.test/node-aix-ppc64.tar.gz'"
        "            integrity: 'sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa=='"
        "            bin: 'bin/node'"
        "            archive: tarball"
        "          targets:"
        "            - os: aix"
        "              cpu: ppc64"
        "snapshots:"
        "  'node@runtime:22.0.0': {}"
    })
    .expect("parse --no-runtime fixture lockfile");

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: true,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        resolved_packages: &Default::default(),
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("--no-runtime should skip the unmatchable runtime entry and let the rest of the install succeed");

    // The runtime slot must NOT exist under the virtual store —
    // the snapshot was filtered out by the skip set.
    //
    // Use `symlink_metadata` rather than `Path::exists()` so a
    // *dangling* symlink fails the assertion too: `exists()`
    // follows symlinks and reports `false` for a broken one, but
    // a broken symlink at `<modules_dir>/node` would still mean
    // the install created an entry the skip set was supposed to
    // suppress.
    let runtime_slot = virtual_store_dir.join("node@runtime:22.0.0");
    assert!(
        std::fs::symlink_metadata(&runtime_slot).is_err(),
        "runtime slot should not be materialized under --no-runtime, got {runtime_slot:?}",
    );
    // Neither should the direct-dep symlink under the project's
    // `node_modules/`. Same `symlink_metadata` rationale.
    let direct_dep = modules_dir.join("node");
    assert!(
        std::fs::symlink_metadata(&direct_dep).is_err(),
        "direct-dep symlink for node should not be created under --no-runtime, got {direct_dep:?}",
    );

    drop(dir);
}

/// End-to-end wiring smoke for the lockfile-verification gate
/// (Phase 7). An invalid `minimumReleaseAgeExclude` pattern (the
/// glob form is rejected when paired with a version part, per
/// upstream's `ERR_PNPM_NAME_PATTERN_IN_VERSION_UNION` arm) trips
/// `build_resolution_verifiers` before the frozen-lockfile dispatch
/// runs. The resulting `InstallError::BuildVerifiers` proves:
///
/// 1. `build_resolution_verifiers` actually fires during install.
/// 2. The error short-circuits the install — no virtual-store
///    materialization, no registry round-trip.
///
/// The gate's positive / negative `verify_lockfile_resolutions`
/// branches are exercised by the unit tests in
/// `pacquet-lockfile-verification`; this test pins only the install
/// wiring so it stays fast and doesn't depend on the mocked
/// packument shape.
#[tokio::test]
async fn install_rejects_invalid_minimum_release_age_exclude_pattern() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    // Activate the verifier with an invalid exclude entry — the
    // version-part-with-wildcard combination is rejected by
    // `create_package_version_policy`.
    config.minimum_release_age = Some(60);
    config.minimum_release_age_exclude = Some(vec!["is-*@1.0.0".to_string()]);
    let config = config.leak();

    // Empty lockfile is enough — the gate runs as soon as
    // `lockfile.is_some()` regardless of the snapshot count.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies: {}"
        "packages: {}"
        "snapshots: {}"
    })
    .expect("parse minimal v9 lockfile");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    let err = result.expect_err("invalid exclude pattern must surface");
    assert!(matches!(err, InstallError::BuildVerifiers(_)), "expected BuildVerifiers, got {err:?}");
    // The build error must short-circuit the install before any
    // virtual-store materialization runs.
    assert!(
        !project_root.join("node_modules/.pacquet").exists(),
        "BuildVerifiers must abort before virtual-store materialization",
    );

    drop(dir);
}

/// Positive-path proof that `verify_lockfile_resolutions` runs from
/// inside `Install::run`. With `minimumReleaseAge` set absurdly high
/// (100 years), every version the mocked registry knows about is
/// inside the cutoff, so the gate rejects every lockfile entry
/// before any tarball is fetched.
///
/// Asserts:
///
/// 1. `Install::run` returns `Err(InstallError::LockfileVerification(...))`
///    with the inner `VerifyError::MinimumReleaseAgeViolation` —
///    i.e. the verifier code path actually ran and returned a
///    violation, the wiring is correct, and the dispatch maps the
///    inner code to the per-policy variant rather than collapsing
///    to the generic envelope.
/// 2. No virtual-store materialization. The gate fails before
///    `InstallFrozenLockfile` runs, so neither the slot nor the
///    project's `node_modules` symlink exist.
#[tokio::test]
async fn frozen_lockfile_gate_rejects_under_huge_minimum_release_age() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.registry = mock_instance.url();
    // 100 years in minutes. Anything the registry has shipped to
    // date is inside the cutoff, so the publish-time check rejects
    // every lockfile entry regardless of what the mocked packument's
    // `time` map actually says.
    config.minimum_release_age = Some(60 * 24 * 365 * 100);
    let config = config.leak();

    // The integrity hash here is placeholder text — the gate fails
    // before the tarball is fetched, so checksum verification never
    // runs and the value doesn't have to match the mock's actual
    // payload. The lockfile only needs to deserialize.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      '@pnpm.e2e/hello-world-js-bin':"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages:"
        "  '@pnpm.e2e/hello-world-js-bin@1.0.0':"
        "    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}"
        "snapshots:"
        "  '@pnpm.e2e/hello-world-js-bin@1.0.0': {}"
    })
    .expect("parse lockfile fixture");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    let err = result.expect_err("100-year cutoff must reject every entry");
    let InstallError::LockfileVerification(ref verify_err) = err else {
        panic!("expected InstallError::LockfileVerification, got {err:?}");
    };
    assert!(
        matches!(
            verify_err,
            pacquet_lockfile_verification::VerifyError::MinimumReleaseAgeViolation { .. }
        ),
        "expected MinimumReleaseAgeViolation, got {verify_err:?}",
    );

    // The gate must short-circuit before any virtual-store
    // materialization — no slot, no project-side symlink.
    let slot = project_root.join("node_modules/.pacquet/@pnpm.e2e+hello-world-js-bin@1.0.0");
    assert!(!slot.exists(), "the gate must fail before any virtual-store materialization");
    assert!(
        !project_root.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "the gate must fail before any project-side symlinks are created",
    );

    drop((dir, mock_instance));
}

// ----------------------------------------------------------------------------
// Fresh-install lockfile generation
//
// These tests port the parts of
// [`installing/deps-installer/test/lockfile.ts`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-installer/test/lockfile.ts)
// that exercise a *fresh* install — the path that converts the
// resolver's `DependenciesGraph` into a v9 `pnpm-lock.yaml`. Tests that
// require an existing lockfile (incremental update, repeat install,
// `--frozen-lockfile`-with-stale-lockfile) stay deferred — see issue
// pnpm/pnpm#11813 for the broader scope.

/// Pacquet equivalent of upstream's
/// ["lockfile has correct format"](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-installer/test/lockfile.ts#L38)
/// for the smallest fresh-install slice: one direct prod dep produces
/// a v9 lockfile with the right `lockfileVersion`, an importer entry
/// under `.`, and matching `packages:` / `snapshots:` rows.
#[tokio::test]
async fn fresh_install_writes_pnpm_lock_yaml_with_expected_shape() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    assert!(lockfile_path.is_file(), "pnpm-lock.yaml must be written next to the manifest");

    let content = std::fs::read_to_string(&lockfile_path).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&content).expect("parse fresh lockfile");

    assert_eq!(lockfile.lockfile_version.major, 9);
    let importer = lockfile.root_project().expect("root importer recorded");
    let deps = importer.dependencies.as_ref().expect("dependencies map");
    let hello_key: pacquet_lockfile::PkgName =
        pacquet_lockfile::PkgName::parse("@pnpm.e2e/hello-world-js-bin").unwrap();
    let entry = deps.get(&hello_key).expect("hello-world-js-bin recorded");
    assert_eq!(entry.specifier, "1.0.0");

    let packages = lockfile.packages.as_ref().expect("packages map populated");
    let pkg_key: pacquet_lockfile::PackageKey =
        "@pnpm.e2e/hello-world-js-bin@1.0.0".parse().unwrap();
    let metadata = packages.get(&pkg_key).expect("packages entry");
    assert!(metadata.resolution.integrity().is_some(), "registry resolution carries integrity");

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map populated");
    assert!(
        snapshots.contains_key(&pkg_key),
        "snapshot keyed by depPath (pure pkg id when no peers)",
    );

    drop((dir, mock_instance));
}

/// Manifest-declared dependency groups land in the matching importer
/// section in the lockfile. Mirrors upstream's
/// ["packages are placed in devDependencies even if they are present as
/// non-dev as well"](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-installer/test/lockfile.ts#L559)
/// at the surface level — pacquet routes deps through
/// `manifest_alias_to_group`, so a dep declared in `devDependencies`
/// lands in the lockfile's `devDependencies` section.
#[tokio::test]
async fn fresh_install_splits_dev_and_prod_dependency_sections() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

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
    config.virtual_store_dir = virtual_store_dir;
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    let content = std::fs::read_to_string(&lockfile_path).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&content).expect("parse fresh lockfile");

    let importer = lockfile.root_project().expect("root importer");
    let prod = importer.dependencies.as_ref().expect("prod section");
    let hello_key = pacquet_lockfile::PkgName::parse("@pnpm.e2e/hello-world-js-bin").unwrap();
    let xyz_key = pacquet_lockfile::PkgName::parse("@pnpm/xyz").unwrap();
    assert!(prod.contains_key(&hello_key));
    assert!(!prod.contains_key(&xyz_key), "dev dep stays out of the prod section");
    let dev = importer.dev_dependencies.as_ref().expect("dev section");
    assert!(dev.contains_key(&xyz_key));

    drop((dir, mock_instance));
}

/// Specifiers recorded into each importer-level entry mirror the
/// user-written `package.json` value, not the resolved version.
/// Mirrors the per-entry `specifier:` check in upstream's
/// ["lockfile has correct format"](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-installer/test/lockfile.ts#L38)
/// — a `^1.0.0` declared spec round-trips through the lockfile as
/// `specifier: ^1.0.0` even when the resolved version is exact
/// (`1.0.0`).
#[tokio::test]
async fn fresh_install_records_user_written_specifier() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "^1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    let content = std::fs::read_to_string(&lockfile_path).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&content).expect("parse fresh lockfile");

    let importer = lockfile.root_project().expect("root importer");
    let deps = importer.dependencies.as_ref().expect("prod deps");
    let key = pacquet_lockfile::PkgName::parse("@pnpm.e2e/hello-world-js-bin").unwrap();
    let entry = deps.get(&key).expect("hello-world-js-bin entry");
    assert_eq!(entry.specifier, "^1.0.0", "specifier must echo the manifest declaration");

    drop((dir, mock_instance));
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

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    let first = std::fs::read_to_string(&lockfile_path).expect("read first");
    let parsed: Lockfile = serde_saphyr::from_str(&first).expect("parse first");
    let second_path = dir.path().join("pnpm-lock.round-trip.yaml");
    parsed.save_to_path(&second_path).expect("save round-trip lockfile");
    let second = std::fs::read_to_string(&second_path).expect("read second");
    let reparsed: Lockfile = serde_saphyr::from_str(&second).expect("parse second");

    assert_eq!(parsed, reparsed, "lockfile round-trip must preserve every field");

    drop((dir, mock_instance));
}

/// `config.lockfile = false` opt-out skips the lockfile write but
/// keeps the install running. Mirrors upstream's
/// ["lockfile is ignored when lockfile = false"](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-installer/test/lockfile.ts#L660):
/// no `pnpm-lock.yaml` on disk, but `node_modules/` materialized.
#[tokio::test]
async fn fresh_install_with_lockfile_disabled_does_not_write_a_lockfile() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    assert!(
        !lockfile_path.exists(),
        "config.lockfile = false must suppress the write (file should not exist)",
    );
    // Sanity: materialization still happened.
    assert!(
        project_root.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "node_modules must still be populated even when the lockfile is skipped",
    );

    drop((dir, mock_instance));
}

/// A fresh install also writes `<virtual_store_dir>/lock.yaml` so the
/// next install's slot-skip optimization has something to diff
/// against. Mirrors the upstream
/// [`writeCurrentLockfile`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/fs/src/write.ts#L41-L51)
/// call at the tail of the install pipeline. The contents round-trip
/// through `Lockfile`, so a subsequent `pacquet install
/// --frozen-lockfile` can read it back without a parse error.
#[tokio::test]
async fn fresh_install_also_writes_current_lockfile_under_virtual_store() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    let current_lockfile_path = virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
    assert!(
        current_lockfile_path.is_file(),
        "current-lockfile must be written under the virtual store dir",
    );

    let content = std::fs::read_to_string(&current_lockfile_path).expect("read current lockfile");
    let current_lockfile: Lockfile =
        serde_saphyr::from_str(&content).expect("parse current lockfile");
    assert_eq!(current_lockfile.lockfile_version.major, 9);
    let importer = current_lockfile.root_project().expect("root importer");
    let key = pacquet_lockfile::PkgName::parse("@pnpm.e2e/hello-world-js-bin").unwrap();
    assert!(
        importer.dependencies.as_ref().is_some_and(|deps| deps.contains_key(&key)),
        "current-lockfile reflects the resolved direct dep",
    );

    // The wanted-lockfile and the current-lockfile describe the same
    // resolved graph in the fresh-install path (no install-time skip
    // set to filter against), so the two files should parse to the
    // same shape.
    let wanted_path = dir.path().join(Lockfile::FILE_NAME);
    let wanted_content = std::fs::read_to_string(&wanted_path).expect("read wanted lockfile");
    let wanted_lockfile: Lockfile =
        serde_saphyr::from_str(&wanted_content).expect("parse wanted lockfile");
    assert_eq!(
        wanted_lockfile, current_lockfile,
        "wanted and current lockfiles must match in the fresh-install path",
    );

    drop((dir, mock_instance));
}

/// `config.lockfile = false` opts out of *both* lockfile writes (the
/// wanted `pnpm-lock.yaml` and the per-virtual-store `lock.yaml`),
/// matching upstream pnpm's all-or-nothing `useLockfile` behavior.
#[tokio::test]
async fn fresh_install_with_lockfile_disabled_skips_current_lockfile_too() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    assert!(
        !virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME).exists(),
        "current-lockfile must also be skipped when config.lockfile = false",
    );

    drop((dir, mock_instance));
}

/// A top-level `optionalDependencies` entry surfaces as
/// `snapshots[<key>].optional: true` in the freshly-written lockfile.
/// Mirrors upstream's `ResolvedPackage.optional` propagation that
/// `BuildModules` consults to decide whether a build failure should
/// be reported via `pnpm:skipped-optional-dependency`. A non-optional
/// sibling lands `optional: false` so the test pins both sides.
#[tokio::test]
async fn fresh_install_marks_optional_snapshots_in_pnpm_lock_yaml() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.add_dependency("@pnpm/xyz", "1.0.0", DependencyGroup::Optional).unwrap();
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    let content = std::fs::read_to_string(&lockfile_path).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&content).expect("parse lockfile");
    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map");

    // Look snapshots up by package name + version rather than by an
    // exact key parse: `@pnpm/xyz` declares peer deps, so the resolver
    // emits a peer-suffixed depPath that won't parse cleanly out of a
    // literal here. Membership-by-name keeps the test robust to the
    // fixture's exact peer-suffix shape.
    let find_optional = |scope: &str, bare: &str| -> Option<bool> {
        snapshots
            .iter()
            .find(|(key, _)| key.name.scope.as_deref() == Some(scope) && key.name.bare == bare)
            .map(|(_, entry)| entry.optional)
    };

    assert_eq!(
        find_optional("pnpm.e2e", "hello-world-js-bin"),
        Some(false),
        "non-optional direct dep must land with optional: false",
    );
    assert_eq!(
        find_optional("pnpm", "xyz"),
        Some(true),
        "optionalDependencies entry must propagate to snapshots[<key>].optional",
    );
    // Note: transitive deps that arrive via auto-install-peers
    // hoisting land at the importer level as non-optional — they're
    // installed top-level to satisfy a missing peer regardless of
    // whether the consumer was optional. Matches upstream pnpm's
    // hoist semantics. Pure transitive optional propagation
    // (consumer → regular `dependencies` child) is exercised by the
    // adapter's unit tests since the mock-registry fixtures here
    // don't expose that shape.

    drop((dir, mock_instance));
}

/// `nodeLinker: hoisted` on the fresh-lockfile path (no lockfile,
/// not frozen) installs successfully and records the hoisted linker
/// in `.modules.yaml`. With an empty manifest there is nothing to
/// materialize, so the assertion focuses on the dispatch reaching
/// the hoisted-linker pipeline rather than bailing — the previous
/// hard-refusal at this site ([#11871](https://github.com/pnpm/pnpm/issues/11871)) is gone.
#[tokio::test]
async fn fresh_install_hoisted_node_linker_records_modules_yaml() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    std::fs::create_dir_all(&project_root).expect("create project root");
    let manifest_path = project_root.join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::Hoisted,
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("fresh hoisted-linker install should succeed");

    let written = modules_dir
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
        !virtual_store_dir.exists(),
        "hoisted install must not materialize the virtual-store root at {virtual_store_dir:?}",
    );

    drop(dir);
}

/// `--no-runtime` (`config.skip_runtimes = true`) on the fresh path
/// is refused for the same reason: pacquet's runtime filter runs only
/// inside the frozen-lockfile path, so honoring the flag on a fresh
/// install would need a port of upstream's runtime-snapshot filter.
#[tokio::test]
async fn fresh_install_refuses_skip_runtimes_before_writing_state() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: true,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    assert!(matches!(result, Err(InstallError::UnsupportedFreshInstallSkipRuntimes)));
    assert!(!dir.path().join(Lockfile::FILE_NAME).exists(), "no wanted lockfile written");
    assert!(!virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME).exists(), "no current lockfile");
    assert!(!modules_dir.join(".modules.yaml").exists(), "no modules manifest");

    drop(dir);
}

/// Dispatch state 2: no `--frozen-lockfile` flag, lockfile present and
/// fresh, `preferFrozenLockfile: true` (the default) → auto-frozen.
/// We prove the frozen path was taken the same way
/// `warm_reinstall_skips_snapshot_when_current_lockfile_matches` does:
/// the lockfile points at a bogus tarball URL, and the install is
/// pre-seeded with a matching current lockfile + virtual-store slot,
/// so only the snapshot-skip path inside the frozen install can
/// produce a successful run. If the dispatch silently fell through to
/// the fresh-resolve path, the bogus URL would be fetched and the
/// install would error out.
#[tokio::test]
async fn prefer_frozen_lockfile_takes_frozen_path_when_lockfile_is_fresh() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("placeholder", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    // Same legacy-layout opt-out as the sibling skip test — the seed
    // helper writes the flat-name slot shape.
    config.enable_global_virtual_store = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

    std::fs::create_dir_all(&virtual_store_dir).unwrap();
    lockfile.save_current_to_virtual_store_dir(&virtual_store_dir).expect("seed current lockfile");
    seed_placeholder_virtual_store_slot(&virtual_store_dir);

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        // No `--frozen-lockfile`; the dispatch must auto-go-frozen
        // via `config.prefer_frozen_lockfile` (defaults to `true`).
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: true, // fixture pins a tripwire tarball URL; skip resolution verification so the tarball-URL check doesn't flag it before the path under test
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect(
        "auto-frozen dispatch must short-circuit the bogus fetch via the skip path \
         (would otherwise error out on the invalid URL)",
    );

    drop(dir);
}

/// Dispatch state 3a: lockfile present + matching manifest, but
/// `Install::prefer_frozen_lockfile = Some(false)` (the CLI's
/// `--no-prefer-frozen-lockfile` opt-out). The dispatch must route to
/// the fresh-resolve path even though the frozen fast path would have
/// applied. We prove it by pointing at an unreachable registry: the
/// fresh-resolve path will hit the resolver and fail, whereas the
/// frozen fast path would short-circuit the network entirely via the
/// skip cache.
#[tokio::test]
async fn no_prefer_frozen_lockfile_flag_forces_fresh_resolve() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("placeholder", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    // Force the resolver onto an unreachable registry so the
    // fresh-resolve path errors out clearly; the frozen path would
    // never consult the registry at all.
    config.registry = "http://invalid.local/".to_string();
    config.enable_global_virtual_store = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

    // Seed exactly as the auto-frozen test does — if dispatch did go
    // frozen, the skip cache would carry the install to success.
    std::fs::create_dir_all(&virtual_store_dir).unwrap();
    lockfile.save_current_to_virtual_store_dir(&virtual_store_dir).expect("seed current lockfile");
    seed_placeholder_virtual_store_slot(&virtual_store_dir);

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        // Opt-out at the call site (the `--no-prefer-frozen-lockfile`
        // CLI flag would land here).
        prefer_frozen_lockfile: Some(false),
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    let err = result.expect_err(
        "fresh-resolve dispatch must consult the unreachable registry and fail; \
         a success would mean the dispatch silently took the frozen fast path",
    );
    assert!(
        !matches!(err, InstallError::OutdatedLockfile { .. }),
        "fresh-resolve fall-through must not surface as OutdatedLockfile, got {err:?}",
    );
}

/// Dispatch state 3b: lockfile present, but the manifest has drifted
/// from it; no `--frozen-lockfile` flag. The freshness gate inside the
/// auto-frozen branch must fail, and the dispatch must fall through
/// to the fresh-resolve path instead of surfacing `OutdatedLockfile`
/// the way state 1 would. We assert via the same "unreachable
/// registry" sentinel as the previous test.
#[tokio::test]
async fn stale_lockfile_under_no_flag_falls_through_to_fresh_resolve() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
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
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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

/// [`super::is_modules_yaml_consistent`] returns `false` when
/// `.modules.yaml` is missing, so a first install (no prior state)
/// can't be mistaken for an up-to-date install.
#[test]
fn is_modules_yaml_consistent_returns_false_when_modules_yaml_absent() {
    let dir = tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");
    let mut config = Config::new();
    config.modules_dir = modules_dir.clone();
    let config = config.leak();

    assert!(!super::is_modules_yaml_consistent(
        &modules_dir,
        config,
        pacquet_config::NodeLinker::default(),
        pacquet_modules_yaml::IncludedDependencies::default(),
    ));
}

/// [`super::is_modules_yaml_consistent`] returns `true` when every
/// layout-determining setting matches what
/// [`super::build_modules_manifest`] would write for the current
/// config / linker / dependency-group selection. The roundtrip needs
/// to be exact because a single drifted setting forces the next
/// install to rebuild the modules directory.
#[test]
fn is_modules_yaml_consistent_returns_true_when_settings_match() {
    let dir = tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    let included = pacquet_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: true,
    };

    let seed = Modules {
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
    write_modules_manifest::<Host>(&modules_dir, seed).expect("seed .modules.yaml");

    assert!(super::is_modules_yaml_consistent(
        &modules_dir,
        config,
        pacquet_config::NodeLinker::default(),
        included,
    ));
}

/// `nodeLinker` drift between `.modules.yaml` and the current config
/// disqualifies the up-to-date short-circuit. Mirrors upstream's
/// `validateModules` behavior — a different linker forces a full
/// rebuild of `node_modules` rather than a fast no-op.
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

    assert!(!super::is_modules_yaml_consistent(
        &modules_dir,
        config,
        pacquet_config::NodeLinker::Isolated,
        pacquet_modules_yaml::IncludedDependencies::default(),
    ));
}

/// Dependency-group drift between `.modules.yaml.included` and the
/// current install request disqualifies the short-circuit. A
/// previously installed `--no-optional` setup can't be re-used to
/// satisfy an install that needs optional dependencies.
#[test]
fn is_modules_yaml_consistent_returns_false_when_included_drifts() {
    let dir = tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    let prod_only = pacquet_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };
    let with_optional = pacquet_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: true,
    };

    let seed = Modules {
        layout_version: Some(LayoutVersion),
        node_linker: Some(NodeLinker::Isolated),
        included: prod_only,
        hoist_pattern: config.hoist_pattern.clone(),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        store_dir: config.store_dir.display().to_string(),
        virtual_store_dir: config.effective_virtual_store_dir().to_string_lossy().into_owned(),
        virtual_store_dir_max_length: config.virtual_store_dir_max_length,
        ..Default::default()
    };
    write_modules_manifest::<Host>(&modules_dir, seed).expect("seed .modules.yaml");

    assert!(!super::is_modules_yaml_consistent(
        &modules_dir,
        config,
        pacquet_config::NodeLinker::Isolated,
        with_optional,
    ));
}

/// End-to-end: when `.modules.yaml`, `<virtual_store_dir>/lock.yaml`,
/// and the wanted lockfile all agree, [`Install::run`] must emit the
/// `name: "pnpm"` "Lockfile is up to date" log and return without
/// running materialization. Mirrors upstream pnpm's
/// `allProjectsAreUpToDate` + `validateModules` short-circuit at
/// <https://github.com/pnpm/pnpm/blob/a456dc78fb/installing/deps-installer/src/install/index.ts#L913-L985>.
#[tokio::test]
async fn frozen_install_short_circuits_when_modules_and_lockfile_are_consistent() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    std::fs::create_dir_all(&project_root).expect("create project root");
    let manifest_path = project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // A sibling `link:` dependency keeps the lockfile non-empty without
    // requiring registry fetches — the gate fires on the eligibility
    // checks alone, materialization is never reached so the
    // (non-existent) link target doesn't matter.
    manifest.add_dependency("sibling", "link:../sibling", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
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

    let included = pacquet_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };

    // Seed the on-disk state a previous install would have left.
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
    write_modules_manifest::<Host>(&modules_dir, seed_modules).expect("seed .modules.yaml");
    lockfile.save_current_to_virtual_store_dir(&virtual_store_dir).expect("seed current lockfile");

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: true,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::Isolated,
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<RecordingReporter>()
    .await
    .expect("up-to-date install should succeed via the short-circuit");

    let captured = EVENTS.lock().unwrap();
    assert!(
        captured.iter().any(|event| matches!(
            event,
            LogEvent::Pnpm(log)
                if log.message == "Lockfile is up to date, resolution step is skipped"
        )),
        r#"the `name: "pnpm"` up-to-date log must be emitted when the install short-circuits"#,
    );

    assert!(
        captured.iter().any(|e| matches!(e, LogEvent::Stage(s) if s.stage == Stage::ImportingDone)),
        "ImportingDone must close the importing bracket on the fast path",
    );
    assert!(
        captured.iter().any(|e| matches!(e, LogEvent::Summary(_))),
        "Summary must fire so `pnpm:root` history renders even on the fast path",
    );

    // `Path::exists()` follows symlinks and returns `false` for a
    // broken target — `symlink_metadata` is the right call to detect
    // the symlink entry itself, so a dangling sibling symlink would
    // still fail this assertion. Materialization on the regular
    // install path always creates the link before falling through.
    let sibling_link = modules_dir.join("sibling");
    assert!(
        std::fs::symlink_metadata(&sibling_link).is_err(),
        "the link: dep must NOT be materialized when the gate fires; \
         a present {sibling_link:?} would mean the install ran the full pipeline",
    );

    // Workspace state is still refreshed so the next `pnpm run`'s
    // `verifyDepsBeforeRun` doesn't fire spuriously.
    let written = load_workspace_state(&project_root)
        .expect("read workspace state")
        .expect("workspace state must be written");
    assert!(
        written.last_validated_timestamp > 0,
        "last_validated_timestamp must be refreshed on the fast path",
    );

    drop(dir);
}

/// Port of pnpm's `optimisticRepeatInstall` short-circuit
/// (`installing/commands/src/installDeps.ts:179-194`). When nothing
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

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    std::fs::create_dir_all(&project_root).expect("create project root");
    std::fs::create_dir_all(&modules_dir).expect("create modules dir so the deps gate passes");
    let manifest_path = project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest.add_dependency("sibling", "link:../sibling", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    // Single-project optimistic-repeat-install requires `pnpm-lock.yaml`
    // on disk (matching pnpm's
    // <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L396-L401>
    // `throwLockfileNotFound`). Write a minimal v9 lockfile next to
    // the manifest so the freshness gate passes — the fast path only
    // checks existence, not contents.
    std::fs::write(project_root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n")
        .expect("seed pnpm-lock.yaml");

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
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

    let included = pacquet_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };

    // Seed `.modules.yaml` and the workspace state so the optimistic
    // check sees a previous install. The `last_validated_timestamp`
    // gets set to a slightly-future value to defeat any
    // mtime-clock-skew between the manifest write above and the
    // workspace-state write below.
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
    write_modules_manifest::<Host>(&modules_dir, seed_modules).expect("seed .modules.yaml");

    let mut projects = std::collections::BTreeMap::new();
    projects.insert(
        project_root.to_string_lossy().into_owned(),
        workspace_state::ProjectEntry {
            name: Some("project".to_string()),
            version: Some("1.0.0".to_string()),
        },
    );
    let settings = crate::optimistic_repeat_install::current_settings(
        config,
        pacquet_config::NodeLinker::Isolated,
        included,
    );
    workspace_state::update_workspace_state(
        &project_root,
        &pacquet_workspace_state::WorkspaceState {
            last_validated_timestamp: pacquet_workspace_state::now_millis() + 60_000,
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
        lockfile: Some(&lockfile),
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
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::Isolated,
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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
        "expected `name: \"pnpm\" / level: \"info\"` 'Already up to date' log; got events: {captured:#?}",
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

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    std::fs::create_dir_all(&project_root).expect("create project root");
    let manifest_path = project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("sibling", "link:../sibling", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
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

    let included = pacquet_modules_yaml::IncludedDependencies {
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
    write_modules_manifest::<Host>(&modules_dir, seed_modules).expect("seed .modules.yaml");
    lockfile.save_current_to_virtual_store_dir(&virtual_store_dir).expect("seed current lockfile");

    let mut projects = std::collections::BTreeMap::new();
    projects.insert(
        project_root.to_string_lossy().into_owned(),
        workspace_state::ProjectEntry {
            name: Some("project".to_string()),
            version: Some("1.0.0".to_string()),
        },
    );
    let settings = crate::optimistic_repeat_install::current_settings(
        config,
        pacquet_config::NodeLinker::Isolated,
        included,
    );
    workspace_state::update_workspace_state(
        &project_root,
        &pacquet_workspace_state::WorkspaceState {
            last_validated_timestamp: pacquet_workspace_state::now_millis() + 60_000,
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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        // The only difference vs the optimistic test above.
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: true,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::Isolated,
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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

/// Regression: a single-project install with NO lockfile anywhere —
/// `pnpm-lock.yaml` is gone and the virtual store has no current
/// `lock.yaml` to stand in for it — must NOT short-circuit, even when
/// `node_modules` and the workspace-state file survive. There is
/// nothing to content-check the manifests against and nothing to
/// regenerate `pnpm-lock.yaml` from, so the full install must run.
/// Mirrors pnpm's [`throwLockfileNotFound`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L396-L401)
/// converting into `upToDate: false`. When the current lockfile IS
/// present, the fast path instead treats it as the wanted lockfile —
/// see `regenerates_missing_wanted_lockfile_from_current_when_manifests_unchanged`
/// in the `optimistic_repeat_install` tests. Companion to the
/// workspace-mode tolerance proved by
/// [`returns_up_to_date_in_workspace_mode_without_lockfile`](crate::optimistic_repeat_install::tests::returns_up_to_date_in_workspace_mode_without_lockfile).
#[tokio::test]
async fn optimistic_repeat_install_does_not_short_circuit_when_lockfile_missing() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    std::fs::create_dir_all(&project_root).expect("create project root");
    std::fs::create_dir_all(&modules_dir).expect("create modules dir so the deps gate passes");
    let manifest_path = project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest.add_dependency("sibling", "link:../sibling", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    // Deliberately do NOT write `pnpm-lock.yaml` and do NOT seed a
    // current `lock.yaml` in the virtual store — that's the scenario
    // under test.

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    let included = pacquet_modules_yaml::IncludedDependencies {
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
    write_modules_manifest::<Host>(&modules_dir, seed_modules).expect("seed .modules.yaml");

    let mut projects = std::collections::BTreeMap::new();
    projects.insert(
        project_root.to_string_lossy().into_owned(),
        workspace_state::ProjectEntry {
            name: Some("project".to_string()),
            version: Some("1.0.0".to_string()),
        },
    );
    let settings = crate::optimistic_repeat_install::current_settings(
        config,
        pacquet_config::NodeLinker::Isolated,
        included,
    );
    workspace_state::update_workspace_state(
        &project_root,
        &pacquet_workspace_state::WorkspaceState {
            last_validated_timestamp: pacquet_workspace_state::now_millis() + 60_000,
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::Isolated,
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    std::fs::create_dir_all(&project_root).expect("create project root");
    let manifest_path = project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("first install must succeed");

    // Sanity check the first install left both artifacts the
    // optimistic check keys off on disk.
    assert!(
        project_root.join("pnpm-lock.yaml").exists(),
        "first install must write pnpm-lock.yaml next to the manifest",
    );
    assert!(
        load_workspace_state(&project_root).expect("read workspace state").is_some(),
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
        // Don't pass the in-memory lockfile — the optimistic check
        // doesn't need it, and we want to prove the fast path runs
        // *before* the lockfile is even loaded. (Matching pnpm's
        // dispatch ordering: `checkDepsStatus` runs before any
        // lockfile parse.)
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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

    drop((dir, mock_instance));
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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
    let forced_mtime = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&manifest_path)
        .expect("open package.json")
        .set_times(std::fs::FileTimes::new().set_modified(forced_mtime))
        .expect("force package.json mtime");
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
        lockfile: Some(&wanted_lockfile),
        lockfile_path: Some(&lockfile_path),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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
    assert!(
        !captured.iter().any(|event| matches!(event, LogEvent::LockfileVerification(_))),
        "verification cache hit must skip the lockfile-verification fan-out; got {captured:#?}",
    );

    drop(dir);
}

/// Shared setup for the offline repeat-install regression tests below:
/// a real install against the mock registry, after which the registry
/// is dropped and the packument cache is wiped. Any code path that
/// falls off the optimistic fast path — the resolver, the
/// lockfile-verification fan-out, a tarball fetch — would have to
/// reach the dead `127.0.0.1:9` registry and fail the install, so the
/// `expect` on the second run is the regression tripwire for the
/// repeat-install optimizations (the benchmarks don't run in CI; these
/// tests are what pins the "zero network, zero pipeline" property).
async fn install_then_go_offline() -> (tempfile::TempDir, &'static Config, PackageManifest) {
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("first install must succeed");

    drop(mock_instance);
    // The benchmark harness wipes `~/.cache/pnpm` (packument cache +
    // `lockfile-verified.jsonl`) before every run; do the same so a
    // regression can't hide behind a cache hit.
    std::fs::remove_dir_all(&cache_dir).expect("wipe the cache dir");

    let mut offline_config = Config::new();
    offline_config.cache_dir = cache_dir;
    offline_config.store_dir = store_dir.into();
    offline_config.modules_dir = modules_dir;
    offline_config.virtual_store_dir = virtual_store_dir;
    offline_config.registry = "http://127.0.0.1:9/".to_string();
    let offline_config = offline_config.leak();

    (dir, offline_config, manifest)
}

/// Rewrite `package.json` with identical content but a strictly newer
/// mtime — the shape the vlt.sh benchmark prepare step (`npm pkg
/// delete`, `touch`) produces before every timed run.
fn touch_manifest(manifest: &PackageManifest) -> PackageManifest {
    let manifest_path = manifest.path().to_path_buf();
    let manifest_text = std::fs::read_to_string(&manifest_path).expect("read package.json");
    std::fs::write(&manifest_path, manifest_text).expect("refresh package.json mtime");
    let forced_mtime = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&manifest_path)
        .expect("open package.json")
        .set_times(std::fs::FileTimes::new().set_modified(forced_mtime))
        .expect("force package.json mtime");
    PackageManifest::from_path(manifest_path).expect("reload manifest")
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
        lockfile: Some(&wanted_lockfile),
        lockfile_path: Some(&lockfile_path),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
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

#[tokio::test]
async fn fresh_lockfile_resolves_catalog_protocol_in_overrides() {
    let (_dir, lockfile) = fresh_lockfile_only_with_overrides(
        &[("@pnpm.e2e/foo", "^100.0.0")],
        &[("@pnpm.e2e/foo@^100.0.0", "catalog:")],
        Some("catalog:\n  '@pnpm.e2e/foo': '100.0.0'\n"),
    )
    .await;

    assert_package_present(&lockfile, "@pnpm.e2e/foo@100.0.0");
    assert_package_absent(&lockfile, "@pnpm.e2e/foo@100.1.0");
    assert_eq!(
        lockfile
            .overrides
            .as_ref()
            .and_then(|overrides| overrides.get("@pnpm.e2e/foo@^100.0.0"))
            .map(String::as_str),
        Some("100.0.0"),
    );
}

async fn fresh_lockfile_only_with_overrides(
    dependencies: &[(&str, &str)],
    overrides: &[(&str, &str)],
    workspace_yaml: Option<&str>,
) -> (tempfile::TempDir, Lockfile) {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    if let Some(workspace_yaml) = workspace_yaml {
        std::fs::write(dir.path().join("pnpm-workspace.yaml"), workspace_yaml).unwrap();
    }
    let store_dir = dir.path().join("pacquet-store");
    let modules_dir = dir.path().join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    for (name, spec) in dependencies {
        manifest.add_dependency(name, spec, DependencyGroup::Prod).unwrap();
    }
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.registry = mock_instance.url();
    if !overrides.is_empty() {
        let mut map = indexmap::IndexMap::new();
        for (selector, spec) in overrides {
            map.insert((*selector).to_string(), (*spec).to_string());
        }
        config.overrides = Some(map);
    }
    let config = config.leak();

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: Some(false),
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: true,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("lockfile-only install should succeed");

    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    let content = std::fs::read_to_string(lockfile_path).expect("read lockfile");
    let lockfile = serde_saphyr::from_str(&content).expect("parse lockfile");
    (dir, lockfile)
}

fn assert_package_present(lockfile: &Lockfile, key: &str) {
    let key: pacquet_lockfile::PackageKey = key.parse().unwrap();
    assert!(
        lockfile.packages.as_ref().is_some_and(|packages| packages.contains_key(&key)),
        "expected packages to contain {key}",
    );
}

fn assert_package_absent(lockfile: &Lockfile, key: &str) {
    let key: pacquet_lockfile::PackageKey = key.parse().unwrap();
    assert!(
        lockfile.packages.as_ref().is_none_or(|packages| !packages.contains_key(&key)),
        "expected packages not to contain {key}",
    );
}

/// `packageExtensions` adds entries to a dependency's manifest at
/// resolve time and the resulting lockfile records the merged shape.
///
/// Ports the spirit of
/// [`packageExtensions.ts:16`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/test/install/packageExtensions.ts#L16)
/// `manifests are extended with fields specified by packageExtensions`.
/// Covers both the resolution-side effect (the extension's
/// `peerDependencies` entry must land in the package's lockfile
/// metadata) and the lockfile-side `packageExtensionsChecksum` write
/// (the prefixed `sha256-…` checksum must be recorded so a subsequent
/// frozen install can detect drift via
/// [`crate::FreshnessCheckError::Stale`]).
#[tokio::test]
async fn fresh_install_applies_package_extensions_to_dependency_manifest() {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    config.registry = mock_instance.url();
    // Add a `peerDependencies` entry to the resolved manifest of
    // `@pnpm.e2e/hello-world-js-bin`, marked optional so the missing
    // peer never escalates to a fetch error during this minimal test.
    let mut peers = std::collections::BTreeMap::new();
    peers.insert("synthetic-peer".to_string(), "*".to_string());
    let mut peers_meta = std::collections::BTreeMap::new();
    peers_meta.insert(
        "synthetic-peer".to_string(),
        pacquet_config::PeerDependencyMeta { optional: Some(true) },
    );
    let mut extensions = indexmap::IndexMap::new();
    extensions.insert(
        "@pnpm.e2e/hello-world-js-bin".to_string(),
        pacquet_config::PackageExtension {
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
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    let content = std::fs::read_to_string(&lockfile_path).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&content).expect("parse fresh lockfile");

    let packages = lockfile.packages.as_ref().expect("packages map populated");
    let pkg_key: pacquet_lockfile::PackageKey =
        "@pnpm.e2e/hello-world-js-bin@1.0.0".parse().unwrap();
    let metadata = packages.get(&pkg_key).expect("packages entry recorded");
    let peers = metadata
        .peer_dependencies
        .as_ref()
        .expect("packageExtensions added peerDependencies must be recorded");
    assert_eq!(peers.get("synthetic-peer").map(String::as_str), Some("*"));

    // The lockfile must also carry the `packageExtensionsChecksum`
    // (sha256-prefixed) so a subsequent frozen install can detect
    // drift. Mirrors upstream's
    // `ctx.wantedLockfile.packageExtensionsChecksum = packageExtensionsChecksum`
    // assignment.
    let checksum = lockfile
        .package_extensions_checksum
        .as_deref()
        .expect("packageExtensionsChecksum must be recorded");
    assert!(
        checksum.starts_with("sha256-"),
        "checksum must use the sha256-prefixed wire shape; got {checksum:?}",
    );

    drop((dir, mock_instance));
}

/// `packageExtensions` drift between the lockfile-recorded checksum
/// and the freshly-computed value from `Config::package_extensions`
/// surfaces as `OutdatedLockfile` with a
/// `StalenessReason::PackageExtensionsChecksumChanged` payload.
/// Mirrors upstream's
/// `getOutdatedLockfileSetting → 'packageExtensionsChecksum'` branch
/// firing `LockfileConfigMismatchError` under `--frozen-lockfile`.
#[tokio::test]
async fn frozen_lockfile_errors_when_package_extensions_drift_from_lockfile() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    // Config declares an extension the lockfile doesn't carry → drift.
    let mut deps = std::collections::BTreeMap::new();
    deps.insert("dep-a".to_string(), "1.0.0".to_string());
    let mut extensions = indexmap::IndexMap::new();
    extensions.insert(
        "foo".to_string(),
        pacquet_config::PackageExtension { dependencies: Some(deps), ..Default::default() },
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
        lockfile: Some(&lockfile),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<SilentReporter>()
    .await;

    let err = result.expect_err("packageExtensions drift must surface as OutdatedLockfile");
    match err {
        InstallError::OutdatedLockfile {
            reason: pacquet_lockfile::StalenessReason::PackageExtensionsChecksumChanged { .. },
        } => {}
        other => {
            panic!("expected OutdatedLockfile::PackageExtensionsChecksumChanged, got {other:?}")
        }
    }

    drop(dir);
}

/// Runs a fresh install in `root` with `root_deps` as direct prod
/// dependencies and `pnpmfile_src` written to `<root>/.pnpmfile.cjs`, so the
/// pnpmfile hooks are discovered and run during resolution.
async fn install_with_pnpmfile(
    registry_url: String,
    root: &std::path::Path,
    root_deps: &[(&str, &str)],
    pnpmfile_src: &str,
) -> Result<(), InstallError> {
    install_with_pnpmfile_reporter::<SilentReporter>(registry_url, root, root_deps, pnpmfile_src)
        .await
}

/// Same as [`install_with_pnpmfile`] but routes install events through the
/// given reporter, so a recording reporter can assert on the `pnpm:hook`
/// log channel.
async fn install_with_pnpmfile_reporter<Reporter: self::Reporter + 'static>(
    registry_url: String,
    root: &std::path::Path,
    root_deps: &[(&str, &str)],
    pnpmfile_src: &str,
) -> Result<(), InstallError> {
    let modules_dir = root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    for (name, spec) in root_deps {
        manifest.add_dependency(name, spec, DependencyGroup::Prod).unwrap();
    }
    manifest.save().unwrap();

    std::fs::write(root.join(".pnpmfile.cjs"), pnpmfile_src).unwrap();

    let mut config = Config::new();
    config.store_dir = root.join("pacquet-store").into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.registry = registry_url;
    let config = config.leak();

    let http_client = Default::default();
    Install {
        tarball_mem_cache: Default::default(),
        http_client: &http_client,
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        lockfile_only: false,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
    }
    .run::<Reporter>()
    .await
}

// Ports pnpm's `readPackage hook` install test
// (pnpm/test/install/hooks.ts): the hook rewrites a resolved package's
// dependency range, and resolution honors it. `@pnpm.e2e/pkg-with-1-dep`
// depends on `@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0`, which would resolve
// to 100.1.0; pinning it to 100.0.0 in the hook installs 100.0.0 instead.
#[tokio::test]
async fn read_package_hook_pins_transitive_dependency_version() {
    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();

    install_with_pnpmfile(
        registry.url(),
        dir.path(),
        &[("@pnpm.e2e/pkg-with-1-dep", "100.0.0")],
        r"module.exports = { hooks: { readPackage (pkg) {
  if (pkg.name === '@pnpm.e2e/pkg-with-1-dep') {
    pkg.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0';
  }
  return pkg;
} } }",
    )
    .await
    .expect("install should succeed");

    let vsd = dir.path().join("node_modules/.pacquet");
    assert!(
        vsd.join("@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0").exists(),
        "readPackage hook should have pinned the transitive dep to 100.0.0",
    );
    assert!(
        !vsd.join("@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0").exists(),
        "the un-pinned 100.1.0 must not be installed",
    );

    drop((dir, registry));
}

// Ports pnpm's `readPackage hook makes installation fail if it does not
// return the modified package manifests`.
#[tokio::test]
async fn read_package_hook_failure_aborts_install() {
    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();

    let result = install_with_pnpmfile(
        registry.url(),
        dir.path(),
        &[("@pnpm.e2e/pkg-with-1-dep", "100.0.0")],
        "module.exports = { hooks: { readPackage (pkg) {} } }",
    )
    .await;

    assert!(result.is_err(), "install must fail when readPackage returns nothing");

    drop((dir, registry));
}

// Ports pnpm's `prints meaningful error when there is syntax error in
// .pnpmfile.cjs`.
#[tokio::test]
async fn pnpmfile_syntax_error_aborts_install() {
    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();

    let result = install_with_pnpmfile(
        registry.url(),
        dir.path(),
        &[("@pnpm.e2e/pkg-with-1-dep", "100.0.0")],
        "/boom",
    )
    .await;

    assert!(result.is_err(), "install must fail on a pnpmfile syntax error");

    drop((dir, registry));
}

// Ports pnpm's `pnpmfile: run afterAllResolved hook` and the deps-installer
// `readPackage, afterAllResolved hooks` test: the hook receives the resolved
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

// Ports pnpm's `adding or changing pnpmfile should change
// pnpmfileChecksum` (pnpm/test/hooks.ts): a project pnpmfile that
// exports hooks makes the install record its normalized-content hash as
// `pnpmfileChecksum` in pnpm-lock.yaml.
#[tokio::test]
async fn pnpmfile_with_hooks_records_pnpmfile_checksum() {
    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();

    let pnpmfile_src = r"module.exports = { hooks: { readPackage (pkg) { return pkg; } } }";
    install_with_pnpmfile(
        registry.url(),
        dir.path(),
        &[("@pnpm.e2e/pkg-with-1-dep", "100.0.0")],
        pnpmfile_src,
    )
    .await
    .expect("install should succeed");

    let lockfile_text = std::fs::read_to_string(dir.path().join("pnpm-lock.yaml")).unwrap();
    eprintln!("{lockfile_text}");
    let expected = pacquet_crypto_hash::create_hash(pnpmfile_src);
    assert!(
        lockfile_text.contains(&format!("pnpmfileChecksum: {expected}")),
        "pnpm-lock.yaml must record the pnpmfile's checksum",
    );
}

// A pnpmfile that exports no `hooks` object contributes no checksum,
// matching pnpm's `entries.some(entry => entry.hooks != null)` gate.
#[tokio::test]
async fn pnpmfile_without_hooks_omits_pnpmfile_checksum() {
    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();

    install_with_pnpmfile(
        registry.url(),
        dir.path(),
        &[("@pnpm.e2e/pkg-with-1-dep", "100.0.0")],
        "module.exports = {}",
    )
    .await
    .expect("install should succeed");

    let lockfile_text = std::fs::read_to_string(dir.path().join("pnpm-lock.yaml")).unwrap();
    eprintln!("{lockfile_text}");
    assert!(
        !lockfile_text.contains("pnpmfileChecksum"),
        "a pnpmfile without hooks must not record a checksum",
    );
}

// A throwing afterAllResolved hook aborts the install, matching pnpm.
#[tokio::test]
async fn after_all_resolved_hook_failure_aborts_install() {
    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();

    let result = install_with_pnpmfile(
        registry.url(),
        dir.path(),
        &[("@pnpm.e2e/pkg-with-1-dep", "100.0.0")],
        "module.exports = { hooks: { afterAllResolved () { throw new Error('boom'); } } }",
    )
    .await;

    assert!(result.is_err(), "install must fail when afterAllResolved throws");
}

/// The first `pnpm:hook` event in `events`, or panic.
fn first_hook_log(events: &[LogEvent]) -> &HookLog {
    events
        .iter()
        .find_map(|event| match event {
            LogEvent::Hook(log) => Some(log),
            _ => None,
        })
        .expect("a pnpm:hook event must be emitted")
}

// Ports pnpm's `pnpmfile: pass log function to readPackage hook`
// (pnpm/test/install/hooks.ts): a `readPackage` hook's `context.log(...)`
// surfaces on the `pnpm:hook` channel with the pnpmfile path (`from`), the
// project (`prefix`), the hook name, and the message.
#[tokio::test]
async fn read_package_hook_log_is_forwarded_to_pnpm_hook_channel() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();

    install_with_pnpmfile_reporter::<RecordingReporter>(
        registry.url(),
        dir.path(),
        &[("@pnpm.e2e/pkg-with-1-dep", "100.0.0")],
        r"module.exports = { hooks: { readPackage (pkg, context) {
  if (pkg.name === '@pnpm.e2e/pkg-with-1-dep') {
    pkg.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0';
    context.log('@pnpm.e2e/dep-of-pkg-with-1-dep pinned to 100.0.0');
  }
  return pkg;
} } }",
    )
    .await
    .expect("install should succeed");

    let captured = EVENTS.lock().unwrap();
    let hook_log = first_hook_log(&captured);
    assert_eq!(hook_log.hook, "readPackage");
    assert_eq!(hook_log.message, "@pnpm.e2e/dep-of-pkg-with-1-dep pinned to 100.0.0");
    assert!(!hook_log.from.is_empty(), "from must be the pnpmfile path");
    assert!(!hook_log.prefix.is_empty(), "prefix must be the project dir");

    drop((dir, registry));
}

// Ports pnpm's `pnpmfile: run afterAllResolved hook`: an `afterAllResolved`
// hook's `context.log(...)` surfaces on the `pnpm:hook` channel.
#[tokio::test]
async fn after_all_resolved_hook_log_is_forwarded_to_pnpm_hook_channel() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();

    install_with_pnpmfile_reporter::<RecordingReporter>(
        registry.url(),
        dir.path(),
        &[("@pnpm.e2e/pkg-with-1-dep", "100.0.0")],
        r"module.exports = { hooks: { afterAllResolved (lockfile, context) {
  context.log('All resolved');
  return lockfile;
} } }",
    )
    .await
    .expect("install should succeed");

    let captured = EVENTS.lock().unwrap();
    let hook_log = first_hook_log(&captured);
    assert_eq!(hook_log.hook, "afterAllResolved");
    assert_eq!(hook_log.message, "All resolved");
    assert!(!hook_log.from.is_empty(), "from must be the pnpmfile path");
    assert!(!hook_log.prefix.is_empty(), "prefix must be the project dir");

    drop((dir, registry));
}

// Ports pnpm's `pnpmfile: run async afterAllResolved hook`: an async
// `afterAllResolved` hook's `context.log(...)` also surfaces on `pnpm:hook`.
#[tokio::test]
async fn async_after_all_resolved_hook_log_is_forwarded_to_pnpm_hook_channel() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let registry = TestRegistry::start();
    let dir = tempdir().unwrap();

    install_with_pnpmfile_reporter::<RecordingReporter>(
        registry.url(),
        dir.path(),
        &[("@pnpm.e2e/pkg-with-1-dep", "100.0.0")],
        r"module.exports = { hooks: { async afterAllResolved (lockfile, context) {
  context.log('All resolved');
  return lockfile;
} } }",
    )
    .await
    .expect("install should succeed");

    let captured = EVENTS.lock().unwrap();
    let hook_log = first_hook_log(&captured);
    assert_eq!(hook_log.hook, "afterAllResolved");
    assert_eq!(hook_log.message, "All resolved");

    drop((dir, registry));
}
