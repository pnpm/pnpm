use super::{CommandExtra, CommandTempCwd, Path, build_writes_marker, fs, json, write_workspace};
use assert_cmd::assert::OutputAssertExt as _;
use pnpm_testing_utils::diagnostics::assert_diagnostic_contains;

/// A script listed in `requiredScripts` must exist in every selected
/// project. The run fails before any script starts and names each project
/// that lacks it.
#[test]
fn recursive_run_fails_when_a_project_lacks_a_required_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", json!({ "name": "project-1", "version": "1.0.0" })),
            ("project-2", build_writes_marker("project-2")),
            ("project-3", json!({ "name": "project-3", "version": "1.0.0" })),
        ],
    );
    require_scripts(&workspace, "build");

    let output = pacquet
        .with_args(["-r", "run", "build"])
        .output()
        .expect("spawn pacquet");

    assert!(!output.status.success(), "a project without the required script must fail the run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_diagnostic_contains(&stderr, "ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT");
    assert_diagnostic_contains(
        &stderr,
        r#"Missing script "build" in packages: project-1, project-3"#,
    );
    assert!(
        !workspace.join("project-2/ran.txt").exists(),
        "no script may run once a required script is missing",
    );

    drop(root);
}

/// `requiredScripts` constrains only the scripts it lists.
#[test]
fn recursive_run_ignores_projects_without_an_unlisted_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", json!({ "name": "project-1", "version": "1.0.0" })),
            ("project-2", build_writes_marker("project-2")),
        ],
    );
    require_scripts(&workspace, "lint");

    pacquet
        .with_args(["-r", "run", "build"])
        .assert()
        .success();
    assert!(workspace.join("project-2/ran.txt").exists(), "project-2 should run build");

    drop(root);
}

fn require_scripts(workspace: &Path, script: &str) {
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml, format!("{yaml}requiredScripts:\n  - {script}\n"))
        .expect("write pnpm-workspace.yaml");
}
