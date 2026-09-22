use super::{
    TempDir,
    finder,
};
use pnpm_hooks::PnpmfileHooks as _;

fn write_pnpmfile(dir: &std::path::Path, file_name: &str, source: &str) -> std::path::PathBuf {
    let path = dir.join(file_name);
    std::fs::write(&path, source).expect("write pnpmfile");
    path
}

/// Load a project pnpmfile plus a global one, the way
/// `PnpmfileSelection { configured, global }` combines them in a real
/// install: the global pnpmfile runs first and stays out of the
/// checksum.
fn load_with_global(
    project_dir: &TempDir,
    global_dir: &TempDir,
    project_source: &str,
    global_source: &str,
) -> std::sync::Arc<dyn pnpm_hooks::PnpmfileHooks> {
    let project = write_pnpmfile(project_dir.path(), ".pnpmfile.cjs", project_source);
    let global = write_pnpmfile(global_dir.path(), ".pnpmfile.cjs", global_source);
    finder::load_pnpmfiles(
        project_dir.path(),
        finder::PnpmfileSelection { configured: Some(&[project]), global: Some(&global) },
    )
    .expect("pnpmfiles load")
    .expect("at least one pnpmfile configured")
}

#[tokio::test]
async fn untracked_read_package_hook_detects_global_hook() {
    let project_dir = TempDir::new().expect("temp dir");
    let global_dir = TempDir::new().expect("temp dir");
    let hooks = load_with_global(
        &project_dir,
        &global_dir,
        "module.exports = { hooks: { updateConfig (config) { return config } } }",
        "module.exports = { hooks: { readPackage (pkg) { return pkg } } }",
    );
    assert!(
        hooks.untracked_read_package_hook().await.unwrap() == Some(true),
        "a readPackage hook on the checksum-excluded global pnpmfile must be reported",
    );
}

#[tokio::test]
async fn untracked_read_package_hook_stays_quiet_without_global_hook() {
    let project_dir = TempDir::new().expect("temp dir");
    let global_dir = TempDir::new().expect("temp dir");
    let hooks = load_with_global(
        &project_dir,
        &global_dir,
        "module.exports = { hooks: { updateConfig (config) { return config } } }",
        "module.exports = { hooks: { updateConfig (config) { return config } } }",
    );
    assert!(
        hooks.untracked_read_package_hook().await.unwrap() == Some(false),
        "a global pnpmfile without readPackage must not disable reuse",
    );
}

#[tokio::test]
async fn untracked_read_package_hook_ignores_project_hook() {
    // The project's own readPackage is checksum-tracked, so it stays a
    // checksum question, not an untracked one: edits to it surface through
    // `pnpmfileChecksum` and must not be double-counted here.
    let project_dir = TempDir::new().expect("temp dir");
    let global_dir = TempDir::new().expect("temp dir");
    let hooks = load_with_global(
        &project_dir,
        &global_dir,
        "module.exports = { hooks: { readPackage (pkg) { return pkg } } }",
        "module.exports = {}",
    );
    assert!(
        hooks.untracked_read_package_hook().await.unwrap() == Some(false),
        "a checksum-tracked project readPackage must not count as untracked",
    );
}

#[tokio::test]
async fn node_js_hooks_detects_read_package() {
    let tmp = TempDir::new().expect("temp dir");
    let with_hook = write_pnpmfile(
        tmp.path(),
        "with.cjs",
        "module.exports = { hooks: { readPackage (pkg) { return pkg } } }",
    );
    let without_hook = write_pnpmfile(
        tmp.path(),
        "without.cjs",
        "module.exports = { hooks: { updateConfig (config) { return config } } }",
    );
    assert!(
        pnpm_hooks::node_runtime::NodeJsHooks::new(with_hook).has_read_package().await.unwrap(),
        "exported readPackage must be detected",
    );
    assert!(
        !pnpm_hooks::node_runtime::NodeJsHooks::new(without_hook).has_read_package().await.unwrap(),
        "missing readPackage must not be reported",
    );
}
