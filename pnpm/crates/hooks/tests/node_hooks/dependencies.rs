use super::{Arc, cjs_hooks, read_package_err};
use pnpm_hooks::PnpmfileHooks as _;

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
            pnpm_hooks::HookContext { log: Arc::new(|_| {}), dir: None },
        )
        .await
        .expect("readPackage should succeed after normalization");
    assert_eq!(updated["dependencies"]["is-positive"], "*");
    assert_eq!(updated["peerDependencies"]["is-negative"], "*");
}
