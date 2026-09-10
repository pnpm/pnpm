use super::{Arc, Mutex, TempDir, write_custom_resolvers_pnpmfile};
use pnpm_hooks::PnpmfileHooks as _;

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

    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    let ctx = pnpm_hooks::PreResolutionHookContext {
        wanted_lockfile: serde_json::json!({}),
        current_lockfile: serde_json::json!({}),
        exists_current_lockfile: false,
        exists_non_empty_wanted_lockfile: false,
        lockfile_dir: "/test/lockfile".to_string(),
        store_dir: "/test/store".to_string(),
        registries: serde_json::json!({ "default": "http://localhost:1234/" }),
    };

    hooks
        .pre_resolution(
            ctx,
            pnpm_hooks::PreResolutionHookLogger { info: Arc::new(|_| {}), warn: Arc::new(|_| {}) },
        )
        .await;
}

#[tokio::test]
async fn test_node_js_hooks_pre_resolution_mjs() {
    let tmp = TempDir::new().expect("temp dir");
    let hooks_dir = tmp.path().join("hooks #100%");
    std::fs::create_dir(&hooks_dir).expect("create hooks dir");
    let pnpmfile_path = hooks_dir.join(".pnpmfile.mjs");
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
  logger.info('preResolution loaded');
}
",
    )
    .expect("write pnpmfile");

    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    let ctx = pnpm_hooks::PreResolutionHookContext {
        wanted_lockfile: serde_json::json!({}),
        current_lockfile: serde_json::json!({}),
        exists_current_lockfile: false,
        exists_non_empty_wanted_lockfile: false,
        lockfile_dir: "/test/lockfile".to_string(),
        store_dir: "/test/store".to_string(),
        registries: serde_json::json!({ "default": "http://localhost:1234/" }),
    };

    let info_messages = Arc::new(Mutex::new(Vec::new()));
    let captured_info_messages = Arc::clone(&info_messages);
    hooks
        .pre_resolution(
            ctx,
            pnpm_hooks::PreResolutionHookLogger {
                info: Arc::new(move |message| {
                    captured_info_messages.lock().unwrap().push(message);
                }),
                warn: Arc::new(|_| {}),
            },
        )
        .await;

    let info_messages = info_messages.lock().unwrap();
    dbg!(&*info_messages);
    assert_eq!(info_messages.as_slice(), ["preResolution loaded"]);
}

#[tokio::test]
async fn custom_resolver_should_refresh_resolution_receives_dep_path_and_snapshot() {
    let tmp = TempDir::new().expect("temp dir");
    let hooks =
        pnpm_hooks::node_runtime::NodeJsHooks::new(write_custom_resolvers_pnpmfile(tmp.path()));
    let resolvers = hooks.get_custom_resolvers().await.expect("load resolvers");
    let snapshot = serde_json::json!({ "resolution": { "integrity": "sha512-x" } });

    let matching: pnpm_lockfile::PackageKey = "refresh-me@1.0.0".parse().expect("valid dep path");
    let other: pnpm_lockfile::PackageKey = "other@1.0.0".parse().expect("valid dep path");

    assert!(
        resolvers[0]
            .should_refresh_resolution(&matching, snapshot.clone())
            .await
            .expect("shouldRefreshResolution"),
    );
    assert!(
        !resolvers[0]
            .should_refresh_resolution(&other, snapshot)
            .await
            .expect("shouldRefreshResolution"),
    );
}
