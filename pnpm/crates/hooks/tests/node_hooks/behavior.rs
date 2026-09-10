use super::{
    Arc, TempDir, cjs_hooks, finder, read_package_err, write_custom_fetchers_pnpmfile,
    write_custom_resolvers_pnpmfile,
};
use pnpm_hooks::PnpmfileHooks as _;

#[tokio::test]
async fn read_package_fails_when_hook_returns_undefined() {
    let err = read_package_err("module.exports = { hooks: { readPackage (pkg) {} } }").await;
    eprintln!("err = {err}");
    assert!(err.contains("readPackage hook did not return a package manifest object."));
}

#[tokio::test]
async fn read_package_fails_with_meaningful_error_on_syntax_error() {
    let err = read_package_err("/boom").await;
    eprintln!("err = {err}");
    assert!(err.contains("Error during pnpmfile execution"));
    assert!(err.contains("SyntaxError"));
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
                    pnpm_hooks::HookContext { log: Arc::new(|_| {}), dir: None },
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

#[tokio::test]
async fn get_custom_resolvers_is_empty_without_resolvers_export() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(&pnpmfile_path, "module.exports = { hooks: {} }").expect("write pnpmfile");
    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    let resolvers = hooks.get_custom_resolvers().await.expect("load resolvers");

    assert!(resolvers.is_empty());
}

#[tokio::test]
async fn custom_resolver_round_trips_can_resolve_and_resolve() {
    let tmp = TempDir::new().expect("temp dir");
    let hooks =
        pnpm_hooks::node_runtime::NodeJsHooks::new(write_custom_resolvers_pnpmfile(tmp.path()));
    let resolvers = hooks.get_custom_resolvers().await.expect("load resolvers");
    let wanted = serde_json::json!({ "alias": "foo", "bareSpecifier": "custom:foo" });

    assert!(resolvers[0].can_resolve(wanted.clone()).await.expect("canResolve"));
    assert!(
        !resolvers[0]
            .can_resolve(serde_json::json!({ "alias": "bar", "bareSpecifier": "^1.0.0" }))
            .await
            .expect("canResolve"),
    );

    let result = resolvers[0]
        .resolve(wanted, serde_json::json!({ "lockfileDir": "/repo" }))
        .await
        .expect("resolve");
    // `custom/` prefix comes from `this.idPrefix`: methods must be
    // invoked with the resolver object as `this`, like pnpm does.
    assert_eq!(result["id"], "custom/foo@1.0.0");
    assert_eq!(result["resolution"]["tarball"], "https://example.com/foo-1.0.0.tgz");
    assert_eq!(result["lockfileDir"], "/repo", "resolve opts reach the hook");
}

#[tokio::test]
async fn custom_resolver_errors_propagate() {
    let tmp = TempDir::new().expect("temp dir");
    let hooks =
        pnpm_hooks::node_runtime::NodeJsHooks::new(write_custom_resolvers_pnpmfile(tmp.path()));
    let resolvers = hooks.get_custom_resolvers().await.expect("load resolvers");
    let dep_path: pnpm_lockfile::PackageKey = "any@1.0.0".parse().expect("valid dep path");

    let err = resolvers[1]
        .should_refresh_resolution(&dep_path, serde_json::json!({}))
        .await
        .expect_err("throwing hook must surface as an error");

    assert!(err.to_string().contains("refresh check crashed"), "got: {err}");
}

#[tokio::test]
async fn get_custom_fetchers_is_empty_without_fetchers_export() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(&pnpmfile_path, "module.exports = { hooks: {} }").expect("write pnpmfile");
    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    let fetchers = hooks.get_custom_fetchers().await.expect("load fetchers");

    assert!(fetchers.is_empty());
}

