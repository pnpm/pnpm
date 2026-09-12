use super::{TempDir, noop_context};
use pnpm_hooks::PnpmfileHooks as _;

#[tokio::test]
async fn update_config_applies_cjs_and_mjs_hook_results() {
    for (file_name, source) in [
        (
            "pnpmfile.cjs",
            r"module.exports = { hooks: { updateConfig (config) {
  config.catalogs = { default: { foo: '1.0.0' } };
  return config;
} } }",
        ),
        (
            "pnpmfile.mjs",
            r"export const hooks = { updateConfig (config) {
  config.catalogs = { default: { foo: '1.0.0' } };
  return config;
} }",
        ),
    ] {
        let tmp = TempDir::new().expect("temp dir");
        let pnpmfile_path = tmp.path().join(file_name);
        std::fs::write(&pnpmfile_path, source).expect("write pnpmfile");

        let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);
        let updated = hooks
            .update_config(serde_json::json!({ "registry": "https://r/" }), noop_context())
            .await
            .expect("updateConfig should succeed");

        assert_eq!(updated["registry"], "https://r/", "untouched keys are preserved");
        assert_eq!(updated["catalogs"]["default"]["foo"], "1.0.0", "hook-set key is applied");
    }
}

#[tokio::test]
async fn update_config_without_hook_returns_config_unchanged() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join("pnpmfile.cjs");
    std::fs::write(&pnpmfile_path, "module.exports = { hooks: {} }").expect("write pnpmfile");

    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);
    let config = serde_json::json!({ "registry": "https://r/" });
    let updated = hooks.update_config(config.clone(), noop_context()).await.expect("ok");

    assert!(!hooks.has_filter_log().await);
    assert_eq!(updated, config, "a pnpmfile without updateConfig leaves config unchanged");
}
