use super::{Install, InstallError};
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_modules_yaml::{
    DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH, LayoutVersion, Modules, NodeLinker, RealApi,
    read_modules_manifest, write_modules_manifest,
};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_registry_mock::AutoMockInstance;
use pacquet_reporter::{
    BrokenModulesLog, ContextLog, IgnoredScriptsLog, LogEvent, PackageManifestLog,
    PackageManifestMessage, ProgressLog, ProgressMessage, Reporter, SilentReporter, Stage,
    StageLog, StatsLog, StatsMessage, SummaryLog,
};
use pacquet_testing_utils::fs::{get_all_folders, is_symlink_or_junction};
use pacquet_workspace_state::{
    self as workspace_state, NodeLinker as WorkspaceStateNodeLinker, load_workspace_state,
};
use pipe_trait::Pipe;
use std::{path::PathBuf, sync::Mutex};
use tempfile::tempdir;
use text_block_macros::text_block;

#[tokio::test]
async fn should_install_dependencies() {
    let mock_instance = AutoMockInstance::load_or_init();

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
    config.modules_dir = modules_dir.to_path_buf();
    config.virtual_store_dir = virtual_store_dir.to_path_buf();
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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
    let path = project_root.join("node_modules/.pacquet/@pnpm+xyz@1.0.0");
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
    config.modules_dir = modules_dir.to_path_buf();
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    let result = Install {
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
    }
    .run::<SilentReporter>()
    .await;

    assert!(matches!(result, Err(InstallError::NoLockfile)));
    drop(dir);
}

#[tokio::test]
async fn should_error_when_writable_lockfile_mode_is_used() {
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
    config.modules_dir = modules_dir.to_path_buf();
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    let result = Install {
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
    }
    .run::<SilentReporter>()
    .await;

    assert!(matches!(result, Err(InstallError::UnsupportedLockfileMode)));
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
/// *isn't* `NoLockfile` / `UnsupportedLockfileMode` proves the
/// dispatch picked the frozen path. Passing a malformed lockfile
/// integrity surfaces as `FrozenLockfile(...)`.
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
    config.modules_dir = modules_dir.to_path_buf();
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
    }
    .run::<SilentReporter>()
    .await
    .expect("--frozen-lockfile + empty lockfile should succeed via InstallFrozenLockfile");

    drop(dir);
}

/// Issue #312: an npm-alias dependency
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
    let mock_instance = AutoMockInstance::load_or_init();

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
    config.modules_dir = modules_dir.to_path_buf();
    config.virtual_store_dir = virtual_store_dir.to_path_buf();
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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

