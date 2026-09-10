use super::{Arc, TempDir};
use pnpm_hooks::PnpmfileHooks as _;

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

    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    let manifest = serde_json::json!({
        "name": "foo",
        "version": "1.0.0"
    });

    let result = hooks
        .read_package(
            manifest.clone(),
            pnpm_hooks::HookContext { log: Arc::new(|_| {}), dir: None },
        )
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

    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    let manifest = serde_json::json!({
        "name": "baz",
        "version": "1.0.0"
    });

    let result = hooks
        .read_package(
            manifest.clone(),
            pnpm_hooks::HookContext { log: Arc::new(|_| {}), dir: None },
        )
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

    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    assert!(hooks.has_filter_log().await);

    let debug_log = serde_json::json!({
        "level": "debug",
        "message": "test debug"
    });

    assert!(
        hooks
            .filter_log(debug_log, pnpm_hooks::HookContext { log: Arc::new(|_| {}), dir: None })
            .await,
    );

    let warn_log = serde_json::json!({
        "level": "warn",
        "message": "test warn"
    });

    assert!(
        !hooks
            .filter_log(warn_log, pnpm_hooks::HookContext { log: Arc::new(|_| {}), dir: None })
            .await,
    );
}

#[tokio::test]
async fn test_node_js_hooks_read_package_mjs() {
    let tmp = TempDir::new().expect("temp dir");
    let hooks_dir = tmp.path().join("hooks #100%");
    std::fs::create_dir(&hooks_dir).expect("create hooks dir");
    let pnpmfile_path = hooks_dir.join(".pnpmfile.mjs");
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

    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path.clone());

    let manifest = serde_json::json!({
        "name": "foo",
        "version": "1.0.0"
    });

    let result = hooks
        .read_package(
            manifest.clone(),
            pnpm_hooks::HookContext { log: Arc::new(|_| {}), dir: None },
        )
        .await;

    let updated = result.unwrap_or_else(|err| {
        panic!(
            "readPackage failed; the Node.js subprocess likely could not load the .mjs file at {}: {err}",
            pnpmfile_path.display(),
        )
    });
    assert_eq!(updated["dependencies"]["bar"], "100.0.0");
}
