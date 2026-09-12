use super::{
    CommandExtra, CommandTempCwd, Duration, assert_no_bail_lets_siblings_finish, fs, json,
};
use assert_cmd::assert::OutputAssertExt;

/// Without `--if-present`, calling a script that does not exist fails
/// with pnpm's `NO_SCRIPT` error.
#[test]
fn run_errors_on_missing_script_without_if_present() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest_path = workspace.join("package.json");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "echo built" },
    })
    .to_string();
    fs::write(&manifest_path, manifest).expect("write package.json");

    let output =
        pacquet.with_arg("run").with_arg("nonexistent").output().expect("spawn pacquet run");
    assert!(!output.status.success(), "missing script must surface as a failure");

    drop(root);
}

/// With `--if-present`, the same missing script becomes a no-op
/// and pacquet exits cleanly. Required for orchestration tools
/// that probe optional scripts without wanting to fail the
/// pipeline.
#[test]
fn run_with_if_present_is_a_noop_for_missing_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest_path = workspace.join("package.json");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "echo built" },
    })
    .to_string();
    fs::write(&manifest_path, manifest).expect("write package.json");

    pacquet.with_arg("run").with_arg("--if-present").with_arg("nonexistent").assert().success();

    drop(root);
}

/// pnpm also accepts `--if-present` ahead of the script name
/// (`pnpm --if-present <script>`), where the script dispatches through
/// the shorthand fallback instead of an explicit `run`. The missing
/// script must be the same clean no-op — not an exec fallback error.
#[test]
fn top_level_if_present_is_a_noop_for_missing_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "echo built" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("--if-present").with_arg("nonexistent").assert().success();

    drop(root);
}

/// `pnpm run` with no script name lists the available scripts, grouped
/// into lifecycle scripts and others. Mirrors pnpm's `printProjectCommands`.
#[test]
fn run_lists_scripts_when_no_name_given() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "echo built", "test": "echo tested" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    let output = pacquet.with_arg("run").output().expect("spawn pacquet run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("STDOUT:\n{stdout}\n");
    assert!(output.status.success(), "listing scripts should succeed");
    assert!(stdout.contains("Commands available via"), "should list non-lifecycle scripts");
    assert!(stdout.contains("build"), "should list the build script");
    assert!(stdout.contains("Lifecycle scripts:"), "should group lifecycle scripts");

    drop(root);
}

/// A `/pattern/` positional selects every matching script rather than
/// naming one, through both `pnpm run <selector>` and the bare
/// `pnpm <selector>` fallback.
#[cfg(unix)]
#[test]
fn run_executes_every_script_matching_a_regexp_selector() {
    for prefix in [&["run"][..], &[][..]] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        let manifest = json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": {
                "typecheck:one": format!(r#"touch "{}""#, workspace.join("one.txt").display()),
                "typecheck:two": format!(r#"touch "{}""#, workspace.join("two.txt").display()),
                "build": format!(r#"touch "{}""#, workspace.join("build.txt").display()),
            },
        })
        .to_string();
        fs::write(workspace.join("package.json"), manifest).expect("write package.json");

        pacquet.with_args(prefix).with_arg("/^typecheck:.+/").assert().success();

        assert!(workspace.join("one.txt").exists(), "prefix: {prefix:?}");
        assert!(workspace.join("two.txt").exists(), "prefix: {prefix:?}");
        assert!(!workspace.join("build.txt").exists(), "prefix: {prefix:?}");

        drop(root);
    }
}

#[test]
fn regexp_selected_scripts_run_concurrently_by_default() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
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
  console.log(self)
}
",
    )
    .expect("write concurrency probe");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": {
                "dev:one": "node track-concurrency.js one two",
                "dev:two": "node track-concurrency.js two one",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let output = pacquet
        .with_args(["--workspace-concurrency=2", "run", "/^dev:/"])
        .assert()
        .success()
        .get_output()
        .clone();

    assert!(workspace.join("saw-parallel").exists(), "the selected scripts should overlap");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("STDOUT:\n{stdout}\n");
    assert!(stdout.contains("dev:one: one"));
    assert!(stdout.contains("dev:two: two"));

    drop(root);
}

#[test]
pub(super) fn regexp_selected_scripts_cancel_siblings_after_failure() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": {
                "dev:slow": r#"node -e "const fs = require('fs'); fs.writeFileSync('slow-started', ''); setTimeout(() => fs.writeFileSync('slow-finished', ''), 30000)""#,
                "dev:fail": r#"node -e "const fs = require('fs'); const wait = () => fs.existsSync('slow-started') ? process.exit(17) : setTimeout(wait, 10); wait()""#,
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    assert_cmd::Command::from_std(pacquet)
        .args(["--workspace-concurrency=2", "run", "/^dev:/"])
        .timeout(Duration::from_mins(1))
        .assert()
        .code(17);
    assert!(
        !workspace.join("slow-finished").exists(),
        "the slow script must be cancelled before its watchdog completes",
    );

    drop(root);
}

#[test]
fn regexp_selected_scripts_no_bail_lets_siblings_finish() {
    assert_no_bail_lets_siblings_finish("--workspace-concurrency=2");
}

#[test]
fn regexp_selected_scripts_no_bail_runs_every_script_sequentially() {
    assert_no_bail_lets_siblings_finish("--workspace-concurrency=1");
}

/// With several failures under `--no-bail`, the listing follows the
/// selection order, not the order the scripts finished in.
#[test]
fn no_bail_reports_failures_in_selection_order() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": {
                "check:slow-fail": r#"node -e "setTimeout(() => process.exit(4), 500)""#,
                "check:fast-fail": r#"node -e "process.exit(3)""#,
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let output = assert_cmd::Command::from_std(pacquet)
        .args(["--workspace-concurrency=2", "--no-bail", "run", "/^check:/"])
        .timeout(Duration::from_mins(1))
        .assert()
        .code(1)
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Some scripts failed: 2 of 2"), "got: {stderr}");
    let slow = stderr.find("check:slow-fail: exit").expect("slow-fail is listed");
    let fast = stderr.find("check:fast-fail: exit").expect("fast-fail is listed");
    assert!(slow < fast, "failures should follow the selection order, got: {stderr}");

    drop(root);
}

/// As in pnpm 11, `--no-bail` also wraps a single selected script's
/// failure in `ERR_PNPM_RUN_FAILED` with exit code 1 rather than exiting
/// with the script's own status.
#[test]
fn no_bail_reports_a_single_failed_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": { "check": r#"node -e "process.exit(3)""# },
        })
        .to_string(),
    )
    .expect("write package.json");

    let output = assert_cmd::Command::from_std(pacquet)
        .args(["--no-bail", "run", "check"])
        .timeout(Duration::from_mins(1))
        .assert()
        .code(1)
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_RUN_FAILED")
            && stderr.contains("Some scripts failed: 1 of 1")
            && stderr.contains("check: exit"),
        "got: {stderr}",
    );

    drop(root);
}

/// Flags on a selector say nothing about which scripts to pick, so pnpm
/// rejects them instead of honouring a subset.
#[cfg(unix)]
#[test]
fn run_rejects_regexp_flags_in_a_selector() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "true" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    let output = pacquet.with_args(["run", "/^BUILD/i"]).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_UNSUPPORTED_SCRIPT_COMMAND_FORMAT"),
        "should reject the flags:\n{stderr}",
    );

    drop(root);
}
