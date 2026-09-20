use super::{TempDir, finder, noop_context};
use pnpm_hooks::PnpmfileHooks as _;

#[tokio::test]
async fn update_config_applies_cjs_mjs_and_package_scoped_js_hook_results() {
    for (case_name, file_name, package_type, source) in [
        (
            "CommonJS pnpmfile in a module scope",
            "pnpmfile.cjs",
            "module",
            r"module.exports = { hooks: { updateConfig (config) {
  config.catalogs = { default: { foo: '1.0.0' } };
  return config;
} } }",
        ),
        (
            "ESM pnpmfile in a CommonJS scope",
            "pnpmfile.mjs",
            "commonjs",
            r"export const hooks = { updateConfig (config) {
  config.catalogs = { default: { foo: '1.0.0' } };
  return config;
} }",
        ),
        (
            "package-scoped ESM with top-level await",
            "pnpmfile.js",
            "module",
            r"await Promise.resolve();
export const hooks = { updateConfig (config) {
  config.catalogs = { default: { foo: '1.0.0' } };
  return config;
} }",
        ),
        (
            "package-scoped ESM exporting a default",
            "pnpmfile.js",
            "module",
            r"export default { hooks: { updateConfig (config) {
  config.catalogs = { default: { foo: '1.0.0' } };
  return config;
} } }",
        ),
        (
            "package-scoped CommonJS",
            "pnpmfile.js",
            "commonjs",
            r"module.exports = { hooks: { updateConfig (config) {
  config.catalogs = { default: { foo: '1.0.0' } };
  return config;
} } }",
        ),
    ] {
        let tmp = TempDir::new().expect("temp dir");
        std::fs::write(tmp.path().join("package.json"), opposite_package_type(package_type))
            .expect("write outer package manifest");
        let hooks_dir = tmp.path().join("hooks");
        std::fs::create_dir(&hooks_dir).expect("create hooks dir");
        std::fs::write(hooks_dir.join("package.json"), format!(r#"{{"type":"{package_type}"}}"#))
            .expect("write nearest package manifest");
        let pnpmfile_path = hooks_dir.join(file_name);
        std::fs::write(&pnpmfile_path, source).expect("write pnpmfile");

        let configured = [pnpmfile_path];
        let hooks = finder::load_pnpmfiles(
            tmp.path(),
            finder::PnpmfileSelection { configured: Some(&configured), global: None },
        )
        .expect("configured pnpmfile should exist")
        .expect("configured pnpmfile should load");
        let updated = hooks
            .update_config(serde_json::json!({ "registry": "https://r/" }), noop_context())
            .await
            .expect("updateConfig should succeed");

        assert_eq!(updated["registry"], "https://r/", "{case_name}: untouched keys are preserved");
        assert_eq!(
            updated["catalogs"]["default"]["foo"], "1.0.0",
            "{case_name}: hook-set key is applied",
        );
    }
}

fn opposite_package_type(package_type: &str) -> &'static str {
    match package_type {
        "module" => r#"{"type":"commonjs"}"#,
        "commonjs" => r#"{"type":"module"}"#,
        _ => unreachable!("test package type is fixed"),
    }
}

#[tokio::test]
async fn update_config_without_hook_returns_config_unchanged() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join("pnpmfile.cjs");
    std::fs::write(&pnpmfile_path, "module.exports = { hooks: {} }").expect("write pnpmfile");

    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);
    let config = serde_json::json!({ "registry": "https://r/" });
    let updated = hooks
        .update_config(config.clone(), noop_context())
        .await
        .expect("ok");

    assert!(!hooks.has_filter_log().await);
    assert_eq!(updated, config, "a pnpmfile without updateConfig leaves config unchanged");
}
