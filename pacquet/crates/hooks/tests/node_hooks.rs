use std::sync::Arc;
use tempfile::TempDir;

use pacquet_hooks::{PnpmfileHooks, finder};

#[test]
fn test_find_pnpmfile_uses_mjs() {
    let tmp = TempDir::new().expect("temp dir");
    let root = tmp.path();

    // Create .pnpmfile.mjs first, then .pnpmfile.cjs
    std::fs::write(root.join(".pnpmfile.mjs"), "// mjs").expect("write mjs");
    std::fs::write(root.join(".pnpmfile.cjs"), "// cjs").expect("write cjs");

    let found = finder::find_pnpmfile(root);
    // Should prefer .mjs
    assert!(found.unwrap().ends_with(".pnpmfile.mjs"));
}

#[test]
fn test_find_pnpmfile_fallback_to_cjs() {
    let tmp = TempDir::new().expect("temp dir");
    let root = tmp.path();

    // Only create .pnpmfile.cjs (no .mjs)
    std::fs::write(root.join(".pnpmfile.cjs"), "// cjs").expect("write cjs");

    let found = finder::find_pnpmfile(root);
    assert!(found.unwrap().ends_with(".pnpmfile.cjs"));
}

#[tokio::test]
async fn calculate_pnpmfile_checksum_hashes_normalized_contents_when_hooks_exported() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    let src = "module.exports = { hooks: { readPackage: (pkg) => pkg } }\n";
    std::fs::write(&pnpmfile_path, src).expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);
    let checksum = hooks.calculate_pnpmfile_checksum().await;

    assert_eq!(checksum, Some(pacquet_crypto_hash::create_hash(src)));
}

#[tokio::test]
async fn calculate_pnpmfile_checksum_is_none_when_no_hooks_exported() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(&pnpmfile_path, "module.exports = {}\n").expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    assert_eq!(hooks.calculate_pnpmfile_checksum().await, None);
}

#[test]
fn test_find_pnpmfile_none_when_missing() {
    let tmp = TempDir::new().expect("temp dir");
    let root = tmp.path();

    let found = finder::find_pnpmfile(root);
    assert!(found.is_none());
}

#[tokio::test]
async fn test_node_js_hooks_read_package() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r"
module.exports = {
  hooks: { readPackage }
}

function readPackage(pkg) {
  if (pkg.name === 'foo') {
    pkg.dependencies = { bar: '100.0.0' };
  }
  return pkg;
}
",
    )
    .expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    let manifest = serde_json::json!({
        "name": "foo",
        "version": "1.0.0"
    });

    let result = hooks
        .read_package(manifest.clone(), pacquet_hooks::HookContext { log: Arc::new(|_| {}) })
        .await;

    let updated = result.expect("readPackage should succeed");
    assert_eq!(updated["dependencies"]["bar"], "100.0.0");
}

#[tokio::test]
async fn test_node_js_hooks_read_package_no_match() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r"
module.exports = {
  hooks: { readPackage }
}

function readPackage(pkg) {
  return pkg;
}
",
    )
    .expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    let manifest = serde_json::json!({
        "name": "baz",
        "version": "1.0.0"
    });

    let result = hooks
        .read_package(manifest.clone(), pacquet_hooks::HookContext { log: Arc::new(|_| {}) })
        .await;

    let updated = result.expect("readPackage should succeed");
    assert_eq!(updated["name"], "baz");
}

#[tokio::test]
async fn test_node_js_hooks_filter_log() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r"
module.exports = {
  hooks: { filterLog }
}

function filterLog(log) {
  return log.level === 'debug' || log.level === 'error';
}
",
    )
    .expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    let debug_log = serde_json::json!({
        "level": "debug",
        "message": "test debug"
    });

    assert!(
        hooks.filter_log(debug_log, pacquet_hooks::HookContext { log: Arc::new(|_| {}) }).await,
    );

    let warn_log = serde_json::json!({
        "level": "warn",
        "message": "test warn"
    });

    assert!(
        !hooks.filter_log(warn_log, pacquet_hooks::HookContext { log: Arc::new(|_| {}) }).await,
    );
}

