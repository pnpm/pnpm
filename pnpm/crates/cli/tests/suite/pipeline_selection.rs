use assert_cmd::{assert::Assert, prelude::*};
use pnpm_testing_utils::{command_env::CommandTestExt, git_repo::GitRepoFixture};
use std::{fs, path::Path, process::Command};

const ROOT_MANIFEST: &str = r#"{"name":"root","version":"1.0.0","scripts":{"build":"echo root"}}"#;

fn pipeline(project: &Path, config_home: &Path, args: &[&str]) -> Assert {
    Command::cargo_bin("pnpm")
        .unwrap()
        .without_ambient_pnpm_config()
        .current_dir(project)
        .env("XDG_CONFIG_HOME", config_home)
        .arg("pipeline")
        .args(args)
        .assert()
        .success()
}

fn runs_root_build(document: &serde_json::Value) -> bool {
    document["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["project"] == "." && task["script"] == "build")
}

#[test]
fn a_root_only_workspace_selects_the_root() {
    for packages in ["packages: ['.']\n", ""] {
        let project = tempfile::tempdir().unwrap();
        fs::write(project.path().join("package.json"), ROOT_MANIFEST).unwrap();
        fs::write(
            project.path().join("pnpm-workspace.yaml"),
            format!("{packages}pipelines:\n  default: [build]\n"),
        )
        .unwrap();
        let result = pipeline(
            project.path(),
            &project.path().join("config"),
            &["--full", "--dry-run", "--json"],
        );
        let document: serde_json::Value =
            serde_json::from_slice(&result.get_output().stdout).unwrap();
        assert!(
            runs_root_build(&document),
            "the root must participate with {packages:?}: {document}",
        );
    }
}

#[test]
fn a_root_only_workspace_selects_the_root_for_a_change() {
    let root = tempfile::tempdir().unwrap();
    let fixture = GitRepoFixture::init(root.path(), "demo");
    fixture.write_file("package.json", ROOT_MANIFEST);
    fixture.write_file("pnpm-workspace.yaml", "packages: ['.']\npipelines:\n  default: [build]\n");
    fixture.write_file("source", "first");
    let base = fixture.commit("initial");
    fixture.write_file("source", "second");
    let result = pipeline(
        &root.path().join("demo-src"),
        &root.path().join("config"),
        &["--base", &base, "--dry-run", "--json"],
    );
    let output = result.get_output();
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(runs_root_build(&document), "the root must participate: {document}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("The diff touches workspace-root files"), "{stderr}");
}

#[test]
fn an_empty_full_selection_does_not_report_affected_projects() {
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("package.json"), ROOT_MANIFEST).unwrap();
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "packages: ['packages/*']\npipelines:\n  default: [build]\n",
    )
    .unwrap();
    let result = pipeline(project.path(), &project.path().join("config"), &["--full", "--dry-run"]);
    let output = String::from_utf8_lossy(&result.get_output().stdout).into_owned();
    assert!(output.contains("No projects are selected — nothing to run."), "{output}");
    assert!(!output.contains("No projects are affected since"), "{output}");
}
