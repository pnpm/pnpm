use assert_cmd::prelude::*;
use pnpm_testing_utils::command_env::CommandTestExt;
use std::{fs, process::Command};

#[test]
fn configured_environment_changes_invalidate_task_outputs() {
    let project = tempfile::tempdir().unwrap();
    pnpm_testing_utils::git_repo::init_isolated_repo(project.path());
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

#[test]
fn submodule_projects_and_their_dependents_bypass_task_caching() {
    let root = tempfile::tempdir().unwrap();
    let repo = pnpm_testing_utils::git_repo::GitRepoFixture::init(root.path(), "workspace");
    let module = pnpm_testing_utils::git_repo::GitRepoFixture::init(root.path(), "module");
    module.write_file("input", "one");
    let _ = module.commit("initial submodule");
    let workspace = root.path().join("workspace-src");
    repo.write_file("package.json", r#"{"private":true}"#);
    repo.write_file(".gitignore", "node_modules/\n**/out/\n**/runs\n");
    repo.write_file("pnpm-workspace.yaml", "packages: ['packages/*']\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: ['^build']\n    outputs: ['out/**']\n");
    for (name, input) in [
        ("producer", "vendor/input"),
        ("consumer", "../producer/out/result"),
        ("independent", "input"),
    ] {
        let script = format!(
            r#"node -e "const fs=require('fs');fs.mkdirSync('out',{{recursive:true}});fs.writeFileSync('out/result',fs.existsSync('{input}')?fs.readFileSync('{input}'):'absent');fs.appendFileSync('runs','x')""#,
        );
        let mut manifest =
            serde_json::json!({"name":name,"version":"1.0.0","scripts":{"build":script}});
        if name == "consumer" {
            manifest["dependencies"] = serde_json::json!({"producer":"workspace:*"});
        }
        repo.write_file(&format!("packages/{name}/package.json"), &manifest.to_string());
    }
    repo.write_file("packages/independent/input", "independent");
    repo.write_file("packages/producer/a-unreadable-input", "source");
    Command::new("git")
        .current_dir(&workspace)
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-b",
            "main",
            &module.file_url(),
            "packages/producer/vendor",
        ])
        .assert()
        .success();
    let _ = repo.commit("workspace with submodule");
    fs::remove_file(workspace.join("packages/producer/a-unreadable-input")).unwrap();
    fs::create_dir(workspace.join("packages/producer/a-unreadable-input")).unwrap();
    let command = || {
        let mut command = Command::cargo_bin("pnpm").unwrap().without_ambient_pnpm_config();
        command
            .current_dir(&workspace)
            .env("XDG_CACHE_HOME", root.path().join("cache"))
            .env("XDG_CONFIG_HOME", root.path().join("config"));
        command
    };
    command().arg("install").assert().success();
    for (index, value) in ["one", "two", "two", "absent", "absent"].into_iter().enumerate() {
        if index == 4 {
            fs::remove_dir(workspace.join("packages/producer/vendor")).unwrap();
        } else if value == "absent" {
            Command::new("git")
                .current_dir(&workspace)
                .args(["submodule", "deinit", "-f", "--", "packages/producer/vendor"])
                .assert()
                .success();
        } else {
            fs::write(workspace.join("packages/producer/vendor/input"), value).unwrap();
        }
        command().args(["pipeline", "--full"]).assert().success();
        for name in ["producer", "consumer"] {
            assert_eq!(
                fs::read_to_string(workspace.join(format!("packages/{name}/out/result"))).unwrap(),
                value,
            );
            assert_eq!(
                fs::read_to_string(workspace.join(format!("packages/{name}/runs"))).unwrap(),
                "x".repeat(index + 1),
            );
        }
        assert_eq!(fs::read_to_string(workspace.join("packages/independent/runs")).unwrap(), "x");
    }
}

#[test]
fn projects_rooted_in_submodules_bypass_task_caching() {
    let root = tempfile::tempdir().unwrap();
    let repo = pnpm_testing_utils::git_repo::GitRepoFixture::init(root.path(), "workspace");
    let module = pnpm_testing_utils::git_repo::GitRepoFixture::init(root.path(), "module");
    let script = r#"node -e "const fs=require('fs');fs.mkdirSync('out',{recursive:true});fs.writeFileSync('out/result',fs.readFileSync('input'));fs.appendFileSync('runs','x')""#;
    module.write_file(
        "package.json",
        &serde_json::json!({"name":"producer","version":"1.0.0","scripts":{"build":script}})
            .to_string(),
    );
    module.write_file(".gitignore", "out/\nruns\nnode_modules/\n");
    module.write_file("input", "one");
    let _ = module.commit("initial submodule package");
    repo.write_file("package.json", r#"{"private":true}"#);
    repo.write_file(".gitignore", "node_modules/\n**/out/\n**/runs\n");
    repo.write_file("pnpm-workspace.yaml", "packages: ['packages/*']\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: ['^build']\n    outputs: ['out/**']\n");
    repo.write_file("packages/consumer/package.json", &serde_json::json!({"name":"consumer","version":"1.0.0","dependencies":{"producer":"workspace:*"},"scripts":{"build":script.replace("'input'", "'../producer/out/result'")}}).to_string());
    let workspace = root.path().join("workspace-src");
    Command::new("git")
        .current_dir(&workspace)
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-b",
            "main",
            &module.file_url(),
            "packages/producer",
        ])
        .assert()
        .success();
    let _ = repo.commit("workspace with submodule package");
    let command = || {
        let mut command = Command::cargo_bin("pnpm").unwrap().without_ambient_pnpm_config();
        command
            .current_dir(&workspace)
            .env("XDG_CACHE_HOME", root.path().join("cache"))
            .env("XDG_CONFIG_HOME", root.path().join("config"));
        command
    };
    command().arg("install").assert().success();
    for (index, value) in ["one", "one", "two", "two"].into_iter().enumerate() {
        fs::write(workspace.join("packages/producer/input"), value).unwrap();
        command().args(["pipeline", "--full"]).assert().success();
        for name in ["producer", "consumer"] {
            assert_eq!(
                fs::read_to_string(workspace.join(format!("packages/{name}/out/result"))).unwrap(),
                value,
            );
            assert_eq!(
                fs::read_to_string(workspace.join(format!("packages/{name}/runs"))).unwrap(),
                "x".repeat(index + 1),
            );
        }
    }
}

