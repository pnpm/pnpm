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

/// `--parallel` implies `--recursive`, so this bare invocation — the one
/// the issue reported — is a recursive run.
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

/// Both spellings of a concurrency of one reach the same execution mode.
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

    let output = assert_cmd::Command::from_std(pacquet)
        .args(["--workspace-concurrency=2", "-r", "run", "/^dev:/"])
        .timeout(Duration::from_mins(1))
        .assert()
        .failure()
        .get_output()
        .clone();

    // The cancelled sibling must not take the failing script's place as
    // the task's verdict, which is what names the project in the error.
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(stderr.contains("ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL"), "stderr: {stderr}");

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
                    "check:pass": r#"node -e "require('fs').writeFileSync('passed.txt', '')""#,
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

const PEAK_DIR: &str = "peak-probe";

/// A script that samples how many of the run's scripts are live while it
/// runs: it drops an `active-<id>` marker in a directory every package
/// shares, counts the markers for 300ms, and leaves its own peak behind
/// as `peak-<id>`.
fn write_peak_probe(workspace: &std::path::Path) {
    fs::create_dir(workspace.join(PEAK_DIR)).expect("create the peak-probe directory");
    fs::write(
        workspace.join("track-peak.js"),
        r"const fs = require('fs')
const path = require('path')
const dir = path.join('..', 'peak-probe')
const id = process.argv[2]
const marker = path.join(dir, `active-${id}`)
fs.writeFileSync(marker, '')
let peak = 0
const sample = () => {
  const live = fs.readdirSync(dir).filter((entry) => entry.startsWith('active-')).length
  peak = Math.max(peak, live)
}
const timer = setInterval(sample, 5)
setTimeout(() => {
  clearInterval(timer)
  sample()
  fs.rmSync(marker, { force: true })
  fs.writeFileSync(path.join(dir, `peak-${id}`), String(peak))
}, 300)
",
    )
    .expect("write peak probe");
}

fn peak_scripts(name: &str) -> serde_json::Value {
    json!({
        "name": name,
        "version": "1.0.0",
        "scripts": {
            "dev:one": format!("node ../track-peak.js {name}-one"),
            "dev:two": format!("node ../track-peak.js {name}-two"),
        },
    })
}

/// `workspaceConcurrency` caps the scripts pnpm has running, not the
/// tasks it has dispatched.
#[test]
fn recursive_run_keeps_the_matched_scripts_within_workspace_concurrency() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_peak_probe(&workspace);
    let manifests: Vec<(&str, serde_json::Value)> =
        ["project-1", "project-2", "project-3", "project-4"]
            .into_iter()
            .map(|name| (name, peak_scripts(name)))
            .collect();
    write_workspace(&workspace, &manifests);

    assert_cmd::Command::from_std(pacquet)
        .args(["--workspace-concurrency=2", "-r", "run", "/^dev:/"])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();

    let peaks: Vec<usize> = fs::read_dir(workspace.join(PEAK_DIR))
        .expect("read the peak-probe directory")
        .map(|entry| entry.expect("read a peak-probe entry").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("peak-"))
        })
        .map(|path| {
            fs::read_to_string(&path)
                .expect("read a peak file")
                .trim()
                .parse()
                .expect("a peak file holds a count")
        })
        .collect();
    eprintln!("per-script peaks: {peaks:?}");
    assert_eq!(peaks.len(), 8);
    assert_eq!(peaks.iter().max().copied(), Some(2));

    drop(root);
}
