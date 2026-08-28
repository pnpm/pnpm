//! Watch-agent integration tests: poll a git repository, build new
//! revisions of a branch in a persistent checkout, and skip ticks with
//! nothing new. The build scripts run through pacquet's `sh -c`
//! executor, so the file is gated to Unix like the other run suites.
#![cfg(unix)]

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{command_env::CommandTestExt, git_repo::GitRepoFixture};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn agent_tick(root: &Path, repo: &str) -> assert_cmd::assert::Assert {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root)
        .without_ambient_pnpm_config()
        .with_env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .with_env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .with_args(["pipeline", "--watch", "--once", "--repo", repo, "--branch", "main"])
        .assert()
}

/// The persistent checkout the agent created: the single repo directory
/// under the agent state dir.
fn agent_checkout(root: &Path) -> PathBuf {
    let agents = root.join("xdg-cache").join("pnpm").join("pipeline").join("agent");
    let state_dir = fs::read_dir(&agents)
        .expect("agent state dir exists")
        .next()
        .expect("one watched (repo, branch)")
        .expect("read agent state dir")
        .path();
    state_dir.join("demo")
}

#[test]
fn watch_agent_builds_new_revisions_and_skips_quiet_ticks() {
    let root = tempfile::Builder::new()
        .prefix("pacquet-test-")
        .tempdir()
        .expect("create temporary directory");
    let fixture = GitRepoFixture::init(root.path(), "demo");
    fixture.write_file(".gitignore", "node_modules\nout\n");
    fixture.write_file(
        "pnpm-workspace.yaml",
        "packages:\n  - pkg\npipelines:\n  default:\n    - build\ntasks:\n  build:\n    dependsOn: []\n    outputs: ['out/**']\n",
    );
    fixture.write_file(
        "pkg/package.json",
        r#"{ "name": "pkg", "version": "1.0.0", "scripts": { "build": "mkdir -p out && cp src/index.txt out/index.txt" } }"#,
    );
    fixture.write_file("pkg/src/index.txt", "v1");
    // The lockfile the checkout's frozen install verifies against.
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root.path().join("demo-src"))
        .without_ambient_pnpm_config()
        .with_env("XDG_CACHE_HOME", root.path().join("xdg-cache"))
        .with_env("XDG_CONFIG_HOME", root.path().join("xdg-config"))
        .with_args(["install"])
        .assert()
        .success();
    let first = fixture.commit("one");
    let repo = root.path().join("demo.git");
    let repo = repo.to_string_lossy();

    let tick = agent_tick(root.path(), &repo);
    let stdout = String::from_utf8_lossy(&tick.get_output().stdout).into_owned();
    tick.success();
    assert!(stdout.contains(&format!("New revision {first}")), "unexpected output: {stdout}");
    assert!(stdout.contains("passed"), "unexpected output: {stdout}");
    let built = agent_checkout(root.path()).join("pkg").join("out").join("index.txt");
    assert_eq!(fs::read_to_string(&built).expect("first build produced the output"), "v1");

    // Nothing new: no build, no output change.
    let tick = agent_tick(root.path(), &repo);
    let stdout = String::from_utf8_lossy(&tick.get_output().stdout).into_owned();
    tick.success();
    assert!(stdout.contains("main is up to date"), "unexpected output: {stdout}");

    // A pushed change is picked up and built.
    fixture.write_file("pkg/src/index.txt", "v2");
    let second = fixture.commit("two");
    let tick = agent_tick(root.path(), &repo);
    let stdout = String::from_utf8_lossy(&tick.get_output().stdout).into_owned();
    tick.success();
    assert!(stdout.contains(&format!("New revision {second}")), "unexpected output: {stdout}");
    assert_eq!(fs::read_to_string(&built).expect("second build refreshed the output"), "v2");
}
