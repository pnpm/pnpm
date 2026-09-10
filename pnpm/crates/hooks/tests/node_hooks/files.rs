use super::{TempDir, finder, read_package_err};
use pnpm_hooks::PnpmfileHooks as _;

#[test]
fn test_find_pnpmfile_uses_mjs() {
    let tmp = TempDir::new().expect("temp dir");
    let root = tmp.path();

    std::fs::write(root.join(".pnpmfile.mjs"), "// mjs").expect("write mjs");
    std::fs::write(root.join(".pnpmfile.cjs"), "// cjs").expect("write cjs");

    let found = finder::find_pnpmfile(root);
    assert!(found.unwrap().ends_with(".pnpmfile.mjs"));
}

#[test]
fn test_find_pnpmfile_fallback_to_cjs() {
    let tmp = TempDir::new().expect("temp dir");
    let root = tmp.path();

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
async fn read_package_fails_when_pnpmfile_requires_missing_module() {
    let err = read_package_err("module.exports = require('./this-does-not-exist')").await;
    eprintln!("err = {err}");
    assert!(err.contains("Error during pnpmfile execution"));
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
async fn custom_fetcher_works_with_mjs_pnpmfile() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.mjs");
    std::fs::write(
        &pnpmfile_path,
        r"
export const fetchers = [{
  canFetch (pkgId, resolution) {
    return resolution.type === '@custom/esm';
  },
  fetch (cafs, resolution) {
    return { filesIndex: { 'main.js': { integrity: 'sha512-esm' } } };
  },
}];
",
    )
    .expect("write pnpmfile");
    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);
    let fetchers = hooks.get_custom_fetchers().await.expect("load fetchers");

    assert_eq!(fetchers.len(), 1);
    let resolution = serde_json::json!({ "type": "@custom/esm" });
    assert!(fetchers[0].can_fetch("x@1.0.0", resolution.clone()).await.unwrap());
    let result = fetchers[0].fetch("x@1.0.0", resolution, serde_json::json!({})).await.unwrap();
    assert_eq!(result["filesIndex"]["main.js"]["integrity"], "sha512-esm");
}
