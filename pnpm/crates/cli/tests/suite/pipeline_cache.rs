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
    assert_eq!(
        document["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1,
    );
    assert_eq!(document["tasks"][0]["script"], "build");
    for path in ["node_modules", "pnpm-lock.yaml", "pnpm-lock.env.yaml"] {
        assert!(!project.path().join(path).exists(), "dry-run must not create {path}");
    }
}

#[test]
fn empty_selection_dry_run_prints_empty_graph_without_a_report() {
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("package.json"), r#"{"name":"a","scripts":{"build":"echo B"}}"#)
        .unwrap();
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "packages: ['packages/*']\npipelines:\n  default: [build]\n",
    )
    .unwrap();
    let storage = tempfile::tempdir().unwrap();
    let report_root = storage
        .path()
        .join("pnpm")
        .join("pipeline");
    let command = || {
        let mut command = Command::cargo_bin("pnpm").unwrap().without_ambient_pnpm_config();
        command
            .current_dir(project.path())
            .env("XDG_CACHE_HOME", storage.path())
            .env("XDG_CONFIG_HOME", storage.path().join("config"));
        command
    };
    let result = command()
        .args(["pipeline", "--full", "--dry-run", "--json"])
        .assert()
        .success();
    let document: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(document, serde_json::json!({"tasks": []}));
    assert!(!report_root.exists());
    let result = command()
        .args(["pipeline", "--full", "--dry-run"])
        .assert()
        .success();
    assert_eq!(
        String::from_utf8_lossy(&result.get_output().stdout),
        "No projects are selected — nothing to run.\n",
    );
    assert!(!report_root.exists());
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
    command()
        .arg("install")
        .assert()
        .success();
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
        command()
            .args(["pipeline", "--full"])
            .assert()
            .success();
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
    command()
        .arg("install")
        .assert()
        .success();
    for (index, value) in ["one", "one", "two", "two"].into_iter().enumerate() {
        fs::write(workspace.join("packages/producer/input"), value).unwrap();
        command()
            .args(["pipeline", "--full"])
            .assert()
            .success();
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
    Command::new("git")
        .current_dir(project)
        .args(["add", "-A"])
        .assert()
        .success();
    let inputs = pnpm_testing_utils::git_repo::unignored_files(project);
    assert!(
        inputs
            .iter()
            .any(|path| path == "CLAUDE.md"),
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
    Command::new("git")
        .current_dir(project.path())
        .args(["add", "-A"])
        .assert()
        .success();
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
    command()
        .arg("install")
        .assert()
        .success();
    command()
        .args(["pipeline", "--full", "--no-cache"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(project.path().join("out/result")).unwrap(), "shared text");
    assert_eq!(fs::read_to_string(project.path().join("runs")).unwrap(), "x");
    // The symlinked directory a tracked input sits under is still refused,
    // which is what makes the run above evidence that nothing was hashed.
    command()
        .args(["pipeline", "--full"])
        .assert()
        .failure();
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
    command()
        .arg("install")
        .assert()
        .success();
    let run = |expected_runs: &str, expected_hit: bool| {
        let result = command()
            .args(["pipeline", "--full"])
            .assert()
            .success();
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

#[test]
fn a_project_outside_a_work_tree_runs_its_tasks_without_the_cache() {
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("package.json"), serde_json::json!({
        "name": "probe", "version": "1.0.0", "scripts": {
            "build": r#"node -e "const fs=require('fs');fs.mkdirSync('out',{recursive:true});fs.writeFileSync('out/result','built');fs.appendFileSync('runs','x')""#
        }
    }).to_string()).unwrap();
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "packages: []\nincludeWorkspaceRoot: true\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: []\n    outputs: ['out/**']\n",
    )
    .unwrap();
    let storage = tempfile::tempdir().unwrap();
    let command = || {
        let mut command = Command::cargo_bin("pnpm").unwrap().without_ambient_pnpm_config();
        command
            .current_dir(project.path())
            .env("XDG_CACHE_HOME", storage.path())
            .env("XDG_CONFIG_HOME", storage.path().join("config"));
        command
    };
    command()
        .arg("install")
        .assert()
        .success();
    // The task's default inputs are `git ls-files`, which a project outside
    // a work tree cannot answer. The task must still run: it is excluded
    // from the cache, and the exclusion is reported rather than silent.
    let run = |args: &[&str], expected_runs: &str| {
        let result = command().args(args).assert().success();
        let output = String::from_utf8_lossy(&result.get_output().stdout).into_owned();
        assert!(output.contains("Cannot enumerate the tracked files"), "{output}");
        assert!(!output.contains("restored from cache"), "{output}");
        assert_eq!(fs::read_to_string(project.path().join("runs")).unwrap(), expected_runs);
    };
    run(&["pipeline"], "x");
    run(&["pipeline", "--full"], "xx");
    run(&["pipeline", "--full"], "xxx");
    assert_eq!(fs::read_to_string(project.path().join("out/result")).unwrap(), "built");
}

fn run_pipeline_cmd(
    workspace: &std::path::Path,
    storage: &std::path::Path,
    extra_env: &[(&str, &str)],
    args: &[&str],
) -> String {
    let mut cmd = Command::cargo_bin("pnpm").unwrap().without_ambient_pnpm_config();
    cmd.current_dir(workspace)
        .env("XDG_CACHE_HOME", storage.join("cache"))
        .env("XDG_CONFIG_HOME", storage.join("config"));
    for (key, val) in extra_env {
        cmd.env(key, val);
    }
    let result = cmd.args(args).assert().success();
    String::from_utf8_lossy(&result.get_output().stdout).into_owned()
}

#[test]
fn additive_empty_and_excluded_inputs_control_cache_invalidation() {
    let root = tempfile::tempdir().unwrap();
    let repo = pnpm_testing_utils::git_repo::GitRepoFixture::init(root.path(), "ws");
    let workspace = root.path().join("ws-src");
    let storage = root.path().join("storage");
    repo.write_file("package.json", r#"{"private":true}"#);
    repo.write_file(".gitignore", "node_modules/\n**/dist/\n**/*-runs\n**/.env.local\n");
    repo.write_file(
        "pnpm-workspace.yaml",
        "packages: ['packages/*']\npipelines:\n  default: [build, stamp]\ntasks:\n  build:\n    dependsOn: []\n    inputs: ['+.env.local', '+../shared.txt', '!README.md']\n    outputs: ['dist/out.txt']\n  stamp:\n    dependsOn: []\n    inputs: []\n    outputs: ['dist/stamp.txt']\n",
    );
    let app_script = r#"node -e "const fs=require('fs');fs.mkdirSync('dist',{recursive:true});const env=fs.existsSync('.env.local')?fs.readFileSync('.env.local','utf8'):'none';const shared=fs.readFileSync('../shared.txt','utf8');const src=fs.readFileSync('src.txt','utf8');fs.writeFileSync('dist/out.txt',[src,env,shared].join(':'));fs.appendFileSync('build-runs','x')""#;
    let stamp_script = r#"node -e "const fs=require('fs');fs.mkdirSync('dist',{recursive:true});fs.writeFileSync('dist/stamp.txt','ok');fs.appendFileSync('stamp-runs','s')""#;
    repo.write_file(
        "packages/app/package.json",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "scripts": { "build": app_script, "stamp": stamp_script }
        })
        .to_string(),
    );
    repo.write_file("packages/app/src.txt", "src1");
    repo.write_file("packages/app/README.md", "doc1");
    repo.write_file("packages/shared.txt", "shared1");
    let _ = repo.commit("initial workspace");

    run_pipeline_cmd(&workspace, &storage, &[], &["install"]);
    run_pipeline_cmd(&workspace, &storage, &[], &["pipeline", "--full"]);
    assert_eq!(fs::read_to_string(workspace.join("packages/app/build-runs")).unwrap(), "x");
    assert_eq!(fs::read_to_string(workspace.join("packages/app/stamp-runs")).unwrap(), "s");
    assert_eq!(
        fs::read_to_string(workspace.join("packages/app/dist/out.txt")).unwrap(),
        "src1:none:shared1",
    );

    repo.write_file("packages/app/README.md", "doc2");
    run_pipeline_cmd(&workspace, &storage, &[], &["pipeline", "--full"]);
    assert_eq!(fs::read_to_string(workspace.join("packages/app/build-runs")).unwrap(), "x");
    assert_eq!(fs::read_to_string(workspace.join("packages/app/stamp-runs")).unwrap(), "s");

    fs::write(workspace.join("packages/app/.env.local"), "env1").unwrap();
    run_pipeline_cmd(&workspace, &storage, &[], &["pipeline", "--full"]);
    assert_eq!(fs::read_to_string(workspace.join("packages/app/build-runs")).unwrap(), "xx");
    assert_eq!(fs::read_to_string(workspace.join("packages/app/stamp-runs")).unwrap(), "s");
    assert_eq!(
        fs::read_to_string(workspace.join("packages/app/dist/out.txt")).unwrap(),
        "src1:env1:shared1",
    );

    repo.write_file("packages/shared.txt", "shared2");
    run_pipeline_cmd(&workspace, &storage, &[], &["pipeline", "--full"]);
    assert_eq!(fs::read_to_string(workspace.join("packages/app/build-runs")).unwrap(), "xxx");
    assert_eq!(fs::read_to_string(workspace.join("packages/app/stamp-runs")).unwrap(), "s");
    assert_eq!(
        fs::read_to_string(workspace.join("packages/app/dist/out.txt")).unwrap(),
        "src1:env1:shared2",
    );

    repo.write_file("packages/app/src.txt", "src2");
    run_pipeline_cmd(&workspace, &storage, &[], &["pipeline", "--full"]);
    assert_eq!(fs::read_to_string(workspace.join("packages/app/build-runs")).unwrap(), "xxxx");
    assert_eq!(fs::read_to_string(workspace.join("packages/app/stamp-runs")).unwrap(), "s");
    assert_eq!(
        fs::read_to_string(workspace.join("packages/app/dist/out.txt")).unwrap(),
        "src2:env1:shared2",
    );
}

#[test]
fn uncacheable_depends_on_tasks_force_dependents_to_rerun_while_passthrough_caches() {
    let root = tempfile::tempdir().unwrap();
    let repo = pnpm_testing_utils::git_repo::GitRepoFixture::init(root.path(), "ws");
    let workspace = root.path().join("ws-src");
    let storage = root.path().join("storage");
    repo.write_file("package.json", r#"{"private":true}"#);
    repo.write_file(".gitignore", "node_modules/\n**/dist/\n**/runs\n**/generated.tmp\n");
    repo.write_file(
        "pnpm-workspace.yaml",
        "packages: ['packages/*']\npipelines:\n  default: [bundle, chain]\ntasks:\n  codegen:\n    dependsOn: []\n    cache: false\n    outputs: ['dist/**']\n  bundle:\n    dependsOn: ['codegen']\n    inputs: []\n    outputs: ['dist/**']\n  chain:\n    dependsOn: ['^chain']\n    outputs: ['dist/**']\n",
    );
    let codegen_script =
        r#"node -e "require('fs').writeFileSync('generated.tmp',process.env.GEN_VAL)""#;
    let bundle_script = r#"node -e "const fs=require('fs');fs.mkdirSync('dist',{recursive:true});fs.writeFileSync('dist/out.txt',fs.readFileSync('generated.tmp','utf8'));fs.appendFileSync('runs','b')""#;
    let leaf_script = r#"node -e "const fs=require('fs');fs.mkdirSync('dist',{recursive:true});fs.writeFileSync('dist/leaf.txt',fs.readFileSync('input.txt','utf8'));fs.appendFileSync('runs','l')""#;
    let top_script = r#"node -e "const fs=require('fs');fs.mkdirSync('dist',{recursive:true});fs.writeFileSync('dist/top.txt',fs.readFileSync('../leaf/dist/leaf.txt','utf8'));fs.appendFileSync('runs','t')""#;
    repo.write_file(
        "packages/leaf/package.json",
        &serde_json::json!({
            "name": "leaf",
            "version": "1.0.0",
            "scripts": { "codegen": codegen_script, "bundle": bundle_script, "chain": leaf_script }
        })
        .to_string(),
    );
    repo.write_file("packages/leaf/input.txt", "v1");
    repo.write_file(
        "packages/middle/package.json",
        &serde_json::json!({
            "name": "middle",
            "version": "1.0.0",
            "dependencies": { "leaf": "workspace:*" }
        })
        .to_string(),
    );
    repo.write_file(
        "packages/top/package.json",
        &serde_json::json!({
            "name": "top",
            "version": "1.0.0",
            "dependencies": { "middle": "workspace:*" },
            "scripts": { "chain": top_script }
        })
        .to_string(),
    );
    let _ = repo.commit("pipeline graph");

    run_pipeline_cmd(&workspace, &storage, &[("GEN_VAL", "one")], &["install"]);
    run_pipeline_cmd(&workspace, &storage, &[("GEN_VAL", "one")], &["pipeline", "--full"]);
    assert_eq!(fs::read_to_string(workspace.join("packages/leaf/dist/out.txt")).unwrap(), "one");
    assert_eq!(fs::read_to_string(workspace.join("packages/top/runs")).unwrap(), "t");

    run_pipeline_cmd(&workspace, &storage, &[("GEN_VAL", "two")], &["pipeline", "--full"]);
    assert_eq!(fs::read_to_string(workspace.join("packages/leaf/dist/out.txt")).unwrap(), "two");
    assert_eq!(fs::read_to_string(workspace.join("packages/top/runs")).unwrap(), "t");

    repo.write_file("packages/leaf/input.txt", "v2");
    run_pipeline_cmd(&workspace, &storage, &[("GEN_VAL", "three")], &["pipeline", "--full"]);
    assert_eq!(fs::read_to_string(workspace.join("packages/leaf/dist/out.txt")).unwrap(), "three");
    assert_eq!(fs::read_to_string(workspace.join("packages/top/dist/top.txt")).unwrap(), "v2");
    assert_eq!(fs::read_to_string(workspace.join("packages/top/runs")).unwrap(), "tt");
}

#[test]
fn readonly_outputs_restore_from_cache_without_permission_errors() {
    let project = tempfile::tempdir().unwrap();
    let storage = tempfile::tempdir().unwrap();
    pnpm_testing_utils::git_repo::init_isolated_repo(project.path());
    fs::write(project.path().join(".gitignore"), "node_modules/\ndist/\nruns\n").unwrap();
    let script = r#"node -e "const fs=require('fs');fs.mkdirSync('dist',{recursive:true});fs.rmSync('dist/out.txt',{force:true});fs.writeFileSync('dist/out.txt',process.env.MODE);fs.chmodSync('dist/out.txt',0o444);fs.appendFileSync('runs','x')""#;
    fs::write(
        project.path().join("package.json"),
        serde_json::json!({
            "name": "readonly-probe",
            "version": "1.0.0",
            "scripts": { "build": script }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "packages: []\nincludeWorkspaceRoot: true\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: []\n    inputs: []\n    outputs: ['dist/**']\n    env: [MODE]\n",
    )
    .unwrap();

    run_pipeline_cmd(project.path(), storage.path(), &[("MODE", "one")], &["install"]);
    let first = run_pipeline_cmd(
        project.path(),
        storage.path(),
        &[("MODE", "one")],
        &["pipeline", "--full"],
    );
    assert!(!first.contains("restored from cache"), "{first}");
    let second = run_pipeline_cmd(
        project.path(),
        storage.path(),
        &[("MODE", "two")],
        &["pipeline", "--full"],
    );
    assert!(!second.contains("restored from cache"), "{second}");
    let restored = run_pipeline_cmd(
        project.path(),
        storage.path(),
        &[("MODE", "one")],
        &["pipeline", "--full"],
    );
    assert!(restored.contains("restored from cache"), "{restored}");
    assert!(!restored.contains("not restoring from cache"), "{restored}");
    assert_eq!(fs::read_to_string(project.path().join("dist/out.txt")).unwrap(), "one");
    assert_eq!(fs::read_to_string(project.path().join("runs")).unwrap(), "xx");
}

#[test]
fn per_project_lockfile_and_lockfile_dir_invalidate_task_cache() {
    let root = tempfile::tempdir().unwrap();
    let repo = pnpm_testing_utils::git_repo::GitRepoFixture::init(root.path(), "ws");
    let workspace = root.path().join("ws-src");
    let storage = root.path().join("storage");
    repo.write_file("package.json", r#"{"private":true}"#);
    repo.write_file(".gitignore", "node_modules/\n**/dist/\n**/runs\n");
    repo.write_file(
        "pnpm-workspace.yaml",
        "packages: ['packages/*']\nsharedWorkspaceLockfile: false\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: []\n    inputs: []\n    outputs: ['dist/**']\n",
    );
    let script = r#"node -e "const fs=require('fs');fs.mkdirSync('dist',{recursive:true});fs.writeFileSync('dist/out.txt','built');fs.appendFileSync('runs','x')""#;
    for name in ["alpha", "beta"] {
        repo.write_file(
            &format!("packages/{name}/package.json"),
            &serde_json::json!({ "name": name, "version": "1.0.0", "scripts": { "build": script } })
                .to_string(),
        );
    }
    let _ = repo.commit("per-project lockfiles");

    run_pipeline_cmd(&workspace, &storage, &[], &["install"]);
    run_pipeline_cmd(&workspace, &storage, &[], &["pipeline", "--full"]);
    run_pipeline_cmd(&workspace, &storage, &[], &["pipeline", "--full"]);
    assert_eq!(fs::read_to_string(workspace.join("packages/alpha/runs")).unwrap(), "x");
    assert_eq!(fs::read_to_string(workspace.join("packages/beta/runs")).unwrap(), "x");

    let alpha_lock = workspace.join("packages/alpha/pnpm-lock.yaml");
    let mut lock_text = fs::read_to_string(&alpha_lock).unwrap();
    lock_text.push_str("# invalidate alpha\n");
    fs::write(&alpha_lock, lock_text).unwrap();

    run_pipeline_cmd(&workspace, &storage, &[], &["pipeline", "--full"]);
    assert_eq!(fs::read_to_string(workspace.join("packages/alpha/runs")).unwrap(), "xx");
    assert_eq!(fs::read_to_string(workspace.join("packages/beta/runs")).unwrap(), "x");

    let custom = tempfile::tempdir().unwrap();
    pnpm_testing_utils::git_repo::init_isolated_repo(custom.path());
    fs::write(custom.path().join(".gitignore"), "node_modules/\ndist/\nruns\ncustom-locks/\n")
        .unwrap();
    fs::write(
        custom.path().join("package.json"),
        serde_json::json!({ "name": "lockdir-probe", "version": "1.0.0", "scripts": { "build": script } })
            .to_string(),
    )
    .unwrap();
    fs::write(
        custom.path().join("pnpm-workspace.yaml"),
        "packages: []\nincludeWorkspaceRoot: true\nlockfileDir: custom-locks\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: []\n    inputs: []\n    outputs: ['dist/**']\n",
    )
    .unwrap();
    run_pipeline_cmd(custom.path(), &storage, &[], &["install"]);
    run_pipeline_cmd(custom.path(), &storage, &[], &["pipeline", "--full"]);
    run_pipeline_cmd(custom.path(), &storage, &[], &["pipeline", "--full"]);
    assert_eq!(fs::read_to_string(custom.path().join("runs")).unwrap(), "x");

    let custom_lock = custom.path().join("custom-locks/pnpm-lock.yaml");
    let mut custom_lock_text = fs::read_to_string(&custom_lock).unwrap();
    custom_lock_text.push_str("# invalidate lockfileDir\n");
    fs::write(&custom_lock, custom_lock_text).unwrap();
    run_pipeline_cmd(custom.path(), &storage, &[], &["pipeline", "--full"]);
    assert_eq!(fs::read_to_string(custom.path().join("runs")).unwrap(), "xx");
}
