//! `concurrencyGroups` caps how many tasks of a group execute at once
//! across every pnpm process that shares a slot pool, a task outside any
//! limited group runs regardless, and a script's own nested `pnpm run`
//! rides the slot its parent holds.

use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const HOLD: Duration = Duration::from_secs(1);
const RELEASE_TIMEOUT: Duration = Duration::from_secs(30);

const HOLD_SCRIPT: &str = r"
    const fs = require('fs');
    const path = require('path');
    const dir = process.env.MARKER_DIR;
    const marker = path.join(dir, `running-${process.pid}`);
    fs.writeFileSync(marker, '');
    const others = fs.readdirSync(dir).filter((name) =>
      name.startsWith('running-') && path.join(dir, name) !== marker
    );
    if (others.length > 0) fs.writeFileSync(path.join(dir, 'overlap'), others.join('\n'));
    if (process.env.ORDER_LOG) {
      fs.appendFileSync(process.env.ORDER_LOG, `${process.env.RUN_ID}\n`);
    }
    const sleeper = new Int32Array(new SharedArrayBuffer(4));
    const deadline = Date.now() + Number(process.env.HOLD_MS);
    while (fs.existsSync(marker) && Date.now() < deadline) {
      Atomics.wait(sleeper, 0, 0, 10);
    }
    if (fs.existsSync(marker)) fs.unlinkSync(marker);
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

fn releasable_holder(pacquet: &Command) -> Child {
    hold(pacquet)
        .env("HOLD_MS", RELEASE_TIMEOUT.as_millis().to_string())
        .spawn()
        .expect("spawn the holding run")
}

/// The holder owns its slot once its script has written the marker.
fn wait_for_holders(workspace: &Path, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let running = fs::read_dir(workspace)
            .expect("list the workspace")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("running-")
            })
            .count();
        if running >= count {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("{count} holders never started their scripts");
}

fn release_holders(workspace: &Path) {
    for entry in fs::read_dir(workspace)
        .expect("list the workspace")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("running-")
        })
    {
        fs::remove_file(entry.path()).expect("release a holding run");
    }
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

    let mut first = hold(&pacquet)
        .env("HOLD_MS", RELEASE_TIMEOUT.as_millis().to_string())
        .spawn()
        .expect("spawn the first run");
    let mut second = hold(&pacquet)
        .env("HOLD_MS", RELEASE_TIMEOUT.as_millis().to_string())
        .spawn()
        .expect("spawn the second run");
    wait_for_holders(&workspace, 2);
    release_holders(&workspace);
    let first = first.wait().expect("wait for the first run");
    let second = second.wait().expect("wait for the second run");

    dbg!(first, second);
    assert!(first.success() && second.success());
    assert!(workspace.join("overlap").exists(), "both available slots were not used together");

    drop(root);
}

#[test]
fn a_task_outside_the_group_runs_while_the_slots_are_held() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, Path::new(pacquet.get_program()), 1);

    let mut holder = releasable_holder(&pacquet);
    wait_for_holders(&workspace, 1);
    let free = pnpm_run(&pacquet, "free").status().expect("run the free script");
    let holder_still_running = holder
        .try_wait()
        .expect("poll the holding run")
        .is_none();
    release_holders(&workspace);
    holder.wait().expect("wait for the holding run");

    dbg!(free, holder_still_running);
    assert!(free.success());
    assert!(holder_still_running, "the ungrouped script waited for the slot");

    drop(root);
}

