use super::{_utils, CommandExtra, fs, pacquet_at, two_project_workspace};
use assert_cmd::assert::OutputAssertExt;
use std::fmt::Write as _;

/// Under a global virtual store, each project of a workspace that keeps a
/// lockfile per project still records what it installed in its own
/// `node_modules/.pnpm`, as it does without one. Recording it in the
/// workspace root's would have every project's install read the others'
/// packages as its own.
#[test]
fn dedicated_projects_keep_their_own_current_lockfile_under_a_global_virtual_store() {
    assert_dedicated_project_state(None);
}

#[test]
fn dedicated_projects_keep_state_local_with_an_explicit_global_virtual_store() {
    assert_dedicated_project_state(Some("shared-links"));
}

fn assert_dedicated_project_state(virtual_store_dir: Option<&str>) {
    let manifest = |name: &str, dependency: &str| {
        serde_json::json!({
            "name": name,
            "version": "1.0.0",
            "dependencies": { dependency: "1.0.0" },
        })
    };
    let fixture =
        two_project_workspace(&manifest("pkg-a", "is-positive"), &manifest("pkg-b", "is-negative"));
    let workspace = &fixture.workspace;
    let mut settings = "sharedWorkspaceLockfile: false\n".to_string();
    if let Some(dir) = virtual_store_dir {
        writeln!(settings, "virtualStoreDir: {dir}").unwrap();
    }
    _utils::enable_gvs_in_workspace_yaml(workspace, &settings);

    pacquet_at(workspace)
        .with_arg("install")
        .assert()
        .success();

    for (project, installed, sibling) in
        [("pkg-a", "is-positive", "is-negative"), ("pkg-b", "is-negative", "is-positive")]
    {
        let current_lockfile = workspace.join(project).join("node_modules/.pnpm/lock.yaml");
        let current = fs::read_to_string(&current_lockfile)
            .unwrap_or_else(|error| panic!("read {}: {error}", current_lockfile.display()));
        assert!(current.contains(installed), "{project} must record {installed}:\n{current}");
        assert!(!current.contains(sibling), "{project} must not record {sibling}:\n{current}");
    }
    let root_current =
        fs::read_to_string(workspace.join("node_modules/.pnpm/lock.yaml")).unwrap_or_default();
    assert!(
        !root_current.contains("is-positive") && !root_current.contains("is-negative"),
        "the root must not record its projects' packages:\n{root_current}",
    );

    if let Some(dir) = virtual_store_dir {
        let shared_store = fs::canonicalize(workspace.join(dir)).unwrap();
        assert!(!shared_store.join("lock.yaml").exists(), "shared store: {shared_store:?}");
        let package = fs::canonicalize(workspace.join("pkg-a/node_modules/is-positive")).unwrap();
        assert!(
            package.starts_with(&shared_store),
            "package: {package:?}, store: {shared_store:?}",
        );
    }

    drop(fixture);
}
