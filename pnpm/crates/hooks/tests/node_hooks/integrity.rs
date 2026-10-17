use super::{TempDir, finder};
use pnpm_hooks::PnpmfileHooks as _;

#[tokio::test]
async fn calculate_pnpmfile_checksum_hashes_normalized_contents_when_hooks_exported() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    let src = "module.exports = { hooks: { readPackage: (pkg) => pkg } }\n";
    std::fs::write(&pnpmfile_path, src).expect("write pnpmfile");

    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);
    let checksum = hooks.calculate_pnpmfile_checksum().await;

    assert_eq!(checksum, Some(pnpm_crypto_hash::create_hash(src)));
}

#[tokio::test]
async fn calculate_pnpmfile_checksum_is_none_when_no_hooks_exported() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(&pnpmfile_path, "module.exports = {}\n").expect("write pnpmfile");

    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    assert_eq!(hooks.calculate_pnpmfile_checksum().await, None);
}

/// No pnpmfile means no checksum, so a lockfile that records one is
/// drift — the removal itself is what the gate must see.
#[tokio::test]
async fn current_pnpmfile_checksum_is_none_without_a_pnpmfile() {
    assert_eq!(pnpm_hooks::current_pnpmfile_checksum(None, Some("sha256-abc")).await, None);
}

#[tokio::test]
async fn current_pnpmfile_checksum_hashes_the_pnpmfile_that_exports_hooks() {
    let tmp = TempDir::new().expect("temp dir");
    let src = "module.exports = { hooks: { readPackage: (pkg) => pkg } }\n";
    std::fs::write(tmp.path().join(".pnpmfile.cjs"), src).expect("write pnpmfile");

    let hooks = finder::load_pnpmfile(tmp.path());
    assert_eq!(
        pnpm_hooks::current_pnpmfile_checksum(hooks.as_ref(), None).await,
        Some(pnpm_crypto_hash::create_hash(src)),
    );
}

/// pnpm records no checksum for a pnpmfile that exports no `hooks`
/// object, so neither may pacquet — otherwise every install in a
/// project with a resolvers-only pnpmfile would report drift.
#[tokio::test]
async fn current_pnpmfile_checksum_is_none_when_the_pnpmfile_exports_no_hooks() {
    let tmp = TempDir::new().expect("temp dir");
    std::fs::write(tmp.path().join(".pnpmfile.cjs"), "module.exports = {}\n")
        .expect("write pnpmfile");

    let hooks = finder::load_pnpmfile(tmp.path());
    assert_eq!(pnpm_hooks::current_pnpmfile_checksum(hooks.as_ref(), None).await, None);
}

/// A lockfile that already records a checksum settles the comparison
/// by hash alone. Pinned with a pnpmfile that exports no hooks —
/// evaluating it would answer `None` and call an unchanged project
/// drifted.
#[tokio::test]
async fn current_pnpmfile_checksum_trusts_the_hash_when_the_lockfile_records_one() {
    let tmp = TempDir::new().expect("temp dir");
    let src = "module.exports = {}\n";
    std::fs::write(tmp.path().join(".pnpmfile.cjs"), src).expect("write pnpmfile");

    let hooks = finder::load_pnpmfile(tmp.path());
    let recorded = pnpm_crypto_hash::create_hash(src);
    assert_eq!(
        pnpm_hooks::current_pnpmfile_checksum(hooks.as_ref(), Some(&recorded)).await,
        Some(recorded),
    );
}

const ROOT_PNPMFILE: &str = "module.exports = { hooks: { readPackage: (pkg) => ({ ...pkg, order: (pkg.order ?? '') + 'root' }) } }\n";
const PROJECT_PNPMFILE: &str = "module.exports = { hooks: { readPackage: (pkg) => ({ ...pkg, order: (pkg.order ?? '') + 'project' }) } }\n";

/// A workspace root holding [`ROOT_PNPMFILE`] and a project under it, with
/// `project_pnpmfile` as the project's own when given.
fn workspace_with_project(project_pnpmfile: Option<&str>) -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().expect("temp dir");
    std::fs::write(tmp.path().join(".pnpmfile.cjs"), ROOT_PNPMFILE).expect("write root pnpmfile");
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).expect("create project");
    if let Some(source) = project_pnpmfile {
        std::fs::write(project.join(".pnpmfile.cjs"), source).expect("write project pnpmfile");
    }
    (tmp, project)
}

fn load_workspace_project(
    tmp: &TempDir,
    project: &std::path::Path,
) -> std::sync::Arc<dyn pnpm_hooks::PnpmfileHooks> {
    finder::load_workspace_project_pnpmfiles(
        project,
        tmp.path(),
        finder::PnpmfileSelection::default(),
    )
    .expect("no configured pnpmfile is missing")
    .expect("the workspace root has a pnpmfile")
}

/// pnpm v11 runs a project's own hooks before those it loaded at the
/// workspace root.
#[tokio::test]
async fn workspace_project_pnpmfiles_run_the_project_hook_before_the_root_hook() {
    let (tmp, project) = workspace_with_project(Some(PROJECT_PNPMFILE));
    let hooks = load_workspace_project(&tmp, &project);
    let pkg = hooks
        .read_package(serde_json::json!({ "name": "foo" }), super::noop_context())
        .await
        .expect("readPackage should succeed");
    assert_eq!(pkg["order"], "projectroot");
}

#[tokio::test]
async fn workspace_project_pnpmfile_checksum_covers_the_project_pnpmfile_when_it_has_one() {
    let (tmp, project) = workspace_with_project(Some(PROJECT_PNPMFILE));
    assert_eq!(
        load_workspace_project(&tmp, &project).calculate_pnpmfile_checksum().await,
        Some(pnpm_crypto_hash::create_hash(PROJECT_PNPMFILE)),
    );
}

#[tokio::test]
async fn workspace_project_pnpmfile_checksum_covers_the_root_pnpmfile_otherwise() {
    let (tmp, project) = workspace_with_project(None);
    let hooks = load_workspace_project(&tmp, &project);
    let pkg = hooks
        .read_package(serde_json::json!({ "name": "foo" }), super::noop_context())
        .await
        .expect("readPackage should succeed");
    assert_eq!(pkg["order"], "root");
    assert_eq!(
        hooks.calculate_pnpmfile_checksum().await,
        Some(pnpm_crypto_hash::create_hash(ROOT_PNPMFILE)),
    );
}

/// The global pnpmfile is not the project's: with no pnpmfile of its own, the
/// project's checksum covers the workspace root's.
#[tokio::test]
async fn workspace_project_pnpmfile_checksum_covers_the_root_pnpmfile_beside_a_global_one() {
    let (tmp, project) = workspace_with_project(None);
    let global = tmp.path().join("global.cjs");
    std::fs::write(&global, PROJECT_PNPMFILE).expect("write global pnpmfile");
    let hooks = finder::load_workspace_project_pnpmfiles(
        &project,
        tmp.path(),
        finder::PnpmfileSelection { global: Some(&global), ..Default::default() },
    )
    .expect("no configured pnpmfile is missing")
    .expect("the workspace root has a pnpmfile");
    assert_eq!(
        hooks.calculate_pnpmfile_checksum().await,
        Some(pnpm_crypto_hash::create_hash(ROOT_PNPMFILE)),
    );
}
