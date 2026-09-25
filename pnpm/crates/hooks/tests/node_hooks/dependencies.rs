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
async fn read_package_fails_when_a_dependency_range_is_not_a_string() {
    let err = read_package_err(
        "module.exports = { hooks: { readPackage: (pkg) => ({ ...pkg, dependencies: { ms: undefined } }) } }",
    )
    .await;
    eprintln!("err = {err}");
    assert!(err.contains(
        "readPackage hook returned an invalid range for 'ms' in the 'dependencies' of foo@1.0.0. \
         Expected a string, got undefined. To remove the dependency, delete the property."
    ));
    assert!(err.contains("Hook imported via"), "the hook's pnpmfile is named; got: {err}");
}

#[tokio::test]
async fn read_package_fails_when_a_peer_dependency_range_is_a_number() {
    let err = read_package_err(
        "module.exports = { hooks: { readPackage: (pkg) => ({ ...pkg, peerDependencies: { ms: 1 } }) } }",
    )
    .await;
    eprintln!("err = {err}");
    assert!(err.contains(
        "readPackage hook returned an invalid range for 'ms' in the 'peerDependencies' of foo@1.0.0. \
         Expected a string, got number. To remove the dependency, delete the property."
    ));
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
