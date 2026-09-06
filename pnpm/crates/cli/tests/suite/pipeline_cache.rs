use assert_cmd::prelude::*;
use pnpm_testing_utils::command_env::CommandTestExt;
use std::{fs, process::Command};

#[test]
fn configured_environment_changes_invalidate_task_outputs() {
    let project = tempfile::tempdir().unwrap();
    assert!(Command::new("git").arg("init").arg(project.path()).output().unwrap().status.success());
    fs::create_dir(project.path().join("src")).unwrap();
    fs::write(project.path().join("src/input"), "source").unwrap();
    fs::write(project.path().join(".gitignore"), "out/\nnode_modules/\nhook-count\n").unwrap();
    fs::write(project.path().join("package.json"), serde_json::json!({
        "name": "probe", "version": "1.0.0", "scripts": {
            "build": r#"node -e "require('fs').mkdirSync('out',{recursive:true});require('fs').writeFileSync('out/result',process.env.BUILD_MODE)""#
        }
    }).to_string()).unwrap();
    fs::write(project.path().join(".pnpmfile.cjs"), r"module.exports = { hooks: { updateConfig(config) { const fs = require('fs'); const file = require('path').join(__dirname, 'hook-count'); fs.appendFileSync(file, 'called\n'); config.extraEnv = { ...config.extraEnv, BUILD_MODE: process.env.PIPELINE_TEST_MODE }; return config; } } }").unwrap();
    let storage = tempfile::tempdir().unwrap();
    let command = || {
        let mut command = Command::cargo_bin("pnpm").unwrap().without_ambient_pnpm_config();
        command
            .current_dir(project.path())
            .env("XDG_CACHE_HOME", storage.path())
            .env("XDG_CONFIG_HOME", storage.path().join("config"));
        command
    };
    for (index, value) in ["one", "two", "two"].into_iter().enumerate() {
        fs::write(project.path().join("pnpm-workspace.yaml"), String::from("packages: []\nincludeWorkspaceRoot: true\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: []\n    inputs: ['src/**']\n    outputs: ['out/**']\n    env: [BUILD_MODE]\n")).unwrap();
        if index == 0 {
            command()
                .env("PIPELINE_TEST_MODE", value)
                .env("BUILD_MODE", "ambient")
                .arg("install")
                .assert()
                .success();
        }
        fs::write(project.path().join("hook-count"), "").unwrap();
        let result = command()
            .env("PIPELINE_TEST_MODE", value)
            .env("BUILD_MODE", "ambient")
            .args(["pipeline", "--full"])
            .assert()
            .success();
        let output = String::from_utf8_lossy(&result.get_output().stdout);
        assert_eq!(output.contains("restored from cache"), index == 2, "{output}");
        assert_eq!(fs::read_to_string(project.path().join("out/result")).unwrap(), value);
        assert_eq!(fs::read_to_string(project.path().join("hook-count")).unwrap(), "called\n");
    }
    for reporter in ["ndjson", "silent"] {
        let result = command()
            .env("PIPELINE_TEST_MODE", "two")
            .args(["pipeline", "--full", "--reporter", reporter])
            .assert()
            .success();
        let output = String::from_utf8_lossy(&result.get_output().stdout);
        assert!(output.is_empty(), "pipeline must not print human output: {output}");
        let events = String::from_utf8_lossy(&result.get_output().stderr);
        if reporter == "silent" {
            assert!(events.is_empty(), "silent pipeline events: {events}");
        } else {
            assert!(!events.is_empty(), "NDJSON must contain pipeline events");
            for line in events.lines() {
                serde_json::from_str::<serde_json::Value>(line)
                    .expect("every NDJSON line must be JSON");
            }
        }
    }
}

#[test]
fn affected_selection_keeps_an_enabled_workspace_root_dependent() {
    let root = tempfile::tempdir().unwrap();
    let fixture = pnpm_testing_utils::git_repo::GitRepoFixture::init(root.path(), "demo");
    fixture.write_file("package.json", r#"{"name":"root","private":true,"scripts":{"build":"echo root"},"dependencies":{"dep":"workspace:*"}}"#);
    fixture.write_file("pnpm-workspace.yaml", "packages: [pkg]\nincludeWorkspaceRoot: true\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: []\n");
    fixture.write_file(
        "pkg/package.json",
        r#"{"name":"dep","version":"1.0.0","scripts":{"build":"echo dep"}}"#,
    );
    fixture.write_file("pkg/source", "first");
    let base = fixture.commit("initial");
    fixture.write_file("pkg/source", "second");
    let result = Command::cargo_bin("pnpm")
        .unwrap()
        .without_ambient_pnpm_config()
        .current_dir(root.path().join("demo-src"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .args(["pipeline", "--base", &base, "--dry-run", "--json"])
        .assert()
        .success();
    let document: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert!(
        document["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["project"] == "." && task["script"] == "build"),
        "root dependent must participate: {document}",
    );
}

#[test]
fn affected_selection_keeps_the_workspace_root_as_an_upstream_dependency() {
    let root = tempfile::tempdir().unwrap();
    let fixture = pnpm_testing_utils::git_repo::GitRepoFixture::init(root.path(), "demo");
    fixture.write_file(
        "package.json",
        r#"{"name":"root","version":"1.0.0","private":true,"scripts":{"build":"echo root"}}"#,
    );
    fixture.write_file("pnpm-workspace.yaml", "packages: [pkg]\nincludeWorkspaceRoot: true\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: ['^build']\n");
    fixture.write_file("pkg/package.json", r#"{"name":"dep","version":"1.0.0","scripts":{"build":"echo dep"},"dependencies":{"root":"workspace:*"}}"#);
    fixture.write_file("pkg/source", "first");
    let base = fixture.commit("initial");
    fixture.write_file("pkg/source", "second");
    let result = Command::cargo_bin("pnpm")
        .unwrap()
        .without_ambient_pnpm_config()
        .current_dir(root.path().join("demo-src"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .args(["pipeline", "--base", &base, "--dry-run", "--json"])
        .assert()
        .success();
    let document: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert!(
        document["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["project"] == "." && task["script"] == "build"),
        "root dependency must participate: {document}",
    );
}

#[test]
fn dry_run_prints_the_graph_without_executing_workspace_code() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        serde_json::json!({
            "name": "probe", "version": "1.0.0", "scripts": {
                "build": r#"node -e "throw new Error('dry-run executed build')""#
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(project.path().join("pnpm-workspace.yaml"), "packages: []\nincludeWorkspaceRoot: true\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: []\n").unwrap();
    fs::write(
        project.path().join(".pnpmfile.cjs"),
        "throw new Error('dry-run executed workspace configuration')",
    )
    .unwrap();
    let result = Command::cargo_bin("pnpm")
        .unwrap()
        .without_ambient_pnpm_config()
        .current_dir(project.path())
        .env("XDG_CONFIG_HOME", project.path().join("config"))
        .args(["pipeline", "--full", "--dry-run", "--json"])
        .assert()
        .success();
    let document: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(document["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(document["tasks"][0]["script"], "build");
    for path in ["node_modules", "pnpm-lock.yaml", "pnpm-lock.env.yaml"] {
        assert!(!project.path().join(path).exists(), "dry-run must not create {path}");
    }
}
