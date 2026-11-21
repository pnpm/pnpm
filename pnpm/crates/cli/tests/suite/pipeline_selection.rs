use assert_cmd::{
    assert::Assert,
    prelude::*,
};
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    command_env::CommandTestExt,
    git_repo::GitRepoFixture,
};
use std::{
    fs,
    path::Path,
    process::Command,
};

const ROOT_MANIFEST: &str = r#"{"name":"root","version":"1.0.0","scripts":{"build":"echo root"}}"#;

fn pipeline(project: &Path, config_home: &Path, args: &[&str]) -> Assert {
    Command::cargo_bin("pnpm")
        .unwrap()
        .without_ambient_pnpm_config()
        .with_current_dir(project)
        .with_env("XDG_CONFIG_HOME", config_home)
        .with_env("XDG_CACHE_HOME", config_home.join("cache"))
        .with_arg("pipeline")
        .with_args(args)
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
    assert!(output.contains("No projects are selected. Nothing to run."), "{output}");
}

#[test]
fn pipeline_dry_run_json_routes_selection_warnings_to_stderr() {
    let root_change = pipeline_after_root_change(
        "packages: [pkg]\npipelines:\n  default: [build]\n",
        &["--dry-run", "--json"],
    );
    let root_stderr = String::from_utf8_lossy(&root_change.get_output().stderr);
    assert!(root_stderr.contains("The diff touches workspace-root files"), "stderr: {root_stderr}");
    let root_doc: serde_json::Value =
        serde_json::from_slice(&root_change.get_output().stdout).expect("valid dry-run JSON");
    assert_eq!(root_doc["tasks"][0]["project"], "pkg");

    let repo_root = tempfile::tempdir().unwrap();
    let fixture = GitRepoFixture::init(repo_root.path(), "demo");
    fixture.write_file("package.json", ROOT_MANIFEST);
    fixture.write_file("pnpm-workspace.yaml", "packages: [pkg]\npipelines:\n  default: [build]\n");
    fixture.write_file(
        "pkg/package.json",
        r#"{"name":"pkg","version":"1.0.0","scripts":{"build":"echo pkg"}}"#,
    );
    let _ = fixture.commit("initial");
    let missing_base = pipeline(
        &repo_root.path().join("demo-src"),
        &repo_root.path().join("config"),
        &["--base", "refs/heads/nonexistent-base", "--dry-run", "--json"],
    );
    let base_stderr = String::from_utf8_lossy(&missing_base.get_output().stderr);
    assert!(base_stderr.contains("Cannot resolve the merge base"), "stderr: {base_stderr}");
    let base_doc: serde_json::Value =
        serde_json::from_slice(&missing_base.get_output().stdout).expect("valid dry-run JSON");
    assert_eq!(base_doc["tasks"][0]["project"], "pkg");
}