#[cfg_attr(
    target_os = "windows",
    ignore = "Node.js ESM import() on Windows resolves absolute paths differently"
)]
#[tokio::test]
async fn test_node_js_hooks_read_package_mjs() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.mjs");
    std::fs::write(
        &pnpmfile_path,
        r"
export const hooks = { readPackage };

function readPackage(pkg) {
  if (pkg.name === 'foo') {
    pkg.dependencies = { bar: '100.0.0' };
  }
  return pkg;
}
",
    )
    .expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path.clone());

    let manifest = serde_json::json!({
        "name": "foo",
        "version": "1.0.0"
    });

    let result = hooks
        .read_package(manifest.clone(), pacquet_hooks::HookContext { log: Arc::new(|_| {}) })
        .await;

    let updated = result.unwrap_or_else(|err| {
        panic!(
            "readPackage failed; the Node.js subprocess likely could not load the .mjs file at {}: {err}",
            pnpmfile_path.display(),
        )
    });
    assert_eq!(updated["dependencies"]["bar"], "100.0.0");
}

#[tokio::test]
async fn test_node_js_hooks_pre_resolution() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r"
module.exports = {
  hooks: { preResolution }
}

function preResolution(ctx, logger) {
  // Verify both ctx and logger are passed correctly
  if (ctx.lockfileDir !== '/test/lockfile') throw new Error('wrong lockfileDir');
  if (ctx.storeDir !== '/test/store') throw new Error('wrong storeDir');
  if (typeof logger.info !== 'function') throw new Error('missing logger.info');
  if (typeof logger.warn !== 'function') throw new Error('missing logger.warn');
}
",
    )
    .expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    let ctx = pacquet_hooks::PreResolutionHookContext {
        wanted_lockfile: serde_json::json!({}),
        current_lockfile: serde_json::json!({}),
        exists_current_lockfile: false,
        exists_non_empty_wanted_lockfile: false,
        lockfile_dir: "/test/lockfile".to_string(),
        store_dir: "/test/store".to_string(),
        registries: serde_json::json!({ "default": "http://localhost:1234/" }),
    };

    // The hook should execute without error
    hooks
        .pre_resolution(
            ctx,
            pacquet_hooks::PreResolutionHookLogger {
                info: Arc::new(|_| {}),
                warn: Arc::new(|_| {}),
            },
        )
        .await;
}

#[tokio::test]
async fn test_node_js_hooks_pre_resolution_mjs() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.mjs");
    std::fs::write(
        &pnpmfile_path,
        r"
export const hooks = { preResolution };

function preResolution(ctx, logger) {
  // Verify both ctx and logger are passed correctly
  if (ctx.lockfileDir !== '/test/lockfile') throw new Error('wrong lockfileDir');
  if (ctx.storeDir !== '/test/store') throw new Error('wrong storeDir');
  if (typeof logger.info !== 'function') throw new Error('missing logger.info');
  if (typeof logger.warn !== 'function') throw new Error('missing logger.warn');
}
",
    )
    .expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    let ctx = pacquet_hooks::PreResolutionHookContext {
        wanted_lockfile: serde_json::json!({}),
        current_lockfile: serde_json::json!({}),
        exists_current_lockfile: false,
        exists_non_empty_wanted_lockfile: false,
        lockfile_dir: "/test/lockfile".to_string(),
        store_dir: "/test/store".to_string(),
        registries: serde_json::json!({ "default": "http://localhost:1234/" }),
    };

    // The hook should execute without error
    hooks
        .pre_resolution(
            ctx,
            pacquet_hooks::PreResolutionHookLogger {
                info: Arc::new(|_| {}),
                warn: Arc::new(|_| {}),
            },
        )
        .await;
}

/// Helper: write `source` to a `.pnpmfile.cjs` in a fresh temp dir and return
/// the hooks bridge plus the dir (kept alive for the file's lifetime).
fn cjs_hooks(source: &str) -> (pacquet_hooks::node_runtime::NodeJsHooks, TempDir) {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(&path, source).expect("write pnpmfile");
    (pacquet_hooks::node_runtime::NodeJsHooks::new(path), tmp)
}

async fn read_package_err(source: &str) -> String {
    let (hooks, _tmp) = cjs_hooks(source);
    hooks
        .read_package(
            serde_json::json!({ "name": "foo", "version": "1.0.0" }),
            pacquet_hooks::HookContext { log: Arc::new(|_| {}) },
        )
        .await
        .expect_err("readPackage should fail")
        .to_string()
}

