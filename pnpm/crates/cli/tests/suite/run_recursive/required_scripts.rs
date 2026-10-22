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

/// A project without a `name` is listed by its directory.
#[test]
fn recursive_run_lists_a_nameless_project_by_its_directory() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", json!({ "name": "project-1", "version": "1.0.0" })),
            ("nameless", json!({ "version": "1.0.0" })),
        ],
    );
    require_scripts(&workspace, "build");

    let output = pacquet
        .with_args(["-r", "run", "build"])
        .output()
        .expect("spawn pacquet");

    assert!(!output.status.success(), "a project without the required script must fail the run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let unwrapped: String = stderr
        .chars()
        .filter(|char| !char.is_whitespace() && *char != '│')
        .collect();
    // The message spells the temp dir as the OS prints it (on Windows a
    // long or verbatim form of the same path), so match its last components.
    let nameless_suffix = Path::new("workspace")
        .join("nameless")
        .display()
        .to_string();
    let listed = unwrapped.contains(&nameless_suffix);
    assert!(listed, "the nameless project should be listed by its directory, got: {stderr}");
    assert!(unwrapped.contains("project-1"), "project-1 should be listed too, got: {stderr}");

    drop(root);
}

/// Only the projects selected for the run must have the script.
#[test]
fn filtered_recursive_run_checks_only_the_selected_projects() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", json!({ "name": "project-1", "version": "1.0.0" })),
            ("project-2", build_writes_marker("project-2")),
        ],
    );
    require_scripts(&workspace, "build");

    pacquet
        .with_args(["-r", "--filter", "project-2", "run", "build"])
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
