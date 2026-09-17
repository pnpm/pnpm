//! `concurrencyGroups` caps how many tasks of a group execute at once
//! across every pnpm process that shares a slot pool, a task outside any
//! limited group runs regardless, and a script's own nested `pnpm run`
//! rides the slot its parent holds.

use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

const HOLD: Duration = Duration::from_millis(700);

const HOLD_SCRIPT: &str = r"
    const fs = require('fs');
    const path = require('path');
    const dir = process.env.MARKER_DIR;
    const marker = path.join(dir, `running-${process.pid}`);
    const others = fs.readdirSync(dir).filter((name) => name.startsWith('running-'));
    fs.writeFileSync(marker, '');
    if (others.length > 0) fs.writeFileSync(path.join(dir, 'overlap'), others.join('\n'));
    setTimeout(() => fs.unlinkSync(marker), Number(process.env.HOLD_MS));
";

fn write_project(workspace: &Path, pacquet: &Path, limit: u32) {
    fs::write(workspace.join("hold.js"), HOLD_SCRIPT).expect("write hold.js");
    let pacquet = serde_json::to_string(&pacquet.to_string_lossy()).expect("quote pacquet path");
    let outer_script = format!(
        "require('child_process').execFileSync({pacquet}, ['run', 'hold'], {{ stdio: 'inherit' }})",
    );
    fs::write(workspace.join("outer.js"), outer_script).expect("write outer.js");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "hold": "node hold.js", "outer": "node outer.js", "free": "node -e 0" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "tasks:\n  hold:\n    concurrencyGroup: test\n  outer:\n    concurrencyGroup: test\nconcurrencyGroups:\n  test: {limit}\n",
        ),
    )
    .expect("write pnpm-workspace.yaml");
}

/// Under the test's own workspace, so the tests neither queue on one
/// another nor touch the developer's state directory.
fn state_dir(workspace: &Path) -> PathBuf {
    workspace.join("state")
}

fn pnpm_run(pacquet: &Command, script: &str) -> Command {
    let workspace = pacquet.get_current_dir().expect("workspace dir");
    let mut command = Command::new(pacquet.get_program());
    command.current_dir(workspace);
    for (name, value) in pacquet.get_envs() {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command.env("PNPM_CONFIG_STATE_DIR", state_dir(workspace));
    command.env("MARKER_DIR", workspace);
    command.env("HOLD_MS", HOLD.as_millis().to_string());
    command.args(["run", script]);
    command
}

fn hold(pacquet: &Command) -> Command {
    pnpm_run(pacquet, "hold")
}

#[test]
fn concurrent_tasks_past_the_limit_wait_for_a_slot() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, Path::new(pacquet.get_program()), 1);

    let started = Instant::now();
    let mut first = hold(&pacquet).spawn().expect("spawn the first run");
    let mut second = hold(&pacquet).spawn().expect("spawn the second run");
    let first = first.wait().expect("wait for the first run");
    let second = second.wait().expect("wait for the second run");
    let elapsed = started.elapsed();

    dbg!(first, second, elapsed);
    assert!(first.success() && second.success());
    assert!(!workspace.join("overlap").exists(), "both runs held the single slot at once");
    assert!(elapsed >= HOLD * 2, "the second run did not wait for the first to finish");
    assert!(
        state_dir(&workspace)
            .join("run-slots")
            .join("test")
            .join("0")
            .is_file(),
    );

    drop(root);
}

#[test]
fn tasks_within_the_limit_do_not_wait() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, Path::new(pacquet.get_program()), 2);

    let started = Instant::now();
    let mut first = hold(&pacquet).spawn().expect("spawn the first run");
    let mut second = hold(&pacquet).spawn().expect("spawn the second run");
    let first = first.wait().expect("wait for the first run");
    let second = second.wait().expect("wait for the second run");
    let elapsed = started.elapsed();

    dbg!(first, second, elapsed);
    assert!(first.success() && second.success());
    assert!(elapsed < HOLD * 2, "a run waited although a second slot was free");

    drop(root);
}