#[tokio::test]
async fn read_package_fails_when_hook_returns_undefined() {
    let err = read_package_err("module.exports = { hooks: { readPackage (pkg) {} } }").await;
    eprintln!("err = {err}");
    assert!(err.contains("readPackage hook did not return a package manifest object."));
}

#[tokio::test]
async fn read_package_fails_when_dependencies_is_not_an_object() {
    let err = read_package_err(
        "module.exports = { hooks: { readPackage: (pkg) => ({ ...pkg, dependencies: 'nope' }) } }",
    )
    .await;
    eprintln!("err = {err}");
    assert!(err.contains("property 'dependencies' must be an object."));
}

#[tokio::test]
async fn read_package_fails_when_dev_dependencies_is_not_an_object() {
    let err = read_package_err(
        "module.exports = { hooks: { readPackage: (pkg) => ({ ...pkg, devDependencies: 1 }) } }",
    )
    .await;
    eprintln!("err = {err}");
    assert!(err.contains("property 'devDependencies' must be an object."));
}

#[tokio::test]
async fn read_package_fails_when_peer_dependencies_is_an_array() {
    let err = read_package_err(
        "module.exports = { hooks: { readPackage: (pkg) => ({ ...pkg, peerDependencies: [] }) } }",
    )
    .await;
    eprintln!("err = {err}");
    assert!(err.contains("property 'peerDependencies' must be an object."));
}

#[tokio::test]
async fn read_package_normalizes_missing_dependency_fields() {
    // The manifest has no dependency fields; the hook writes into them
    // directly, relying on pnpm's normalization that defaults each to `{}`
    // before the hook runs.
    let (hooks, _tmp) = cjs_hooks(
        r"module.exports = { hooks: { readPackage (pkg) {
  pkg.dependencies['is-positive'] = '*';
  pkg.optionalDependencies['is-negative'] = '*';
  pkg.peerDependencies['is-negative'] = '*';
  pkg.devDependencies['is-positive'] = '*';
  return pkg;
} } }",
    );

    let updated = hooks
        .read_package(
            serde_json::json!({ "name": "x", "version": "1.0.0" }),
            pacquet_hooks::HookContext { log: Arc::new(|_| {}) },
        )
        .await
        .expect("readPackage should succeed after normalization");
    assert_eq!(updated["dependencies"]["is-positive"], "*");
    assert_eq!(updated["peerDependencies"]["is-negative"], "*");
}

#[tokio::test]
async fn read_package_fails_with_meaningful_error_on_syntax_error() {
    let err = read_package_err("/boom").await;
    eprintln!("err = {err}");
    assert!(err.contains("Error during pnpmfile execution"));
    assert!(err.contains("SyntaxError"));
}

#[tokio::test]
async fn read_package_fails_when_pnpmfile_requires_missing_module() {
    let err = read_package_err("module.exports = require('./this-does-not-exist')").await;
    eprintln!("err = {err}");
    assert!(err.contains("Error during pnpmfile execution"));
}

// The worker multiplexes concurrent readPackage calls by request id: each
// concurrent call must get back the manifest it sent, not another call's.
#[tokio::test]
async fn worker_multiplexes_concurrent_read_package_calls() {
    let (hooks, _tmp) = cjs_hooks(
        r"module.exports = { hooks: { readPackage (pkg) {
  pkg.dependencies['self'] = pkg.name;
  return pkg;
} } }",
    );
    let hooks = Arc::new(hooks);

    let mut set = tokio::task::JoinSet::new();
    for i in 0..32u32 {
        let hooks = Arc::clone(&hooks);
        set.spawn(async move {
            let name = format!("pkg-{i}");
            let updated = hooks
                .read_package(
                    serde_json::json!({ "name": name, "version": "1.0.0" }),
                    pacquet_hooks::HookContext { log: Arc::new(|_| {}) },
                )
                .await
                .expect("readPackage should succeed");
            (name, updated["dependencies"]["self"].as_str().unwrap().to_string())
        });
    }

    while let Some(joined) = set.join_next().await {
        let (sent, echoed) = joined.expect("task should not panic");
        assert_eq!(sent, echoed, "a concurrent call received another call's response");
    }
}