#[test]
fn affected_selection_follows_overrides_to_a_changed_catalog_entry() {
    let root = tempfile::tempdir().unwrap();
    let fixture = GitRepoFixture::init(root.path(), "demo");
    let workspace_manifest = |lib: &str| {
        format!(
            "packages: [app, other]\npipelines:\n  default: [build]\nchangedFilesIgnorePattern: [pnpm-workspace.yaml]\noverrides:\n  lib: 'catalog:local'\ncatalogs:\n  local:\n    lib: {lib}\n",
        )
    };
    fixture.write_file("package.json", ROOT_MANIFEST);
    fixture.write_file("pnpm-workspace.yaml", &workspace_manifest("^1.0.0"));
    fixture.write_file(
        "app/package.json",
        r#"{"name":"app","version":"1.0.0","scripts":{"build":"echo app"},"dependencies":{"lib":"^1.0.0"}}"#,
    );
    fixture.write_file(
        "other/package.json",
        r#"{"name":"other","version":"1.0.0","scripts":{"build":"echo other"}}"#,
    );
    let base = fixture.commit("initial");
    fixture.write_file("pnpm-workspace.yaml", &workspace_manifest("^1.1.0"));

    let result = pipeline(
        &root.path().join("demo-src"),
        &root.path().join("config"),
        &["--base", &base, "--dry-run", "--json"],
    );
    let document: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    let projects: Vec<&str> = document["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|task| task["project"].as_str())
        .collect();
    assert_eq!(projects, ["app"], "{document}");
}

#[test]
fn filters_narrow_requested_tasks_but_keep_their_prerequisites() {
    let root = tempfile::tempdir().unwrap();
    let fixture = GitRepoFixture::init(root.path(), "filtered");
    fixture.write_file("package.json", r#"{"name":"root","private":true}"#);
    fixture.write_file("pnpm-workspace.yaml", "packages: [packages/*]\npipelines:\n  default: [build, lint]\ntasks:\n  build:\n    dependsOn: ['^build']\n  lint:\n    dependsOn: []\n");
    for name in ["app", "dep", "dev", "other"] {
        let dependencies = if name == "app" {
            serde_json::json!({"dep": "workspace:*"})
        } else {
            serde_json::json!({})
        };
        let dev_dependencies = if name == "app" {
            serde_json::json!({"dev": "workspace:*"})
        } else {
            serde_json::json!({})
        };
        fixture.write_file(
            &format!("packages/{name}/package.json"),
            &serde_json::json!({
                "name": name, "version": "1.0.0", "dependencies": dependencies,
                "devDependencies": dev_dependencies,
                "scripts": {"build": "echo build", "lint": "echo lint"}
            })
            .to_string(),
        );
    }
    let base = fixture.commit("initial");
    fixture.write_file("packages/dep/new-source", "untracked");
    for (flag, selector, expected_lint) in [
        ("--filter", "app", vec!["packages/app"]),
        ("--filter-prod", "app...", vec!["packages/app", "packages/dep"]),
        ("--filter", "missing", vec![]),
    ] {
        for mode in [vec!["--full"], vec!["--base", &base]] {
            let mut args = vec![flag, selector, "--dry-run", "--json"];
            args.extend(mode);
            let result =
                pipeline(&root.path().join("filtered-src"), &root.path().join("config"), &args);
            let document: serde_json::Value =
                serde_json::from_slice(&result.get_output().stdout).unwrap();
            assert_filtered_tasks(&document, &expected_lint);
        }
    }
}

fn assert_filtered_tasks(document: &serde_json::Value, expected_lint: &[&str]) {
    let tasks = document["tasks"].as_array().unwrap();
    let mut lint: Vec<_> = tasks
        .iter()
        .filter(|task| task["script"] == "lint")
        .map(|task| task["project"].as_str().unwrap())
        .collect();
    lint.sort_unstable();
    assert_eq!(lint, expected_lint, "{document}");
    assert!(
        tasks
            .iter()
            .all(|task| task["project"] != "packages/other"),
        "{document}",
    );
    if !expected_lint.is_empty() {
        assert!(
            tasks
                .iter()
                .any(|task| task["project"] == "packages/dep" && task["script"] == "build"),
            "{document}",
        );
    }
}

#[test]
fn regex_pipeline_tasks_use_the_matched_tasks_dependencies() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{"name":"regex-probe","scripts":{"prepare":"echo prepare","build:js":"echo build"}}"#,
    )
    .unwrap();
    fs::write(project.path().join("pnpm-workspace.yaml"), "packages: []\nincludeWorkspaceRoot: true\npipelines:\n  default: ['/^build:/']\ntasks:\n  build:js:\n    dependsOn: [prepare]\n  prepare:\n    dependsOn: []\n").unwrap();
    let storage = tempfile::tempdir().unwrap();
    let result = pipeline(project.path(), storage.path(), &["--full", "--dry-run", "--json"]);
    let document: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    let tasks = document["tasks"].as_array().unwrap();
    assert!(
        tasks
            .iter()
            .any(|task| task["script"] == "prepare"),
        "{document}",
    );
    assert!(
        tasks
            .iter()
            .any(|task| task["script"] == "build:js"),
        "{document}",
    );
    assert_eq!(tasks.len(), 2, "{document}");
}