#[test]
fn a_waiting_task_reports_who_holds_the_slots() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, Path::new(pacquet.get_program()), 1);

    let mut holder = releasable_holder(&pacquet);
    wait_for_holders(&workspace, 1);
    let mut waiting = hold(&pacquet)
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn the waiting run");
    let mut stdout = BufReader::new(waiting.stdout.take().expect("capture the waiting run stdout"));
    let mut rendered = String::new();
    loop {
        let mut line = String::new();
        if stdout.read_line(&mut line).expect("read the waiting warning") == 0 {
            break;
        }
        rendered.push_str(&line);
        if rendered.contains(r#"Waiting to run "hold": all 1 slots of concurrency group "test""#) {
            break;
        }
    }
    release_holders(&workspace);
    stdout.read_to_string(&mut rendered).expect("read the waiting run stdout");
    let status = waiting.wait().expect("wait for the waiting run");
    holder.wait().expect("wait for the holding run");

    // The default reporter renders warnings on stdout.
    dbg!(&rendered);
    assert!(status.success());
    assert!(rendered.contains(r#"Waiting to run "hold": all 1 slots of concurrency group "test""#));
    assert!(rendered.contains("You are #1 of 1 in line"));
    assert!(rendered.contains(&format!("pid {} in ", holder.id())));

    drop(root);
}

fn read_until_queued(mut stdout: BufReader<impl Read>) -> Result<(), String> {
    let mut rendered = String::new();
    loop {
        let mut line = String::new();
        match stdout.read_line(&mut line) {
            Ok(0) => return Err(rendered),
            Ok(_) => {
                rendered.push_str(&line);
                if rendered.contains("Waiting to run") {
                    return Ok(());
                }
            }
            Err(error) => return Err(format!("{error}: {rendered}")),
        }
    }
}

fn wait_until_queued(child: &mut Child) {
    let stdout = BufReader::new(child.stdout.take().expect("capture stdout"));
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let _ = tx.send(read_until_queued(stdout));
    });
    match rx.recv_timeout(Duration::from_secs(30)) {
        Ok(Ok(())) => {
            let _ = reader.join();
        }
        Ok(Err(rendered)) => {
            fail_queued_run(child, reader, &format!("the run never queued: {rendered}"));
        }
        Err(error) => {
            fail_queued_run(
                child,
                reader,
                &format!("timed out waiting for the queue notice ({error})"),
            );
        }
    }
}

fn fail_queued_run(child: &mut Child, reader: thread::JoinHandle<()>, message: &str) -> ! {
    let _ = child.kill();
    let status = child.wait().expect("wait for the waiting run");
    let _ = reader.join();
    panic!("{message} status={status:?}");
}

fn queued_run(pacquet: &Command, script: &str, run_id: &str, order_log: &Path) -> Child {
    pnpm_run(pacquet, script)
        .env("RUN_ID", run_id)
        .env("ORDER_LOG", order_log)
        .env("HOLD_MS", "10")
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn a queued run")
}

#[test]
fn waiters_run_in_the_order_they_began_waiting() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, Path::new(pacquet.get_program()), 1);
    let order_log = workspace.join("order");

    let mut holder = releasable_holder(&pacquet);
    wait_for_holders(&workspace, 1);
    let mut first = queued_run(&pacquet, "hold", "first", &order_log);
    wait_until_queued(&mut first);
    let mut second = queued_run(&pacquet, "hold", "second", &order_log);
    wait_until_queued(&mut second);
    release_holders(&workspace);
    assert!(
        first
            .wait()
            .expect("first waiter")
            .success(),
    );
    assert!(
        second
            .wait()
            .expect("second waiter")
            .success(),
    );
    holder.wait().expect("holder");

    let order = fs::read_to_string(&order_log).expect("read the run order");
    dbg!(&order);
    assert_eq!(order, "first\nsecond\n");

    drop(root);
}

#[test]
fn a_higher_priority_waiter_runs_before_earlier_arrivals() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, Path::new(pacquet.get_program()), 1);
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": { "hold": "node hold.js", "urgent": "node hold.js" },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "tasks:\n  hold:\n    concurrencyGroup: test\n  urgent:\n    concurrencyGroup: test\n    priority: 10\nconcurrencyGroups:\n  test: 1\n",
    )
    .expect("write pnpm-workspace.yaml");
    let order_log = workspace.join("order");

    let mut holder = releasable_holder(&pacquet);
    wait_for_holders(&workspace, 1);
    let mut low = queued_run(&pacquet, "hold", "low", &order_log);
    wait_until_queued(&mut low);
    let mut high = queued_run(&pacquet, "urgent", "high", &order_log);
    wait_until_queued(&mut high);
    release_holders(&workspace);
    assert!(
        low.wait()
            .expect("low-priority waiter")
            .success(),
    );
    assert!(
        high.wait()
            .expect("high-priority waiter")
            .success(),
    );
    holder.wait().expect("holder");

    let order = fs::read_to_string(&order_log).expect("read the run order");
    dbg!(&order);
    assert_eq!(order, "high\nlow\n");

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
