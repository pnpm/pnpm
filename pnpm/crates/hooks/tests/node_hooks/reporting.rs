use super::{
    Arc, TempDir, cjs_hooks, write_custom_fetchers_pnpmfile, write_custom_resolvers_pnpmfile,
};
use pnpm_hooks::PnpmfileHooks as _;

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
            pnpm_hooks::HookContext {
                log: Arc::new(move |message| sink.lock().unwrap().push(message)),
                dir: None,
            },
        )
        .await
        .expect("readPackage should succeed");

    assert_eq!(logs.lock().unwrap().as_slice(), &["hello from foo".to_string()]);
}

#[tokio::test]
async fn get_custom_resolvers_reports_per_resolver_capabilities() {
    let tmp = TempDir::new().expect("temp dir");
    let hooks =
        pnpm_hooks::node_runtime::NodeJsHooks::new(write_custom_resolvers_pnpmfile(tmp.path()));

    let resolvers = hooks.get_custom_resolvers().await.expect("load resolvers");

    assert_eq!(resolvers.len(), 2);
    assert!(resolvers[0].has_can_resolve());
    assert!(resolvers[0].has_resolve());
    assert!(resolvers[0].has_should_refresh_resolution());
    assert!(!resolvers[1].has_can_resolve());
    assert!(!resolvers[1].has_resolve());
    assert!(resolvers[1].has_should_refresh_resolution());
}

#[tokio::test]
async fn get_custom_fetchers_reports_per_fetcher_capabilities() {
    let tmp = TempDir::new().expect("temp dir");
    let hooks =
        pnpm_hooks::node_runtime::NodeJsHooks::new(write_custom_fetchers_pnpmfile(tmp.path()));

    let fetchers = hooks.get_custom_fetchers().await.expect("load fetchers");

    assert_eq!(fetchers.len(), 2);
    assert!(fetchers[0].has_can_fetch());
    assert!(fetchers[0].has_fetch());
    assert!(fetchers[1].has_can_fetch());
    assert!(!fetchers[1].has_fetch());
}
