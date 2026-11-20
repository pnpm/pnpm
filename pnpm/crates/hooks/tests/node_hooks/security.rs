use super::TempDir;
use pnpm_hooks::PnpmfileHooks as _;

#[tokio::test]
async fn custom_fetcher_null_entries_are_skipped() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r"
module.exports = {
  fetchers: [
    null,
    undefined,
    { canFetch() { return true; }, fetch() { return { delegate: {} }; } },
  ],
}
",
    )
    .expect("write pnpmfile");
    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);
    let fetchers = hooks.get_custom_fetchers().await.expect("load fetchers");

    assert_eq!(fetchers.len(), 3);
    assert!(!fetchers[0].has_can_fetch());
    assert!(!fetchers[0].has_fetch());
    assert!(!fetchers[1].has_can_fetch());
    assert!(!fetchers[1].has_fetch());
    assert!(fetchers[2].has_can_fetch());
    assert!(fetchers[2].has_fetch());
}
