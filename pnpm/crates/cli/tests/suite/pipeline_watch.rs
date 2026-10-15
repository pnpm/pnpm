//! Watch-agent integration tests: poll a git repository, build new
//! revisions of a branch in a persistent checkout, and skip ticks with
//! nothing new.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{command_env::CommandTestExt, git_repo::GitRepoFixture};
use std::{
    fs,
    io::{self, BufRead as _, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

fn agent_tick(root: &Path, repo: &str) -> assert_cmd::assert::Assert {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root)
        .without_ambient_pnpm_config()
        .with_env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .with_env("XDG_STATE_HOME", root.join("xdg-state"))
        .with_env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .with_args(["pipeline", "--watch", "--once", "--repo", repo, "--branch", "main"])
        .assert()
}

/// The persistent checkout the agent created: the single repo directory
/// under the agent state dir.
fn agent_checkout(root: &Path) -> PathBuf {
    let agents = root
        .join("xdg-state")
        .join("pnpm")
        .join("pipeline")
        .join("agent");
    let state_dir = fs::read_dir(&agents)
        .expect("agent state dir exists")
        .next()
        .expect("one watched (repo, branch)")
        .expect("read agent state dir")
        .path();
    state_dir.join("checkout").join("demo")
}

/// A `demo` repository whose `main` has one commit with a passing build,
/// and the hash of that commit.
fn init_demo_repo(root: &Path) -> (GitRepoFixture, String) {
    let fixture = GitRepoFixture::init(root, "demo");
    fixture.write_file(".gitignore", "node_modules\nout\n");
    fixture.write_file(
        "pnpm-workspace.yaml",
        "packages:\n  - pkg\npipelines:\n  default:\n    - build\ntasks:\n  build:\n    dependsOn: []\n    outputs: ['out/**']\n",
    );
    fixture.write_file(
        "pkg/package.json",
        r#"{ "name": "pkg", "version": "1.0.0", "scripts": { "build": "node -e \"const fs = require('fs'); fs.mkdirSync('out', { recursive: true }); fs.copyFileSync('src/index.txt', 'out/index.txt')\"" } }"#,
    );
    fixture.write_file("pkg/src/index.txt", "v1");
    // The lockfile the checkout's frozen install verifies against.
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root.join("demo-src"))
        .without_ambient_pnpm_config()
        .with_env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .with_env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .with_args(["install"])
        .assert()
        .success();
    let first = fixture.commit("one");
    (fixture, first)
}

#[test]
fn watch_agent_builds_new_revisions_and_skips_quiet_ticks() {
    let root = tempfile::Builder::new()
        .prefix("pacquet-test-")
        .tempdir()
        .expect("create temporary directory");
    let (fixture, first) = init_demo_repo(root.path());
    let repo = root.path().join("demo.git");
    let repo = repo.to_string_lossy();

    let tick = agent_tick(root.path(), &repo);
    let stdout = String::from_utf8_lossy(&tick.get_output().stdout).into_owned();
    tick.success();
    assert!(stdout.contains(&format!("New revision {first}")), "unexpected output: {stdout}");
    assert!(stdout.contains("passed"), "unexpected output: {stdout}");
    let built = agent_checkout(root.path())
        .join("pkg")
        .join("out")
        .join("index.txt");
    assert_eq!(fs::read_to_string(&built).expect("first build produced the output"), "v1");

    // Nothing new: no build, no output change.
    let tick = agent_tick(root.path(), &repo);
    let stdout = String::from_utf8_lossy(&tick.get_output().stdout).into_owned();
    tick.success();
    assert!(stdout.contains("main is up to date"), "unexpected output: {stdout}");

    // A pushed change is picked up and built.
    fs::write(agent_checkout(root.path()).join("pkg/src/index.txt"), "dirty source").unwrap();
    fs::write(agent_checkout(root.path()).join("pkg/new.txt"), "untracked collision").unwrap();
    fixture.write_file("pkg/new.txt", "tracked in the next revision");
    fixture.write_file("pkg/src/index.txt", "v2");
    let second = fixture.commit("two");
    let tick = agent_tick(root.path(), &repo);
    let stdout = String::from_utf8_lossy(&tick.get_output().stdout).into_owned();
    tick.success();
    assert!(stdout.contains(&format!("New revision {second}")), "unexpected output: {stdout}");
    assert_eq!(fs::read_to_string(&built).expect("second build refreshed the output"), "v2");

    fixture.write_file(
        "pkg/package.json",
        r#"{"name":"pkg","version":"1.0.0","scripts":{"build":"node -e \"process.exit(1)\""}}"#,
    );
    let failed = fixture.commit("failing build");
    for _ in 0..2 {
        let tick = agent_tick(root.path(), &repo);
        let stdout = String::from_utf8_lossy(&tick.get_output().stdout).into_owned();
        tick.failure();
        assert!(
            stdout.contains(&format!("New revision {failed}")),
            "failed revision must remain pending: {stdout}",
        );
    }
}