// A `context.log(...)` call inside readPackage is forwarded to the
// HookContext's log callback.
#[tokio::test]
async fn worker_forwards_read_package_context_log() {
    let (hooks, _tmp) = cjs_hooks(
        r"module.exports = { hooks: { readPackage (pkg, context) {
  context.log('hello from ' + pkg.name);
  return pkg;
} } }",
    );

    let logs = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&logs);
    hooks
        .read_package(
            serde_json::json!({ "name": "foo", "version": "1.0.0" }),
            pacquet_hooks::HookContext {
                log: Arc::new(move |message| sink.lock().unwrap().push(message)),
            },
        )
        .await
        .expect("readPackage should succeed");

    assert_eq!(logs.lock().unwrap().as_slice(), &["hello from foo".to_string()]);
}

fn noop_context() -> pacquet_hooks::HookContext {
    pacquet_hooks::HookContext { log: Arc::new(|_| {}) }
}

#[test]
fn is_plugin_name_matches_the_three_patterns() {
    assert!(finder::is_plugin_name("pnpm-plugin-foo"));
    assert!(finder::is_plugin_name("@pnpm/plugin-foo"));
    assert!(finder::is_plugin_name("@my-org/pnpm-plugin-foo"));

    assert!(!finder::is_plugin_name("foo"));
    assert!(!finder::is_plugin_name("@pnpm.e2e/foo"));
    assert!(!finder::is_plugin_name("@my-org/not-a-plugin"));
    assert!(!finder::is_plugin_name("my-pnpm-plugin-foo"));
}

#[test]
fn calc_pnpmfile_paths_skips_non_plugins_and_missing_dirs() {
    let tmp = TempDir::new().expect("temp dir");
    let config_modules = tmp.path().join(".pnpm-config");

    // A plugin with a pnpmfile.cjs.
    let cjs_plugin = config_modules.join("pnpm-plugin-a");
    std::fs::create_dir_all(&cjs_plugin).unwrap();
    std::fs::write(cjs_plugin.join("pnpmfile.cjs"), "module.exports = {}").unwrap();

    // A scoped plugin with a pnpmfile.mjs (preferred over cjs).
    let mjs_plugin = config_modules.join("@scope/pnpm-plugin-b");
    std::fs::create_dir_all(&mjs_plugin).unwrap();
    std::fs::write(mjs_plugin.join("pnpmfile.mjs"), "export const hooks = {}").unwrap();
    std::fs::write(mjs_plugin.join("pnpmfile.cjs"), "module.exports = {}").unwrap();

    // A non-plugin config dep (no pnpmfile loaded) and a plugin whose
    // directory was never installed (skipped silently).
    std::fs::create_dir_all(config_modules.join("@pnpm.e2e/foo")).unwrap();

    let names = ["pnpm-plugin-a", "@scope/pnpm-plugin-b", "@pnpm.e2e/foo", "pnpm-plugin-missing"];
    let paths = finder::calc_pnpmfile_paths_of_plugin_deps(&config_modules, names);

    assert_eq!(
        paths,
        vec![mjs_plugin.join("pnpmfile.mjs"), cjs_plugin.join("pnpmfile.cjs")],
        "plugins sort lexically; mjs is preferred; non-plugins and missing dirs are dropped",
    );
}

#[tokio::test]
async fn update_config_applies_hook_result() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join("pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r"module.exports = { hooks: { updateConfig (config) {
  config.catalogs = { default: { foo: '1.0.0' } };
  return config;
} } }",
    )
    .expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);
    let updated = hooks
        .update_config(serde_json::json!({ "registry": "https://r/" }), noop_context())
        .await
        .expect("updateConfig should succeed");

    assert_eq!(updated["registry"], "https://r/", "untouched keys are preserved");
    assert_eq!(updated["catalogs"]["default"]["foo"], "1.0.0", "hook-set key is applied");
}

#[tokio::test]
async fn update_config_without_hook_returns_config_unchanged() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join("pnpmfile.cjs");
    std::fs::write(&pnpmfile_path, "module.exports = { hooks: {} }").expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);
    let config = serde_json::json!({ "registry": "https://r/" });
    let updated = hooks.update_config(config.clone(), noop_context()).await.expect("ok");

    assert_eq!(updated, config, "a pnpmfile without updateConfig leaves config unchanged");
}
