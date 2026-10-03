use super::{CommandExtra, CommandTempCwd, Path, assert_eq, fs, pacquet_at};
use assert_cmd::assert::OutputAssertExt;

/// A `node -e` script that waits up to 30 seconds for the mark `other`
/// leaves in the workspace root, then records `name` in `order.txt` there.
fn wait_for_then_record(other: &str, name: &str) -> String {
    format!(
        r#"node -e "const fs = require('fs'); const deadline = Date.now() + 30000; const poll = () => {{ if (fs.existsSync('../../{other}')) return fs.appendFileSync('../../order.txt', '{name};'); if (Date.now() > deadline) process.exit(1); setTimeout(poll, 20); }}; poll()""#,
    )
}

/// A `lib` project and an `app` project depending on it through
/// `workspace:*`, with a lockfile each.
fn lib_and_app(workspace: &Path, lib_scripts: &serde_json::Value, app_scripts: &serde_json::Value) {
    workspace_projects(
        workspace,
        &[
            ("lib", serde_json::json!({}), lib_scripts.clone()),
            ("app", serde_json::json!({ "lib": "workspace:*" }), app_scripts.clone()),
        ],
    );
}

/// A workspace of `(name, dependencies, scripts)` projects under
/// `packages/`, with a lockfile each.
fn workspace_projects(workspace: &Path, projects: &[(&str, serde_json::Value, serde_json::Value)]) {
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root package.json");
    for (name, dependencies, scripts) in projects {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("mkdir project");
        fs::write(
            dir.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": dependencies,
                "scripts": scripts,
            })
            .to_string(),
        )
        .expect("write project package.json");
    }
}

/// A project resolves while a workspace project it depends on still runs
/// its scripts, and runs its own scripts only after them. The `readPackage`
/// hook marks that `app` is resolving, which `lib`'s `postinstall` waits
/// for: an install that started `app` only after `lib` finished would time
/// that script out.
#[test]
fn a_project_resolves_while_its_workspace_dependency_runs_its_scripts() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let workspace = &fixture.workspace;
    lib_and_app(
        workspace,
        &serde_json::json!({ "postinstall": wait_for_then_record("app-resolving", "lib") }),
        &serde_json::json!({
            "postinstall": r#"node -e "require('fs').appendFileSync('../../order.txt', 'app;')""#,
        }),
    );
    fs::write(
        workspace.join("packages/app/.pnpmfile.cjs"),
        r"const fs = require('fs');
const path = require('path');
module.exports = { hooks: { readPackage(pkg) {
  fs.writeFileSync(path.join(__dirname, '../../app-resolving'), '');
  return pkg;
} } };
",
    )
    .expect("write app pnpmfile");
    fs::write(
        workspace.join("packages/lib/.pnpmfile.cjs"),
        "module.exports = { hooks: { readPackage: (pkg) => pkg } };\n",
    )
    .expect("write lib pnpmfile");

    pacquet_at(workspace)
        .with_arg("install")
        .assert()
        .success();

    let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
    assert_eq!(order, "lib;app;");
}

/// A project's `preinstall` and `pnpm:devPreinstall` scripts run before
/// its install could wait for the workspace projects it depends on, so such
/// a project waits for them before it starts at all.
fn a_project_with_a_pre_resolution_script_waits_for_its_workspace_dependencies(script: &str) {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let workspace = &fixture.workspace;
    lib_and_app(
        workspace,
        &serde_json::json!({
            "postinstall": r#"node -e "setTimeout(() => require('fs').writeFileSync('../../lib-built', ''), 500)""#,
        }),
        &serde_json::json!({
            script: r#"node -e "process.exit(require('fs').existsSync('../../lib-built') ? 0 : 1)""#,
        }),
    );

    pacquet_at(workspace)
        .with_arg("install")
        .assert()
        .success();
}

#[test]
fn a_project_with_a_preinstall_script_waits_for_its_workspace_dependencies() {
    a_project_with_a_pre_resolution_script_waits_for_its_workspace_dependencies("preinstall");
}

#[test]
fn a_project_with_a_dev_preinstall_script_waits_for_its_workspace_dependencies() {
    a_project_with_a_pre_resolution_script_waits_for_its_workspace_dependencies(
        "pnpm:devPreinstall",
    );
}

/// Once a project fails, a project still waiting for its workspace
/// dependencies links nothing and runs no script, even though it started
/// before the failure. `broken` fails while `app` waits for `lib`, whose
/// `postinstall` holds `app` at the wait until then.
#[test]
fn a_failure_stops_the_projects_waiting_for_their_workspace_dependencies() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let workspace = &fixture.workspace;
    workspace_projects(
        workspace,
        &[
            (
                "broken",
                serde_json::json!({}),
                serde_json::json!({
                    "postinstall": r#"node -e "require('fs').writeFileSync('../../broken-failing', ''); process.exit(1)""#,
                }),
            ),
            (
                "lib",
                serde_json::json!({}),
                serde_json::json!({
                    "postinstall": r#"node -e "const fs = require('fs'); const deadline = Date.now() + 30000; const poll = () => { if (fs.existsSync('../../broken-failing')) return setTimeout(() => {}, 1000); if (Date.now() > deadline) process.exit(1); setTimeout(poll, 20); }; poll()""#,
                }),
            ),
            (
                "app",
                serde_json::json!({ "lib": "workspace:*" }),
                serde_json::json!({
                    "postinstall": r#"node -e "require('fs').writeFileSync('../../app-ran', '')""#,
                }),
            ),
        ],
    );

    pacquet_at(workspace)
        .with_arg("install")
        .assert()
        .failure();

    assert!(workspace.join("broken-failing").exists());
    assert!(!workspace.join("app-ran").exists());
}

/// On a repeat install that finds a project's tree up to date, the project
/// still runs its scripts only after the workspace projects it depends on
/// ran theirs.
#[test]
fn an_up_to_date_project_runs_its_scripts_after_its_workspace_dependencies() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let workspace = &fixture.workspace;
    lib_and_app(
        workspace,
        &serde_json::json!({
            "postinstall": r#"node -e "setTimeout(() => require('fs').writeFileSync('../../lib-built', ''), 500)""#,
        }),
        &serde_json::json!({
            "postinstall": r#"node -e "process.exit(require('fs').existsSync('../../lib-built') ? 0 : 1)""#,
        }),
    );
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&yaml_path, format!("{yaml}optimisticRepeatInstall: false\n"))
        .expect("write pnpm-workspace.yaml");
    pacquet_at(workspace)
        .with_arg("install")
        .assert()
        .success();
    fs::remove_file(workspace.join("lib-built")).expect("clear the first install's mark");

    let output = pacquet_at(workspace)
        .with_arg("install")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&output);
    eprintln!("STDOUT:\n{stdout}");
    assert!(
        stdout.contains("Already up to date"),
        "the repeat install must take the up-to-date path this test covers",
    );
}