#[test]
fn watch_agent_refuses_a_zero_poll_interval() {
    let result = Command::cargo_bin("pnpm")
        .unwrap()
        .without_ambient_pnpm_config()
        .args(["pipeline", "--watch", "--repo", "unused", "--interval", "0"])
        .assert()
        .failure();
    let error = String::from_utf8_lossy(&result.get_output().stderr);
    assert!(error.contains("0 is not in 1.."), "{error}");
}

/// Spawn a watch agent polling every second that writes to `stdout`.
fn spawn_agent(root: &Path, repo: &str, stdout: Stdio) -> Child {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root)
        .without_ambient_pnpm_config()
        .with_env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .with_env("XDG_STATE_HOME", root.join("xdg-state"))
        .with_env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .with_args(["pipeline", "--watch", "--repo", repo, "--branch", "main", "--interval", "1"])
        .with_stdout(stdout)
        .spawn()
        .expect("spawn the watch agent")
}

fn wait_for_agent_to_stop(mut agent: Child) -> ExitStatus {
    let deadline = Instant::now() + Duration::from_mins(1);
    loop {
        if let Some(status) = agent.try_wait().expect("poll the watch agent") {
            return status;
        }
        if Instant::now() > deadline {
            agent.kill().expect("kill the watch agent");
            panic!("the watch agent kept running after its stdout reader closed");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// `pnpm pipeline --watch | head -0` must not leave the agent polling
/// for a reader that is gone (pnpm/tasks#132).
#[test]
fn watch_agent_stops_when_stdout_is_closed_at_startup() {
    let root = tempfile::Builder::new()
        .prefix("pacquet-test-")
        .tempdir()
        .expect("create temporary directory");
    let (stdout_reader, stdout_writer) = io::pipe().expect("create stdout pipe");
    drop(stdout_reader);

    let agent = spawn_agent(root.path(), "unused", Stdio::from(stdout_writer));
    assert_eq!(wait_for_agent_to_stop(agent).code(), Some(0));
}

/// `pnpm pipeline --watch | head -1` reads the startup line and leaves;
/// the agent stops at the next tick that writes.
#[test]
fn watch_agent_stops_when_stdout_closes_between_ticks() {
    let root = tempfile::Builder::new()
        .prefix("pacquet-test-")
        .tempdir()
        .expect("create temporary directory");
    init_demo_repo(root.path());
    let repo = root.path().join("demo.git");
    let repo = repo.to_string_lossy();
    // Build the revision first, so every tick of the watch is a quick
    // "up to date".
    agent_tick(root.path(), &repo).success();
    let (stdout_reader, stdout_writer) = io::pipe().expect("create stdout pipe");

    let agent = spawn_agent(root.path(), &repo, Stdio::from(stdout_writer));
    let mut startup_line = String::new();
    BufReader::new(stdout_reader).read_line(&mut startup_line).expect("read the startup line");
    assert!(startup_line.starts_with("Watching "), "{startup_line}");
    assert_eq!(wait_for_agent_to_stop(agent).code(), Some(0));
}
