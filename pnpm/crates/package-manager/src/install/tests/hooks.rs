use super::{
    super::{apply_deploy_manifest_hook, apply_deploy_manifest_hook_to_arc},
    first_hook_log, install_with_pnpmfile, install_with_pnpmfile_reporter,
};
use pnpm_reporter::{HookLog, LogEvent, LogLevel, Reporter};
use pnpm_testing_utils::registry::TestRegistry;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

#[test]
fn deploy_manifest_hook_preserves_existing_dependency_metadata() {
    let mut manifest = serde_json::json!({
        "dependencies": {
            "existing": "workspace:*",
            "non-object": "workspace:*",
            "new": "workspace:*",
        },
        "dependenciesMeta": {
            "existing": { "built": false, "injected": false },
            "non-object": null,
        },
    });

    apply_deploy_manifest_hook(&mut manifest);

    assert_eq!(
        manifest["dependenciesMeta"],
        serde_json::json!({
            "existing": { "built": false, "injected": true },
            "non-object": { "injected": true },
            "new": { "injected": true },
        }),
    );
}
#[test]
fn deploy_manifest_hook_reuses_unchanged_arc() {
    let manifest = Arc::new(serde_json::json!({
        "dependencies": { "registry-package": "1.0.0" },
    }));

    let transformed = apply_deploy_manifest_hook_to_arc(Arc::clone(&manifest));

    assert!(Arc::ptr_eq(&manifest, &transformed));
}
// The `readPackage` hook rewrites a resolved package's
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
// A `readPackage` hook that does not return the modified package
// manifest makes the installation fail.
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
// A syntax error in `.pnpmfile.cjs` prints a meaningful error and
// aborts the install.
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
// A project pnpmfile that exports hooks makes the install record its
// normalized-content hash as `pnpmfileChecksum` in pnpm-lock.yaml, so
// adding or changing the pnpmfile changes the checksum.
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
    let expected = pnpm_crypto_hash::create_hash(pnpmfile_src);
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
// A `readPackage` hook's `context.log(...)`
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
// An `afterAllResolved`
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
// An async
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
// Ports pnpm's `pnpmfile: preResolution hook logger`: a `preResolution`
// hook's `logger.info(...)` and `logger.warn(...)` surface on the
// `pnpm:hook` channel with `from: "pnpmfile"`, `hook: "preResolution"`,
// and the matching log level. Mirrors the TypeScript
// `createPreResolutionHookLogger` which emits at `info`/`warn`. A raw
// `console.log(...)` line from the hook (not wrapped in the JSON logger
// protocol) is forwarded at `info` rather than silently dropped.
#[tokio::test]
async fn pre_resolution_hook_log_is_forwarded_to_pnpm_hook_channel() {
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
        r"module.exports = { hooks: { preResolution (ctx, logger) {
  logger.info('Starting resolution');
  logger.warn('Some packages may need updates');
  console.log('raw hook output');
} } }",
    )
    .await
    .expect("install should succeed");

    let captured = EVENTS.lock().unwrap();
    fn find_hook_event<'a>(
        captured: &'a [LogEvent],
        level: LogLevel,
        message: &str,
    ) -> &'a HookLog {
        captured
            .iter()
            .find_map(|event| match event {
                LogEvent::Hook(log)
                    if log.hook == "preResolution"
                        && log.level == level
                        && log.message == message =>
                {
                    Some(log)
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!("a pnpm:hook {level:?} event with message {message:?} must be emitted")
            })
    }

    for log in [
        find_hook_event(&captured, LogLevel::Info, "Starting resolution"),
        find_hook_event(&captured, LogLevel::Warn, "Some packages may need updates"),
        find_hook_event(&captured, LogLevel::Info, "raw hook output"),
    ] {
        assert_eq!(log.from, "pnpmfile", "preResolution from is hardcoded to 'pnpmfile'");
        assert_eq!(log.prefix, *dir.path().to_string_lossy(), "prefix must be the lockfile dir");
    }

    drop((dir, registry));
}