#[tokio::test]
async fn custom_fetcher_round_trips_can_fetch_and_fetch() {
    let tmp = TempDir::new().expect("temp dir");
    let hooks =
        pnpm_hooks::node_runtime::NodeJsHooks::new(write_custom_fetchers_pnpmfile(tmp.path()));
    let fetchers = hooks.get_custom_fetchers().await.expect("load fetchers");
    let resolution =
        serde_json::json!({ "type": "@custom/local", "url": "https://example.com/pkg" });

    assert!(fetchers[0].can_fetch("foo@1.0.0", resolution.clone()).await.expect("canFetch"));
    assert!(
        !fetchers[0]
            .can_fetch("foo@1.0.0", serde_json::json!({ "type": "tarball" }))
            .await
            .expect("canFetch"),
    );

    let opts = serde_json::json!({ "pkg": { "name": "foo", "version": "1.0.0" } });
    let result = fetchers[0].fetch("foo@1.0.0", resolution, opts.clone()).await.expect("fetch");
    assert_eq!(result["filesIndex"]["package.json"]["integrity"], "sha512-abc123");
    // TS-parity positions: `cafs` / `fetchers` are null placeholders
    // over IPC, `resolution` and `opts` arrive in the TS slots.
    assert_eq!(result["receivedNullCafs"], true);
    assert_eq!(result["receivedNullFetchers"], true);
    assert_eq!(result["receivedUrl"], "https://example.com/pkg");
    assert_eq!(result["receivedOpts"], opts);
}

#[tokio::test]
async fn custom_fetcher_errors_propagate() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r"
module.exports = {
  fetchers: [{
    canFetch () { return true; },
    fetch () { throw new Error('fetch crashed'); },
  }],
}
",
    )
    .expect("write pnpmfile");
    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);
    let fetchers = hooks.get_custom_fetchers().await.expect("load fetchers");

    let err = fetchers[0]
        .fetch("foo@1.0.0", serde_json::json!({}), serde_json::json!({}))
        .await
        .expect_err("throwing fetch must surface as an error");

    assert!(err.to_string().contains("fetch crashed"), "got: {err}");
}

#[tokio::test]
async fn custom_fetcher_delegate_response_round_trips() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r#"
module.exports = {
  fetchers: [{
    canFetch (pkgId, resolution) {
      return resolution.type === '@custom/proxy';
    },
    fetch (cafs, resolution, opts, fetchers) {
      return {
        delegate: {
          tarball: resolution.proxyUrl,
          integrity: "sha512-delegated",
        },
      };
    },
  }],
}
"#,
    )
    .expect("write pnpmfile");
    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);
    let fetchers = hooks.get_custom_fetchers().await.expect("load fetchers");

    let resolution = serde_json::json!({
        "type": "@custom/proxy",
        "proxyUrl": "https://proxy.example.com/foo-1.0.0.tgz",
    });
    assert!(fetchers[0].can_fetch("foo@1.0.0", resolution.clone()).await.unwrap());
    let result = fetchers[0].fetch("foo@1.0.0", resolution, serde_json::json!({})).await.unwrap();
    assert_eq!(result["delegate"]["tarball"], "https://proxy.example.com/foo-1.0.0.tgz");
    assert_eq!(result["delegate"]["integrity"], "sha512-delegated");
}

#[tokio::test]
async fn custom_fetcher_can_fetch_truthy_values() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r#"
module.exports = {
  fetchers: [
    { canFetch() { return 1; }, fetch() { return { delegate: {} }; } },
    { canFetch() { return "yes"; }, fetch() { return { delegate: {} }; } },
    { canFetch() { return 0; }, fetch() { return { delegate: {} }; } },
    { canFetch() { return ""; }, fetch() { return { delegate: {} }; } },
  ],
}
"#,
    )
    .expect("write pnpmfile");
    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);
    let fetchers = hooks.get_custom_fetchers().await.expect("load fetchers");
    let empty_resolution = serde_json::json!({});

    assert!(
        fetchers[0].can_fetch("a@1.0.0", empty_resolution.clone()).await.unwrap(),
        "1 is truthy",
    );
    assert!(
        fetchers[1].can_fetch("a@1.0.0", empty_resolution.clone()).await.unwrap(),
        r#""yes" is truthy"#,
    );
    assert!(
        !fetchers[2].can_fetch("a@1.0.0", empty_resolution.clone()).await.unwrap(),
        "0 is falsy",
    );
    assert!(!fetchers[3].can_fetch("a@1.0.0", empty_resolution).await.unwrap(), r#""" is falsy"#);
}