#[cfg(unix)]
fn symlinked_input_project(project: &std::path::Path, task_settings: &str) {
    use std::os::unix::fs::symlink;

    pnpm_testing_utils::git_repo::init_isolated_repo(project);
    fs::write(project.join("AGENTS.md"), "shared text").unwrap();
    fs::write(project.join("NOTES.md"), "shared text").unwrap();
    symlink("AGENTS.md", project.join("CLAUDE.md")).unwrap();
    symlink("MISSING.md", project.join("DANGLING.md")).unwrap();
    fs::write(project.join(".gitignore"), "out/\nnode_modules/\nruns\n").unwrap();
    fs::write(project.join("package.json"), serde_json::json!({
        "name": "probe", "version": "1.0.0", "scripts": {
            "build": r#"node -e "const fs=require('fs');fs.mkdirSync('out',{recursive:true});fs.writeFileSync('out/result',fs.readFileSync('CLAUDE.md','utf8'));fs.appendFileSync('runs','x')""#
        }
    }).to_string()).unwrap();
    fs::write(
        project.join("pnpm-workspace.yaml"),
        format!(
            "packages: []\nincludeWorkspaceRoot: true\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: []\n{task_settings}",
        ),
    )
    .unwrap();
    Command::new("git").current_dir(project).args(["add", "-A"]).assert().success();
    let inputs = pnpm_testing_utils::git_repo::unignored_files(project);
    assert!(
        inputs.iter().any(|path| path == "CLAUDE.md"),
        "the link the task reads must be one of the files pnpm hashes, or nothing below tests \
         what it claims to; git reported: {inputs:?}",
    );
}

#[cfg(unix)]
#[test]
fn no_cache_runs_tasks_without_hashing_their_inputs() {
    use std::os::unix::fs::symlink;

    let project = tempfile::tempdir().unwrap();
    symlinked_input_project(project.path(), "");
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("dir")).unwrap();
    fs::write(project.path().join("dir/input"), "source").unwrap();
    Command::new("git").current_dir(project.path()).args(["add", "-A"]).assert().success();
    fs::remove_file(project.path().join("dir/input")).unwrap();
    fs::remove_dir(project.path().join("dir")).unwrap();
    symlink(outside.path(), project.path().join("dir")).unwrap();
    let storage = tempfile::tempdir().unwrap();
    let command = || {
        let mut command = Command::cargo_bin("pnpm").unwrap().without_ambient_pnpm_config();
        command
            .current_dir(project.path())
            .env("XDG_CACHE_HOME", storage.path())
            .env("XDG_CONFIG_HOME", storage.path().join("config"));
        command
    };
    command().arg("install").assert().success();
    command().args(["pipeline", "--full", "--no-cache"]).assert().success();
    assert_eq!(fs::read_to_string(project.path().join("out/result")).unwrap(), "shared text");
    assert_eq!(fs::read_to_string(project.path().join("runs")).unwrap(), "x");
    // The symlinked directory a tracked input sits under is still refused,
    // which is what makes the run above evidence that nothing was hashed.
    command().args(["pipeline", "--full"]).assert().failure();
    assert_eq!(fs::read_to_string(project.path().join("runs")).unwrap(), "x");
}

#[cfg(unix)]
#[test]
fn symlinked_inputs_are_hashed_as_link_targets() {
    use std::os::unix::fs::symlink;

    let project = tempfile::tempdir().unwrap();
    symlinked_input_project(project.path(), "    outputs: ['out/**']\n");
    let storage = tempfile::tempdir().unwrap();
    let command = || {
        let mut command = Command::cargo_bin("pnpm").unwrap().without_ambient_pnpm_config();
        command
            .current_dir(project.path())
            .env("XDG_CACHE_HOME", storage.path())
            .env("XDG_CONFIG_HOME", storage.path().join("config"));
        command
    };
    command().arg("install").assert().success();
    let run = |expected_runs: &str, expected_hit: bool| {
        let result = command().args(["pipeline", "--full"]).assert().success();
        let output = String::from_utf8_lossy(&result.get_output().stdout).into_owned();
        assert_eq!(output.contains("restored from cache"), expected_hit, "{output}");
        assert_eq!(fs::read_to_string(project.path().join("runs")).unwrap(), expected_runs);
    };
    run("x", false);
    run("x", true);
    // Both targets hold the same text, so only a key built from the link
    // target itself — not from the content it resolves to — misses here.
    fs::remove_file(project.path().join("CLAUDE.md")).unwrap();
    symlink("NOTES.md", project.path().join("CLAUDE.md")).unwrap();
    run("xx", false);
    // The file a link points at is a tracked input in its own right, so
    // editing it still invalidates the task that reads it through the link.
    fs::write(project.path().join("NOTES.md"), "edited text").unwrap();
    run("xxx", false);
    assert_eq!(fs::read_to_string(project.path().join("out/result")).unwrap(), "edited text");
}
