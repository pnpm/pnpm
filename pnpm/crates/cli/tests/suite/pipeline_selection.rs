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
        .env("XDG_CACHE_HOME", config_home.join("cache"))
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

fn changed_pkg_selection(
    root_manifest: &str,
    pkg_manifest: &str,
    settings: &str,
) -> serde_json::Value {
    let root = tempfile::tempdir().unwrap();
    let fixture = GitRepoFixture::init(root.path(), "demo");
    fixture.write_file("package.json", root_manifest);
    fixture.write_file(
        "pnpm-workspace.yaml",
        &format!("packages: [pkg]\nincludeWorkspaceRoot: true\npipelines:\n  default: [build]\n{settings}"),
    );
    fixture.write_file("pkg/package.json", pkg_manifest);
    fixture.write_file("pkg/source", "first");
    let base = fixture.commit("initial");
    fixture.write_file("pkg/source", "second");
    let result = pipeline(
        &root.path().join("demo-src"),
        &root.path().join("config"),
        &["--base", &base, "--dry-run", "--json"],
    );
    serde_json::from_slice(&result.get_output().stdout).unwrap()
}

#[test]
fn affected_selection_keeps_an_enabled_workspace_root_dependent() {
    let document = changed_pkg_selection(
        r#"{"name":"root","private":true,"scripts":{"build":"echo root"},"dependencies":{"dep":"workspace:*"}}"#,
        r#"{"name":"dep","version":"1.0.0","scripts":{"build":"echo dep"}}"#,
        "tasks:\n  build:\n    dependsOn: []\n",
    );
    assert!(runs_root_build(&document), "root dependent must participate: {document}");
}

#[test]
fn affected_selection_keeps_the_workspace_root_as_an_upstream_dependency() {
    let document = changed_pkg_selection(
        r#"{"name":"root","version":"1.0.0","private":true,"scripts":{"build":"echo root"}}"#,
        r#"{"name":"dep","version":"1.0.0","scripts":{"build":"echo dep"},"dependencies":{"root":"workspace:*"}}"#,
        "tasks:\n  build:\n    dependsOn: ['^build']\n",
    );
    assert!(runs_root_build(&document), "root dependency must participate: {document}");
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

fn pipeline_after_root_change(workspace_manifest: &str, args: &[&str]) -> Assert {
    let root = tempfile::tempdir().unwrap();
    let fixture = GitRepoFixture::init(root.path(), "demo");
    fixture.write_file("package.json", ROOT_MANIFEST);
    fixture.write_file("pnpm-workspace.yaml", workspace_manifest);
    fixture.write_file(
        "pkg/package.json",
        r#"{"name":"pkg","version":"1.0.0","scripts":{"build":"echo pkg"}}"#,
    );
    fixture.write_file("source", "first");
    let base = fixture.commit("initial");
    fixture.write_file("source", "second");
    let args: Vec<&str> = ["--base", &base]
        .into_iter()
        .chain(args.iter().copied())
        .collect();
    pipeline(&root.path().join("demo-src"), &root.path().join("config"), &args)
}

#[test]
fn a_root_only_workspace_selects_the_root_for_a_change() {
    for packages in ["packages: ['.']\n", ""] {
        let result = pipeline_after_root_change(
            &format!("{packages}pipelines:\n  default: [build]\n"),
            &["--dry-run", "--json"],
        );
        let stdout = &result.get_output().stdout;
        let text = String::from_utf8_lossy(stdout);
        assert!(!text.contains("The diff touches workspace-root files"), "{text}");
        let document: serde_json::Value = serde_json::from_slice(stdout).unwrap();
        assert!(
            runs_root_build(&document),
            "the root must participate with {packages:?}: {document}",
        );
    }
}

#[test]
fn a_root_change_in_a_workspace_with_projects_runs_every_project() {
    let result = pipeline_after_root_change(
        "packages: [pkg]\npipelines:\n  default: [build]\n",
        &["--dry-run"],
    );
    let output = String::from_utf8_lossy(&result.get_output().stdout).into_owned();
    assert!(output.contains("The diff touches workspace-root files"), "{output}");
    assert!(output.contains("pkg#build"), "{output}");
}

#[test]
fn an_empty_full_selection_reports_no_selected_projects() {
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
}
