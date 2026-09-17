//! `machineRunConcurrency` caps how many `pnpm run` / `pnpm exec`
//! invocations execute at once across every pnpm process that shares a
//! slot pool, and a script's own nested `pnpm run` rides the slot its
//! parent holds.

use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{
    fs,
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

const HOLD: Duration = Duration::from_millis(700);

/// A project whose `hold` script announces itself with a marker file,
/// records an `overlap` file if another `hold` is running, and keeps its
/// slot for [`HOLD`]. `outer` runs `hold` through a nested `pnpm run`.
/// `limit` is the `machineRunConcurrency` under test.
fn write_project(workspace: &Path, pacquet: &Path, limit: u32) {
    let hold_script = r"
        const fs = require('fs');
        const marker = `running-${process.pid}`;
        const others = fs.readdirSync('.').filter((name) => name.startsWith('running-'));
        fs.writeFileSync(marker, '');
        if (others.length > 0) fs.writeFileSync('overlap', others.join('\n'));
        setTimeout(() => fs.unlinkSync(marker), Number(process.env.HOLD_MS));
    ";
    fs::write(workspace.join("hold.js"), hold_script).expect("write hold.js");
    let pacquet = serde_json::to_string(&pacquet.to_string_lossy()).expect("quote pacquet path");
    let outer_script = format!(
        "require('child_process').execFileSync({pacquet}, ['run', 'hold'], {{ stdio: 'inherit' }})"
    );
    fs::write(workspace.join("outer.js"), outer_script).expect("write outer.js");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "hold": "node hold.js", "outer": "node outer.js" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("machineRunConcurrency: {limit}\nmachineRunConcurrencyGroup: test\n"),
    )
    .expect("write pnpm-workspace.yaml");
}

/// The slot pool of a test lives under its own workspace, so the tests
/// neither queue on one another nor touch the developer's state directory.
fn state_dir(workspace: &Path) -> std::path::PathBuf {
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
    command.env("HOLD_MS", HOLD.as_millis().to_string());
    command.args(["run", script]);
    command
}

fn hold(pacquet: &Command) -> Command {
    pnpm_run(pacquet, "hold")
}

#[test]
fn concurrent_runs_past_the_limit_wait_for_a_slot() {
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
        workspace
            .join("state")
            .join("run-slots")
            .join("test")
            .join("0")
            .is_file()
    );

    drop(root);
}

#[test]
fn runs_within_the_limit_do_not_wait() {
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
fn a_waiting_run_reports_who_holds_the_slots() {
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
    assert!(stdout.contains("Waiting for a free run slot"));
    assert!(stdout.contains(&format!("pid {} in ", holder.id())));

    drop(root);
}

/// With a single slot, a script that itself calls `pnpm run` would wait on
/// its own parent forever if the nested run took a slot of its own.
#[test]
fn a_nested_run_uses_the_slot_its_parent_holds() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, Path::new(pacquet.get_program()), 1);

    pnpm_run(&pacquet, "outer")
        .env("HOLD_MS", "10")
        .assert()
        .success();

    drop(root);
}
