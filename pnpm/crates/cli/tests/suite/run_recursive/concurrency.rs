//! The scripts a `/pattern/` selector matches in one package run
//! concurrently when the run's concurrency allows it, as they do in
//! pnpm 11 — <https://github.com/pnpm/pnpm/issues/14933>.

use super::{CommandExtra, CommandTempCwd, Duration, fs, json, write_workspace};
use assert_cmd::assert::OutputAssertExt;

/// A pair of scripts that record whether they ever ran simultaneously:
/// each drops an `active-<self>` marker in its package directory, waits
/// up to a second for the other's, and writes `saw-parallel` if it
/// appears. Prints `finished-<self>` so a test can also tell both ran.
fn write_overlap_probe(workspace: &std::path::Path) {
    fs::write(
        workspace.join("track-concurrency.js"),
        r"const fs = require('fs')
const [self, other] = process.argv.slice(2)
const marker = `active-${self}`
fs.writeFileSync(marker, '')
const started = Date.now()
const check = setInterval(() => {
  if (fs.existsSync(`active-${other}`)) {
    fs.writeFileSync('saw-parallel', '')
    finish()
  } else if (Date.now() - started > 1000) {
    finish()
  }
}, 10)
function finish () {
  clearInterval(check)
  fs.rmSync(marker, { force: true })
  console.log(`finished-${self}`)
}
",
    )
    .expect("write concurrency probe");
}

fn overlapping_scripts() -> serde_json::Value {
    json!({
        "name": "project-1",
        "version": "1.0.0",
        "scripts": {
            "dev:one": "node ../track-concurrency.js one two",
            "dev:two": "node ../track-concurrency.js two one",
        },
    })
}

/// `pnpm --parallel` starts every script the selector matched at once,
/// including the ones a single package matched — the reported
/// regression's exact invocation.
#[test]
fn parallel_run_runs_regexp_scripts_of_one_package_concurrently() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_overlap_probe(&workspace);
    write_workspace(&workspace, &[("project-1", overlapping_scripts())]);

    let output = pacquet
        .with_args(["--parallel", "/^dev:/"])
        .assert()
        .success()
        .get_output()
        .clone();

    let saw_parallel = workspace
        .join("project-1")
        .join("saw-parallel")
        .exists();
    eprintln!("saw-parallel exists: {saw_parallel}");
    assert!(saw_parallel, "the scripts of one package should overlap under --parallel");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("STDOUT:\n{stdout}\n");
    assert!(stdout.contains("dev:one: finished-one"));
    assert!(stdout.contains("dev:two: finished-two"));

    drop(root);
}

/// The default recursive run schedules a task's scripts up to the
/// workspace concurrency, the same limit the tasks themselves get.
#[test]
fn recursive_run_runs_regexp_scripts_concurrently_under_workspace_concurrency() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_overlap_probe(&workspace);
    write_workspace(&workspace, &[("project-1", overlapping_scripts())]);

    pacquet
        .with_args(["--workspace-concurrency=2", "-r", "run", "/^dev:/"])
        .assert()
        .success();

    let saw_parallel = workspace
        .join("project-1")
        .join("saw-parallel")
        .exists();
    eprintln!("saw-parallel exists: {saw_parallel}");
    assert!(
        saw_parallel,
        "the scripts of one package should overlap under workspace-concurrency=2",
    );

    drop(root);
}

/// A script-level concurrency of one — `--sequential` or
/// `--workspace-concurrency=1` — keeps the matched scripts apart.
#[test]
fn recursive_run_runs_regexp_scripts_sequentially_below_concurrency_two() {
    for flags in [&["--sequential"][..], &["--workspace-concurrency=1"][..]] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        write_overlap_probe(&workspace);
        write_workspace(&workspace, &[("project-1", overlapping_scripts())]);

        let output = pacquet
            .with_args(flags)
            .with_args(["-r", "run", "/^dev:/"])
            .assert()
            .success()
            .get_output()
            .clone();

        let saw_parallel = workspace
            .join("project-1")
            .join("saw-parallel")
            .exists();
        eprintln!("saw-parallel exists: {saw_parallel} (flags: {flags:?})");
        assert!(!saw_parallel, "the scripts must not overlap with {flags:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        eprintln!("STDOUT:\n{stdout}\n");
        assert!(stdout.contains("finished-one"), "flags: {flags:?}");
        assert!(stdout.contains("finished-two"), "flags: {flags:?}");

        drop(root);
    }
}

/// Under `--bail` the first failing script cancels the sibling still
/// running beside it in the same package, like it does across packages.
#[test]
fn bail_cancels_the_sibling_script_of_the_same_package() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "project-1",
            json!({
                "name": "project-1",
                "version": "1.0.0",
                "scripts": {
                    "dev:slow": r#"node -e "const fs = require('fs'); fs.writeFileSync('slow-started', ''); setTimeout(() => fs.writeFileSync('slow-finished', ''), 30000)""#,
                    "dev:fail": r#"node -e "const fs = require('fs'); const wait = () => fs.existsSync('slow-started') ? process.exit(17) : setTimeout(wait, 10); wait()""#,
                },
            }),
        )],
    );

    assert_cmd::Command::from_std(pacquet)
        .args(["--workspace-concurrency=2", "-r", "run", "/^dev:/"])
        .timeout(Duration::from_mins(1))
        .assert()
        .failure();

    let slow_started = workspace
        .join("project-1")
        .join("slow-started")
        .exists();
    let slow_finished = workspace
        .join("project-1")
        .join("slow-finished")
        .exists();
    eprintln!("slow-started exists: {slow_started}, slow-finished exists: {slow_finished}");
    assert!(slow_started, "the slow script must have started beside the failing one");
    assert!(!slow_finished, "the slow script must be cancelled before its watchdog completes");

    drop(root);
}

/// Under `--no-bail` a failing script does not stop its sibling in the
/// same package, and the run reports the failure once all settled.
#[test]
fn no_bail_runs_every_regexp_script_of_the_same_package() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "project-1",
            json!({
                "name": "project-1",
                "version": "1.0.0",
                "scripts": {
                    "check:pass": "touch passed.txt",
                    "check:fail": r#"node -e "process.exit(3)""#,
                },
            }),
        )],
    );

    let output = assert_cmd::Command::from_std(pacquet)
        .args(["--no-bail", "--workspace-concurrency=2", "-r", "run", "/^check:/"])
        .timeout(Duration::from_mins(1))
        .assert()
        .failure()
        .get_output()
        .clone();

    let passed = workspace
        .join("project-1")
        .join("passed.txt")
        .exists();
    eprintln!("passed.txt exists: {passed}");
    assert!(passed, "the passing script must run beside the failing one");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("failed in 1 packages"), "stderr: {stderr}");

    drop(root);
}