#[test]
fn a_task_outside_the_group_runs_while_the_slots_are_held() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, Path::new(pacquet.get_program()), 1);

    let mut holder = hold(&pacquet).spawn().expect("spawn the holding run");
    thread::sleep(HOLD / 2);
    let started = Instant::now();
    let free = pnpm_run(&pacquet, "free").status().expect("run the free script");
    let elapsed = started.elapsed();
    holder.wait().expect("wait for the holding run");

    dbg!(free, elapsed);
    assert!(free.success());
    assert!(elapsed < HOLD / 2, "the ungrouped script waited for the slot");

    drop(root);
}

#[test]
fn a_waiting_task_reports_who_holds_the_slots() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, Path::new(pacquet.get_program()), 1);

    let mut holder = hold(&pacquet).spawn().expect("spawn the holding run");
    thread::sleep(HOLD / 2);
    let output = hold(&pacquet).output().expect("run the waiting run");
    holder.wait().expect("wait for the holding run");

    // The default reporter renders warnings on stdout.
    let stdout = String::from_utf8_lossy(&output.stdout);
    dbg!(&stdout);
    assert!(output.status.success());
    assert!(stdout.contains(r#"Waiting to run "hold": all 1 slots of concurrency group "test""#));
    assert!(stdout.contains(&format!("pid {} in ", holder.id())));

    drop(root);
}

/// With a single slot, a script that itself calls `pnpm run` for a task of
/// the same group would wait on its own parent forever if the nested run
/// took a slot of its own.
#[test]
fn a_nested_task_uses_the_slot_its_parent_holds() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, Path::new(pacquet.get_program()), 1);

    pnpm_run(&pacquet, "outer")
        .env("HOLD_MS", "10")
        .assert()
        .success();

    drop(root);
}

/// `pnpm pipeline` runs the tasks of one invocation in-process, and they
/// count against the same pools as separate invocations do.
#[test]
fn pipeline_tasks_of_one_group_share_its_slots() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");
    for name in ["a", "b"] {
        let project = workspace.join("packages").join(name);
        fs::create_dir_all(&project).expect("create project dir");
        fs::write(project.join("hold.js"), HOLD_SCRIPT).expect("write hold.js");
        fs::write(
            project.join("package.json"),
            json!({ "name": name, "version": "0.0.0", "scripts": { "hold": "node hold.js" } })
                .to_string(),
        )
        .expect("write package.json");
    }
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages: ['packages/*']\npipelines:\n  default: [hold]\ntasks:\n  hold:\n    dependsOn: []\n    concurrencyGroup: test\nconcurrencyGroups:\n  test: 1\n",
    )
    .expect("write pnpm-workspace.yaml");

    let pnpm = |args: &[&str]| {
        let mut command = Command::new(pacquet.get_program());
        command.current_dir(&workspace);
        for (name, value) in pacquet.get_envs() {
            match value {
                Some(value) => command.env(name, value),
                None => command.env_remove(name),
            };
        }
        command
            .env("PNPM_CONFIG_STATE_DIR", state_dir(&workspace))
            .env("MARKER_DIR", &workspace)
            .env("HOLD_MS", HOLD.as_millis().to_string())
            .args(args);
        command
    };
    // The pipeline installs with a frozen lockfile, so write one first.
    pnpm(&["install"]).assert().success();

    let started = Instant::now();
    let output = pnpm(&["pipeline", "--full", "--no-cache"]).output().expect("run the pipeline");
    let elapsed = started.elapsed();

    dbg!(String::from_utf8_lossy(&output.stderr), elapsed);
    assert!(output.status.success());
    assert!(!workspace.join("overlap").exists(), "both tasks held the single slot at once");
    assert!(elapsed >= HOLD * 2, "the second task did not wait for the first to finish");

    drop(root);
}