/// Issue #312, unversioned variant: `"foo": "npm:bar"` (no `@<range>`)
/// must default to `latest` without panicking. `resolve_registry_dependency`
/// turns `"npm:bar"` into `("bar", "latest")`; the previous code then
/// fed `"latest"` to `package.pinned_version()` which panics because
/// `node_semver::Range` cannot parse the string. The fix is to route
/// `"latest"` (and any `PackageTag`-parseable value) through
/// `PackageVersion::fetch_from_registry` directly.
///
/// We use the same scoped test package as the pinned-version test above
/// but omit the `@1.0.0` suffix to trigger the default-to-`latest` path.
#[tokio::test]
async fn unversioned_npm_alias_defaults_to_latest() {
    let mock_instance = AutoMockInstance::load_or_init();

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
    config.modules_dir = modules_dir.to_path_buf();
    config.virtual_store_dir = virtual_store_dir.to_path_buf();
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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
    let has_real_name_dir = std::fs::read_dir(&virtual_store_dir_path)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("@pnpm.e2e+hello-world-js-bin@"));
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
    config.modules_dir = modules_dir.to_path_buf();
    config.virtual_store_dir = virtual_store_dir;
    let config = config.leak();

    let result = Install {
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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
/// one has no link_file calls and no such event in the captured
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
    config.modules_dir = modules_dir.to_path_buf();
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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
    assert_eq!(emitted_store_dir, &store_dir.display().to_string());
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        // Drive a non-default `included`: prod + optional, no dev,
        // so the assertion below pins the mapping of dispatched
        // groups to the on-disk `included` field.
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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
        .pipe_as_ref(read_modules_manifest::<RealApi>)
        .expect("read .modules.yaml")
        .expect("modules manifest exists");

    assert_eq!(layout_version, Some(LayoutVersion));
    assert_eq!(node_linker, Some(NodeLinker::Isolated));
    assert!(included.dependencies);
    assert!(!included.dev_dependencies);
    assert!(included.optional_dependencies);
    assert_eq!(emitted_store_dir, store_dir.display().to_string());
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        // Same `included` shape as `install_writes_modules_yaml` so the
        // dev/optional/production assertions below line up with the
        // dispatched groups.
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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
    assert_eq!(settings.hoist_workspace_packages, Some(true));
    assert_eq!(settings.hoist_pattern.as_deref(), Some(&["*".to_string()][..]));

    drop(dir);
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
    let mock_instance = AutoMockInstance::load_or_init();

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
    config.modules_dir = modules_dir.to_path_buf();
    config.virtual_store_dir = virtual_store_dir.to_path_buf();
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: false,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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

/// A v9 lockfile fixture pinned to a placeholder package whose
/// integrity is bogus on purpose. Pacquet enforces tarball integrity
/// on the install path, so any test that lets the install reach the
/// fetch site would fail — meaning a successful install with this
/// fixture is *proof* that the per-snapshot skip path (issue #433
/// section B) short-circuited the fetch entirely.
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
    // check (#447) rejects any drift between the on-disk manifest and
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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
    assert_eq!(written.snapshots.as_ref().map(|s| s.len()), Some(1));

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
    // check (#447) rejects any drift between the on-disk manifest and
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
    }
    .run::<RecordingReporter>()
    .await;

    let captured = EVENTS.lock().unwrap();
    let broken: Vec<&BrokenModulesLog> = captured
        .iter()
        .filter_map(|e| match e {
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
    // check (#447) rejects any drift between the on-disk manifest and
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
    }
    .run::<RecordingReporter>()
    .await
    .expect("first install should succeed");

    let first_context = EVENTS
        .lock()
        .unwrap()
        .iter()
        .find_map(|e| match e {
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
    }
    .run::<RecordingReporter>()
    .await
    .expect("second install should succeed");

    let second_context = EVENTS
        .lock()
        .unwrap()
        .iter()
        .find_map(|e| match e {
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
    // check (#447) rejects any drift between the on-disk manifest and
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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
        .filter_map(|e| match e {
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

/// Issue #447: a `--frozen-lockfile` install where the on-disk
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
    }
    .run::<SilentReporter>()
    .await
    .expect("frozen-lockfile install under GVS should succeed");

    // `register_project` wrote `<store_dir>/projects/<short-hash>`
    // pointing back at the project dir. Canonicalize the *entry
    // path* (not `read_link`'s output) so the kernel follows the
    // symlink — pacquet, like upstream pnpm, writes the target as
    // a path relative to the link's parent, so canonicalizing the
    // raw `read_link` string from the CWD would never resolve.
    let projects_dir = store_dir.join("projects");
    assert!(projects_dir.is_dir(), "GVS-on install must create <store_dir>/projects/");
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
    }
    .run::<SilentReporter>()
    .await
    .expect("frozen-lockfile install with GVS off should succeed");

    assert!(
        !store_dir.join("projects").exists(),
        "GVS-off install must NOT create the project-registry directory",
    );

    drop(dir);
}

/// Workspace install under GVS registers each importer separately.
/// Mirrors upstream's per-project
/// [`registerProject`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/projectRegistry.ts)
/// call site, which fires once per workspace package — a workspace
/// with `.` (root) and `packages/web` therefore ends up with two
/// entries in `<store_dir>/projects/`, each resolving back to its
/// own root dir. `pacquet store prune` (tracked separately) needs
/// every reachable importer in the registry to keep the
/// `<store_dir>/links/...` slots they share alive.
#[tokio::test]
async fn frozen_lockfile_under_gvs_registers_each_workspace_importer() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let workspace_root = dir.path().join("workspace");
    let modules_dir = workspace_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    // Workspace layout: root + one sub-importer. Both directories
    // have to exist on disk because `register_project` canonicalises
    // the target before writing the symlink.
    let web_dir = workspace_root.join("packages/web");
    std::fs::create_dir_all(&web_dir).expect("create packages/web");
    let manifest_path = workspace_root.join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // The sub-importer needs a `package.json` too — the freshness check
    // satisfies on the root only today, but the per-importer registry
    // write still resolves the target on disk.
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
    // install reaches the per-importer registry-write loop without
    // doing any actual fetch/link work.
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
    }
    .run::<SilentReporter>()
    .await
    .expect("workspace frozen-lockfile install under GVS should succeed");

    // Exactly two registry entries — one per importer. Resolve the
    // symlink targets and confirm both project roots are present.
    let projects_dir = store_dir.join("projects");
    assert!(projects_dir.is_dir(), "GVS-on workspace install must create <store_dir>/projects/");
    let mut targets: Vec<PathBuf> = std::fs::read_dir(&projects_dir)
        .unwrap()
        .map(|entry| {
            // Canonicalize the entry path so the kernel follows the
            // (relative) symlink — see the sibling test for context.
            dunce::canonicalize(entry.unwrap().path()).expect("canonicalize registry entry")
        })
        .collect();
    targets.sort();
    let mut expected = [
        dunce::canonicalize(&workspace_root).expect("canonicalize workspace root"),
        dunce::canonicalize(&web_dir).expect("canonicalize packages/web"),
    ];
    expected.sort();
    assert_eq!(targets, expected, "every importer must have a registry entry");

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
    write_modules_manifest::<RealApi>(&modules_dir, seed_modules).expect("seed .modules.yaml");

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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
    }
    .run::<SilentReporter>()
    .await
    .expect("frozen-lockfile install should succeed");

    let written = modules_dir
        .pipe_as_ref(read_modules_manifest::<RealApi>)
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
    // freshness check (#447) doesn't reject the install before we
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
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(BROKEN_OPTIONAL_LOCKFILE)
        .expect("parse broken-optional fixture lockfile");

    Install {
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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
        .pipe_as_ref(read_modules_manifest::<RealApi>)
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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
        .pipe_as_ref(read_modules_manifest::<RealApi>)
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        // `--no-optional` shape: Optional NOT in the dispatch list.
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::default(),
        resolved_packages: &Default::default(),
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
/// (umbrella #438 slice 6). Empty lockfile drives the cheapest
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::Hoisted,
        resolved_packages: &Default::default(),
    }
    .run::<SilentReporter>()
    .await
    .expect("hoisted-linker install with empty lockfile should succeed");

    let written = modules_dir
        .pipe_as_ref(read_modules_manifest::<RealApi>)
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        node_linker: pacquet_config::NodeLinker::Hoisted,
        resolved_packages: &Default::default(),
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
/// Closes the variant-mismatch checkbox of #437 slice F.
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
    config.modules_dir = modules_dir.to_path_buf();
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    // Lockfile with a runtime entry whose variants only target a
    // platform we're not running on. `runtime:22.0.0` is preserved
    // through `PkgVerPeer`'s `Prefix::Runtime` (#511 / #512); the
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: true,
        skip_runtimes: false,
        supported_architectures: None,
        resolved_packages: &Default::default(),
        node_linker: pacquet_config::NodeLinker::default(),
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
/// Closes the `--no-runtime` checkbox of #437 slice F.
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
    config.modules_dir = modules_dir.to_path_buf();
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
        tarball_mem_cache: &Default::default(),
        http_client: &Default::default(),
        config,
        manifest: &manifest,
        lockfile: Some(&lockfile),
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: true,
        skip_runtimes: true,
        supported_architectures: None,
        resolved_packages: &Default::default(),
        node_linker: pacquet_config::NodeLinker::default(),
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
