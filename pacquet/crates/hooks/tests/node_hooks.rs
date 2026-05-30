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
        r#"
module.exports = {
  hooks: { readPackage }
}

function readPackage(pkg) {
  if (pkg.name === 'foo') {
    pkg.dependencies = { bar: '100.0.0' };
  }
  return pkg;
}
"#,
    )
    .expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks { file: pnpmfile_path };

    let manifest = serde_json::json!({
        "name": "foo",
        "version": "1.0.0"
    });

    let result = hooks
        .read_package(manifest.clone(), pacquet_hooks::HookContext { log: Arc::new(|_| {}) })
        .await;

    assert!(result.is_some());
    let updated = result.unwrap();
    assert_eq!(updated["dependencies"]["bar"], "100.0.0");
}

#[tokio::test]
async fn test_node_js_hooks_read_package_no_match() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r#"
module.exports = {
  hooks: { readPackage }
}

function readPackage(pkg) {
  return pkg;
}
"#,
    )
    .expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks { file: pnpmfile_path };

    let manifest = serde_json::json!({
        "name": "baz",
        "version": "1.0.0"
    });

    let result = hooks
        .read_package(manifest.clone(), pacquet_hooks::HookContext { log: Arc::new(|_| {}) })
        .await;

    assert!(result.is_some());
    let updated = result.unwrap();
    assert_eq!(updated["name"], "baz");
}

#[tokio::test]
async fn test_node_js_hooks_filter_log() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r#"
module.exports = {
  hooks: { filterLog }
}

function filterLog(log) {
  return log.level === 'debug' || log.level === 'error';
}
"#,
    )
    .expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks { file: pnpmfile_path };

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
        r#"
export const hooks = { readPackage };

function readPackage(pkg) {
  if (pkg.name === 'foo') {
    pkg.dependencies = { bar: '100.0.0' };
  }
  return pkg;
}
"#,
    )
    .expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks { file: pnpmfile_path.clone() };

    let manifest = serde_json::json!({
        "name": "foo",
        "version": "1.0.0"
    });

    let result = hooks
        .read_package(manifest.clone(), pacquet_hooks::HookContext { log: Arc::new(|_| {}) })
        .await;

    assert!(
        result.is_some(),
        "readPackage returned None; the Node.js subprocess likely failed to load the .mjs file at {}",
        pnpmfile_path.display()
    );
    let updated = result.unwrap();
    assert_eq!(updated["dependencies"]["bar"], "100.0.0");
}

#[tokio::test]
async fn test_node_js_hooks_pre_resolution() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r#"
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
"#,
    )
    .expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks { file: pnpmfile_path };

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
        r#"
export const hooks = { preResolution };

function preResolution(ctx, logger) {
  // Verify both ctx and logger are passed correctly
  if (ctx.lockfileDir !== '/test/lockfile') throw new Error('wrong lockfileDir');
  if (ctx.storeDir !== '/test/store') throw new Error('wrong storeDir');
  if (typeof logger.info !== 'function') throw new Error('missing logger.info');
  if (typeof logger.warn !== 'function') throw new Error('missing logger.warn');
}
"#,
    )
    .expect("write pnpmfile");

    let hooks = pacquet_hooks::node_runtime::NodeJsHooks { file: pnpmfile_path };

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
