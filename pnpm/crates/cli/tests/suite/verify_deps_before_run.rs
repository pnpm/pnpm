//! E2E coverage for the `verify-deps-before-run` gate, mirroring the
//! TypeScript scenarios in `pnpm11/pnpm/test/verifyDepsBeforeRun/` that
//! translate to pacquet (the interactive `prompt` flow needs a PTY and
//! is exercised only through its non-interactive error branch).

pub use _utils::*;

use crate::_utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    diagnostics::assert_diagnostic_contains,
    fs::{backdate_existing_files, bump_mtime},
};
use serde_json::json;
use std::{fs, path::Path};

fn write_manifest(workspace: &Path, marker: &Path) {
    write_named_manifest(workspace, "verify-deps-project", marker);
}

/// The fixture manifest with a `link:` dependency on a local package, so a
/// never-installed project has something to install without a registry.
fn write_named_manifest(workspace: &Path, name: &str, marker: &Path) {
    let linked = workspace.join("linked-dep");
    fs::create_dir_all(&linked).expect("create the linked package");
    fs::write(linked.join("package.json"), json!({ "name": "linked-dep" }).to_string())
        .expect("write the linked package manifest");
    write_named_manifest_with_dependency_groups(
        workspace,
        name,
        marker,
        json!({ "dependencies": { "linked-dep": "link:./linked-dep" } }),
    );
}

#[cfg(unix)]
fn write_manifest_with_dependency_groups(
    workspace: &Path,
    marker: &Path,
    groups: serde_json::Value,
) {
    write_named_manifest_with_dependency_groups(workspace, "verify-deps-project", marker, groups);
}

/// The fixture manifest — a `hello` script that touches `marker` —
/// extended with the dependency groups the caller needs.
fn write_named_manifest_with_dependency_groups(
    workspace: &Path,
    name: &str,
    marker: &Path,
    groups: serde_json::Value,
) {
    let serde_json::Value::Object(mut manifest) = json!({
        "name": name,
        "version": "0.0.0",
        "scripts": {
            "hello": format!(r#"touch "{}""#, marker.display()),
        },
    }) else {
        unreachable!("the manifest literal is an object")
    };
    let serde_json::Value::Object(groups) = groups else {
        panic!("the dependency groups must be an object")
    };
    manifest.extend(groups);
    fs::write(workspace.join("package.json"), serde_json::Value::Object(manifest).to_string())
        .expect("write package.json");
}

/// The default action is `install` (pnpm's
/// `'verify-deps-before-run': 'install'`): a fresh project's first
/// `run` spawns an install before executing the script.
#[cfg(unix)]
#[test]
fn default_install_action_installs_before_running_the_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    pacquet
        .with_args(["run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the script must run after the spawned install");
    assert!(workspace.join("node_modules").exists(), "the gate must have spawned an install first");

    drop(root);
}

/// Concurrent gates on one stale tree start one install instead of one
/// each racing in the same `node_modules`: the others wait for it, find
/// the dependencies up to date, and run their scripts
/// ([pnpm/pnpm#14551](https://github.com/pnpm/pnpm/issues/14551)).
#[cfg(unix)]
#[test]
fn concurrent_gates_on_a_stale_tree_start_one_install() {
    use std::process::Stdio;

    const RUNS: usize = 4;
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    let project = workspace.join("packages/project");
    fs::create_dir_all(&project).expect("create workspace project");
    fs::write(
        project.join("package.json"),
        json!({ "name": "project", "version": "1.0.0" }).to_string(),
    )
    .expect("write the workspace project manifest");
    let installs = workspace.join("installs.log");
    let write_root_manifest = |dependencies: serde_json::Value| {
        let manifest = json!({
            "name": "verify-deps-root",
            "version": "0.0.0",
            "scripts": {
                "hello": "true",
                // Keep the install running until every gate has checked
                // the stale tree.
                "postinstall": format!(r#"echo installed >> "{}" && sleep 2"#, installs.display()),
            },
            "dependencies": dependencies,
        });
        fs::write(workspace.join("package.json"), manifest.to_string())
            .expect("write the root manifest");
    };

    write_root_manifest(json!({}));
    pacquet
        .with_arg("install")
        .assert()
        .success();
    fs::remove_file(&installs).expect("reset the install log");

    write_root_manifest(json!({ "project": "workspace:*" }));
    bump_mtime(&workspace.join("package.json"));
    let runs = (0..RUNS)
        .map(|_| {
            pacquet_in(&workspace)
                .with_args(["run", "hello"])
                .with_stdout(Stdio::null())
                .with_stderr(Stdio::piped())
                .spawn()
                .expect("spawn pacquet run")
        })
        .collect::<Vec<_>>();
    for run in runs {
        let output = run.wait_with_output().expect("wait for pacquet run");
        assert!(
            output.status.success(),
            "every run must succeed:\n{}",
            String::from_utf8_lossy(&output.stderr),
        );
    }

    let install_count = fs::read_to_string(&installs)
        .expect("read the install log")
        .lines()
        .count();
    assert_eq!(install_count, 1, "the concurrent gates must share one install");
    assert!(
        workspace.join("node_modules/project").exists(),
        "the shared install must link the new dependency",
    );

    drop(root);
}

/// The spawned install reproduces the dependency groups the last
/// install recorded, spelled the way the CLI accepts them, so a
/// production-only install leaves `pnpm run` working
/// ([pnpm/pnpm#14147](https://github.com/pnpm/pnpm/issues/14147)).
#[cfg(unix)]
#[test]
fn install_action_reruns_a_production_only_install() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let marker = workspace.join("marker.txt");
    let write_project = |foo_version: &str| {
        write_manifest_with_dependency_groups(
            &workspace,
            &marker,
            json!({
                "dependencies": {
                    "@pnpm.e2e/foo": foo_version,
                },
                "devDependencies": {
                    "@pnpm.e2e/bar": "100.0.0",
                },
            }),
        );
    };

    write_project("100.0.0");
    pacquet
        .with_args(["install", "--prod"])
        .assert()
        .success();
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/bar").exists(),
        "a production-only install must skip devDependencies",
    );

    write_project("100.1.0");
    bump_mtime(&workspace.join("package.json"));

    pacquet_in(&workspace)
        .with_args(["run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the script must run after the spawned install");
    let installed: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("node_modules/@pnpm.e2e/foo/package.json"))
            .expect("read the installed @pnpm.e2e/foo manifest"),
    )
    .expect("parse the installed @pnpm.e2e/foo manifest");
    assert_eq!(
        installed["version"], "100.1.0",
        "the spawned install must install the updated production dependency",
    );
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/bar").exists(),
        "the spawned install must keep the recorded production-only groups",
    );

    drop((root, mock_instance));
}

#[test]
fn dedupe_peers_lockfile_regeneration_installs_before_running_the_script() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "dedupePeers", true);
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "verify-deps-project",
            "version": "0.0.0",
            "dependencies": {
                "@pnpm.e2e/foo": "100.0.0",
            },
            "scripts": {
                "hello": r#"node -e "require('fs').appendFileSync('postinstall.log', 'h')""#,
                "postinstall": r#"node -e "require('fs').appendFileSync('postinstall.log', 'x')""#,
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(workspace.join("postinstall.log")).expect("read postinstall log"),
        "x",
    );

    let mut state = pnpm_workspace_state::load_workspace_state(&workspace)
        .expect("read workspace state")
        .expect("installed workspace state");
    state.last_validated_timestamp = backdate_existing_files(&workspace);
    pnpm_workspace_state::update_workspace_state(&workspace, &state)
        .expect("record backdated validation");

    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_in(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let regenerated_lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read regenerated lockfile");

    let output = pacquet_in(&workspace)
        .with_args(["run", "hello"])
        .output()
        .expect("run script after lockfile regeneration");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(output.status.success(), "the script must run successfully");
    assert!(
        stderr.contains("Lockfile is up to date, resolution step is skipped"),
        "the verifier install must reuse the regenerated lockfile:\n{stderr}",
    );
    let policy_verdict = stderr
        .find("Lockfile passes supply-chain policies")
        .expect("the verifier must report its lockfile policy verdict");
    let frozen_install = stderr
        .find("Lockfile is up to date, resolution step is skipped")
        .expect("the verifier must report the frozen install");
    let up_to_date = stderr
        .find("Already up to date")
        .expect("the verifier must report that no packages changed");
    assert!(
        policy_verdict < frozen_install && frozen_install < up_to_date,
        "the verifier messages must match pnpm's order:\n{stderr}",
    );
    assert_eq!(
        fs::read_to_string(workspace.join("pnpm-lock.yaml"))
            .expect("read lockfile after verifier install"),
        regenerated_lockfile,
    );
    assert_eq!(
        fs::read_to_string(workspace.join("postinstall.log")).expect("read postinstall log"),
        "xxh",
    );

    pacquet_in(&workspace)
        .with_args(["run", "hello"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(workspace.join("postinstall.log")).expect("read postinstall log"),
        "xxhh",
    );

    drop((root, mock_instance));
}

#[cfg(unix)]
#[test]
fn error_action_follows_the_dependency_state() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    let output = pacquet
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .output()
        .expect("spawn pacquet run");
    assert!(!output.status.success(), "running before any install must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN")
            && stderr.contains("Cannot check whether dependencies are outdated"),
        "expected the verify-deps error:\n{stderr}",
    );
    assert!(!marker.exists(), "the script must not run");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    pacquet_in(&workspace)
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the script must run once dependencies are in sync");

    // An mtime-only rewrite (same content) must still pass: the gate
    // re-checks the content against the lockfile instead of trusting
    // the mtime.
    let manifest = fs::read_to_string(workspace.join("package.json")).expect("read package.json");
    fs::write(workspace.join("package.json"), manifest).expect("rewrite package.json");
    bump_mtime(&workspace.join("package.json"));
    pacquet_in(&workspace)
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .assert()
        .success();

    // Deleting pnpm-lock.yaml and the current lockfile leaves nothing to
    // stand in for it, so the check fails like pnpm's
    // RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND — and the pre-run check must not
    // recreate the file (pnpm's run path never restores the lockfile; only
    // the install command does).
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    fs::remove_file(workspace.join("node_modules/.pnpm/lock.yaml"))
        .expect("remove the current lockfile");
    let output = pacquet_in(&workspace)
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .output()
        .expect("spawn pacquet run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(!output.status.success(), "a missing lockfile must fail");
    assert!(stderr.contains("Cannot find a lockfile in"), "expected the lockfile error:\n{stderr}");
    assert!(
        !workspace.join("pnpm-lock.yaml").exists(),
        "the pre-run check must not write pnpm-lock.yaml",
    );
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(workspace.join("pnpm-lock.yaml").exists(), "install must restore the lockfile");

    // A manifest that no longer matches the lockfile must fail again.
    let mut manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    manifest["dependencies"] = json!({ "@pnpm.e2e/foo": "100.0.0" });
    fs::write(workspace.join("package.json"), manifest.to_string())
        .expect("write modified package.json");
    bump_mtime(&workspace.join("package.json"));
    let output = pacquet_in(&workspace)
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .output()
        .expect("spawn pacquet run");
    assert!(!output.status.success(), "an out-of-sync manifest must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN"),
        "expected the verify-deps error:\n{stderr}",
    );

    drop(root);
}

/// Dedicated workspace lockfiles record one project in each workspace
/// state file. The pre-run check must validate the project being run,
/// rather than comparing that state to every workspace package.
#[cfg(unix)]
#[test]
fn separate_lockfiles_check_only_the_active_workspace_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let root_marker = workspace.join("root-marker.txt");
    write_manifest(&workspace, &root_marker);
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "verifyDepsBeforeRun: error\nsharedWorkspaceLockfile: false\npackages:\n  - packages/*\n",
    )
    .expect("write pnpm-workspace.yaml");

    let project = workspace.join("packages/project");
    fs::create_dir_all(&project).expect("create workspace project");
    let project_marker = project.join("project-marker.txt");
    write_manifest(&project, &project_marker);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    pacquet_in(&workspace)
        .with_args(["run", "hello"])
        .assert()
        .success();
    pacquet_in(&project)
        .with_args(["run", "hello"])
        .assert()
        .success();
    assert!(root_marker.exists(), "the root script must run");
    assert!(project_marker.exists(), "the workspace script must run");

    // The check is scoped to the active project, not skipped: a
    // dependency the project's own lockfile does not carry still fails.
    write_manifest_with_dependency_groups(
        &project,
        &project_marker,
        json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
    );
    bump_mtime(&project.join("package.json"));
    let output = pacquet_in(&project)
        .with_args(["run", "hello"])
        .output()
        .expect("spawn pacquet run");
    assert!(!output.status.success(), "an out-of-sync project must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN"),
        "expected the verify-deps error:\n{stderr}",
    );

    drop(root);
}

/// A workspace root needs no package manifest of its own. With dedicated
/// lockfiles the gate reads the manifest of the nested project that owns
/// the lockfile and the state.
#[cfg(unix)]
#[test]
fn separate_lockfiles_allow_a_nested_project_without_a_root_manifest() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "verifyDepsBeforeRun: error\nsharedWorkspaceLockfile: false\npackages:\n  - packages/*\n",
    )
    .expect("write pnpm-workspace.yaml");

    let project = workspace.join("packages/project");
    fs::create_dir_all(&project).expect("create workspace project");
    let marker = project.join("project-marker.txt");
    write_manifest(&project, &marker);

    pacquet_in(&project)
        .with_arg("install")
        .assert()
        .success();
    pacquet_in(&project)
        .with_args(["run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the nested workspace script must run");

    drop(root);
}

/// With `sharedWorkspaceLockfile: false`, a filtered install writes the state
/// for the selected project, but not for the workspace root. A recursive or
/// filtered run from the root must check the selected project's state rather
/// than expecting a root workspace state file (pnpm/pnpm#15272).
#[cfg(unix)]
#[test]
fn separate_lockfiles_filtered_recursive_run_checks_selected_project() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "verifyDepsBeforeRun: error\nsharedWorkspaceLockfile: false\npackages:\n  - packages/*\n",
    )
    .expect("write pnpm-workspace.yaml");

    let project_a = workspace.join("packages/project-a");
    let project_b = workspace.join("packages/project-b");
    fs::create_dir_all(&project_a).expect("create project-a");
    fs::create_dir_all(&project_b).expect("create project-b");
    let marker_a = project_a.join("marker-a.txt");
    let marker_b = project_b.join("marker-b.txt");
    write_named_manifest(&project_a, "project-a", &marker_a);
    write_named_manifest(&project_b, "project-b", &marker_b);

    pacquet_in(&workspace)
        .with_args(["--filter", "project-a", "install"])
        .assert()
        .success();

    pacquet_in(&workspace)
        .with_args(["--filter", "project-a", "run", "hello"])
        .assert()
        .success();
    assert!(marker_a.exists(), "project-a script must run");
    fs::remove_file(&marker_a).expect("clean marker-a");

    pacquet_in(&workspace)
        .with_args(["--filter", "project-a", "hello"])
        .assert()
        .success();
    assert!(marker_a.exists(), "project-a shortcut script must run");

    pacquet_in(&workspace)
        .with_args(["--filter", "project-a", "exec", "node", "-e", "0"])
        .assert()
        .success();

    let project_c = workspace.join("packages/project-c");
    fs::create_dir_all(&project_c).expect("create project-c");
    fs::write(
        project_c.join("package.json"),
        json!({ "name": "project-c", "version": "1.0.0" }).to_string(),
    )
    .expect("write project-c package.json");

    pacquet_in(&workspace)
        .with_args([
            "--filter",
            "project-a",
            "--filter",
            "project-c",
            "run",
            "--if-present",
            "hello",
        ])
        .assert()
        .success();
    assert!(marker_a.exists(), "project-a script must run with uninstalled project-c skipped");
    fs::remove_file(&marker_a).expect("clean marker-a");

    let output = pacquet_in(&workspace)
        .with_args(["--filter", "project-b", "run", "hello"])
        .output()
        .expect("spawn pacquet run");
    assert!(!output.status.success(), "uninstalled project-b must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN"),
        "expected the verify-deps error for uninstalled project:\n{stderr}",
    );

    write_named_manifest_with_dependency_groups(
        &project_a,
        "project-a",
        &marker_a,
        json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
    );
    bump_mtime(&project_a.join("package.json"));
    let output = pacquet_in(&workspace)
        .with_args(["--filter", "project-a", "run", "hello"])
        .output()
        .expect("spawn pacquet run");
    assert!(!output.status.success(), "out-of-sync project-a must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN"),
        "expected verify-deps error for out-of-sync project:\n{stderr}",
    );

    write_named_manifest(&project_a, "project-a", &marker_a);
    pacquet_in(&workspace)
        .with_args(["--filter", "project-a", "install"])
        .assert()
        .success();
    pacquet_in(&workspace)
        .with_args(["--filter", "project-b", "install"])
        .assert()
        .success();
    let _ = fs::remove_file(&marker_a);

    pacquet_in(&workspace)
        .with_args(["--recursive", "run", "hello"])
        .assert()
        .success();
    assert!(marker_a.exists(), "project-a script must run under recursive");
    assert!(marker_b.exists(), "project-b script must run under recursive");

    drop(root);
}

/// A filtered install leaves the other projects out of the current lockfile.
/// A lockfile that is only newer, such as one a Docker `COPY` wrote, must not
/// make the run gate treat it as outdated (pnpm/pnpm#16322).
#[cfg(unix)]
#[test]
fn filtered_install_accepts_a_touched_lockfile_with_unchanged_contents() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "verifyDepsBeforeRun: error\npackages:\n  - packages/*\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), json!({ "name": "root" }).to_string())
        .expect("write the root package.json");

    let project_a = workspace.join("packages/project-a");
    let project_b = workspace.join("packages/project-b");
    fs::create_dir_all(&project_a).expect("create project-a");
    fs::create_dir_all(&project_b).expect("create project-b");
    let marker_a = project_a.join("marker-a.txt");
    write_named_manifest(&project_a, "project-a", &marker_a);
    write_named_manifest(&project_b, "project-b", &project_b.join("marker-b.txt"));

    pacquet_in(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    pacquet_in(&workspace)
        .with_args(["--filter", "project-a", "install", "--frozen-lockfile"])
        .assert()
        .success();
    bump_mtime(&workspace.join("pnpm-lock.yaml"));

    pacquet_in(&workspace)
        .with_args(["--filter", "project-a", "run", "hello"])
        .assert()
        .success();
    assert!(marker_a.exists(), "project-a script must run");

    drop(root);
}

/// With `sharedWorkspaceLockfile: false` and project-specific `packageConfigs`
/// overrides, running a script inside the project or via `--filter` right
/// after install must not fail the `verifyDepsBeforeRun` check (pnpm/pnpm#15545).
#[cfg(unix)]
#[test]
fn separate_lockfiles_with_package_configs_overrides_allows_script_run() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "verifyDepsBeforeRun: error\nsharedWorkspaceLockfile: false\npackages:\n  - packages/*\npackageConfigs:\n  project-a:\n    overrides:\n      ms: 2.0.0\n",
    )
    .expect("write pnpm-workspace.yaml");

    let project_a = workspace.join("packages/project-a");
    let project_b = workspace.join("packages/project-b");
    fs::create_dir_all(&project_a).expect("create project-a");
    fs::create_dir_all(&project_b).expect("create project-b");
    let marker_a = project_a.join("marker-a.txt");
    let marker_b = project_b.join("marker-b.txt");
    write_named_manifest(&project_a, "project-a", &marker_a);
    write_named_manifest(&project_b, "project-b", &marker_b);

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    pacquet_in(&project_a)
        .with_args(["run", "hello"])
        .assert()
        .success();
    assert!(marker_a.exists(), "project-a script must run from inside package dir");
    fs::remove_file(&marker_a).expect("clean marker-a");

    pacquet_in(&workspace)
        .with_args(["--filter", "project-a", "run", "hello"])
        .assert()
        .success();
    assert!(marker_a.exists(), "project-a script must run via --filter from root");
    fs::remove_file(&marker_a).expect("clean marker-a");

    pacquet_in(&project_a)
        .with_args(["exec", "node", "-e", "0"])
        .assert()
        .success();

    pacquet_in(&project_b)
        .with_args(["run", "hello"])
        .assert()
        .success();
    assert!(marker_b.exists(), "project-b without overrides must also run");
    fs::remove_file(&marker_b).expect("clean marker-b");

    // When the override setting in pnpm-workspace.yaml changes, the check
    // must detect the drift and fail.
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "verifyDepsBeforeRun: error\nsharedWorkspaceLockfile: false\npackages:\n  - packages/*\npackageConfigs:\n  project-a:\n    overrides:\n      ms: 3.0.0\n",
    )
    .expect("write updated pnpm-workspace.yaml");

    let output = pacquet_in(&project_a)
        .with_args(["run", "hello"])
        .output()
        .expect("spawn pacquet run");
    assert!(!output.status.success(), "project-a must fail after overrides drift");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN"),
        "expected verify-deps error after overrides drift:\n{stderr}",
    );
    assert!(
        stderr.contains("overrides"),
        "expected overrides setting drift in error message:\n{stderr}",
    );

    drop(root);
}

/// One shared lockfile covers every project, so a command run from a
/// directory that has no manifest of its own is still checked against
/// the workspace root's state.
#[cfg(unix)]
#[test]
fn a_shared_lockfile_is_checked_from_a_directory_without_a_manifest() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &workspace.join("marker.txt"));
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "verifyDepsBeforeRun: error\npackages:\n  - packages/*\n",
    )
    .expect("write pnpm-workspace.yaml");
    let tools = workspace.join("tools");
    fs::create_dir_all(&tools).expect("create the directory without a manifest");

    let output = pacquet_in(&tools)
        .with_args(["exec", "true"])
        .output()
        .expect("spawn pacquet exec");
    assert!(!output.status.success(), "exec before any install must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN"),
        "expected the verify-deps error:\n{stderr}",
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();
    pacquet_in(&tools)
        .with_args(["exec", "true"])
        .assert()
        .success();

    drop(root);
}

/// `warn` reports the drift but still runs the script.
#[cfg(unix)]
#[test]
fn warn_action_warns_and_runs_the_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    let output = pacquet
        .with_args(["--config.verify-deps-before-run=warn", "run", "hello"])
        .output()
        .expect("spawn pacquet run");
    assert!(output.status.success(), "warn mode must not block the script");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Your node_modules are out of sync with your lockfile."),
        "expected the out-of-sync warning:\n{stderr}",
    );
    assert!(marker.exists(), "the script must run");
    assert!(!workspace.join("node_modules").exists(), "warn mode must not install");

    drop(root);
}

/// A lockfile that records overrides the root manifest now keeps only in its
/// ignored `pnpm` field must not be rewritten by an implicit install, which
/// would drop them. The gate refuses and leaves the lockfile alone
/// ([pnpm/pnpm#16278](https://github.com/pnpm/pnpm/issues/16278)).
#[test]
fn install_action_refuses_to_drop_settings_of_the_ignored_pnpm_field() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);
    fs::write(workspace.join("pnpm-workspace.yaml"), "overrides:\n  foo: 1.0.0\n")
        .expect("write pnpm-workspace.yaml");
    pacquet
        .with_arg("install")
        .assert()
        .success();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read the lockfile");
    assert!(lockfile.contains("overrides:"), "the lockfile must record the overrides:\n{lockfile}");

    fs::remove_file(workspace.join("pnpm-workspace.yaml")).expect("remove pnpm-workspace.yaml");
    write_named_manifest_with_dependency_groups(
        &workspace,
        "verify-deps-project",
        &marker,
        json!({ "pnpm": { "overrides": { "foo": "1.0.0" }, "onlyBuiltDependencies": [] } }),
    );
    // `prompt` cannot ask here, and must not advise a plain install either.
    for action in ["install", "prompt"] {
        assert_refuses_to_drop_overrides(&workspace, action);
    }
    fs::remove_file(workspace.join("package.json")).expect("remove package.json");
    fs::write(
        workspace.join("package.yaml"),
        format!(
            "name: verify-deps-project\nversion: 0.0.0\nscripts:\n  hello: touch \"{}\"\npnpm:\n  overrides:\n    foo: 1.0.0\n",
            marker.display(),
        ),
    )
    .expect("write package.yaml");
    assert_refuses_to_drop_overrides(&workspace, "install");
    assert!(!marker.exists(), "the script must not run");
    assert_eq!(
        fs::read_to_string(&lockfile_path).expect("read the lockfile"),
        lockfile,
        "the lockfile must be left alone",
    );

    drop(root);
}

fn assert_refuses_to_drop_overrides(workspace: &Path, action: &str) {
    let output = pacquet_in(workspace)
        .with_args([&format!("--config.verify-deps-before-run={action}"), "run", "hello"])
        .output()
        .expect("spawn pacquet run");
    assert!(!output.status.success(), "{action} mode must refuse to install");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_diagnostic_contains(
        &stderr,
        r#"installing would drop "pnpm.overrides" from the lockfile, because the "pnpm" field in package.json is no longer read by pnpm"#,
    );
    assert_diagnostic_contains(&stderr, "Move these settings to pnpm-workspace.yaml");
}

/// `prompt` cannot ask in a non-interactive environment and must fail
/// with the dedicated hint instead of hanging.
#[test]
fn prompt_action_errors_when_not_interactive() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &workspace.join("marker.txt"));

    let output = pacquet
        .with_args(["--config.verify-deps-before-run=prompt", "run", "hello"])
        .output()
        .expect("spawn pacquet run");
    assert!(!output.status.success(), "prompt mode must fail without a TTY");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // miette wraps the help text, so collapse whitespace before matching.
    let stderr_flat = stderr
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN")
            && stderr_flat.contains(
                "cannot prompt for confirmation in non-interactive environments"
            ),
        "expected the non-interactive prompt error:\n{stderr}",
    );

    drop(root);
}

/// `false` disables the gate entirely: the script runs and nothing is
/// installed.
#[cfg(unix)]
#[test]
fn false_disables_the_gate() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    pacquet
        .with_args(["--config.verify-deps-before-run=false", "run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the script must run");
    assert!(!workspace.join("node_modules").exists(), "no install may be spawned");

    drop(root);
}

/// Every spawned script sees `pnpm_config_verify_deps_before_run=false`,
/// so a nested `pnpm run` / `pnpm exec` never re-enters the check
/// (pnpm/pnpm#10060). Mirrors the TS `checkEnv` assertions.
#[cfg(unix)]
#[test]
fn scripts_get_the_check_disabled_through_their_env() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "verify-deps-project",
        "version": "0.0.0",
        "scripts": {
            "checkEnv": r#"[ "$pnpm_config_verify_deps_before_run" = "false" ]"#,
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet
        .with_args(["run", "checkEnv"])
        .assert()
        .success();

    drop(root);
}

/// The `pnpm_config_verify_deps_before_run` env var outranks even the
/// CLI `--config.` override — that priority is what makes the script
/// env stamp above an effective recursion breaker.
#[cfg(unix)]
#[test]
fn env_var_outranks_the_cli_config_override() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    pacquet
        .with_env("pnpm_config_verify_deps_before_run", "false")
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the script must run with the check disabled by env");

    drop(root);
}

/// A present-but-empty env var disables the gate outright, still
/// overriding the CLI: pnpm applies the variable on presence alone, and
/// the empty string is falsy there.
#[cfg(unix)]
#[test]
fn empty_env_value_disables_the_gate() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    pacquet
        .with_env("pnpm_config_verify_deps_before_run", "")
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the script must run with the gate disabled by the empty env var");
    assert!(!workspace.join("node_modules").exists(), "no check or install may run");

    drop(root);
}

/// The exec path stamps the same recursion guard as the lifecycle env
/// builder.
#[cfg(unix)]
#[test]
fn exec_children_get_the_check_disabled_through_their_env() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &workspace.join("marker.txt"));

    pacquet
        .with_args([
            "--config.verify-deps-before-run=false",
            "exec",
            "sh",
            "-c",
            r#"[ "$pnpm_config_verify_deps_before_run" = "false" ]"#,
        ])
        .assert()
        .success();

    drop(root);
}

/// pnpm assigns the `pnpm_config_verify_deps_before_run` env var
/// verbatim, so an unrecognized value is truthy there: the check runs
/// but matches no action, and the script proceeds.
#[cfg(unix)]
#[test]
fn unrecognized_env_value_checks_without_acting() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    pacquet
        .with_env("pnpm_config_verify_deps_before_run", "definitely-not-an-action")
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the script must run");
    assert!(!workspace.join("node_modules").exists(), "no action may fire");

    drop(root);
}

/// `pnpm exec` runs the same gate as `pnpm run`.
#[cfg(unix)]
#[test]
fn exec_runs_the_gate_too() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &workspace.join("marker.txt"));

    let output = pacquet
        .with_args(["--config.verify-deps-before-run=error", "exec", "true"])
        .output()
        .expect("spawn pacquet exec");
    assert!(!output.status.success(), "exec before any install must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN"),
        "expected the verify-deps error:\n{stderr}",
    );

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    pacquet_in(&workspace)
        .with_args(["--config.verify-deps-before-run=error", "exec", "true"])
        .assert()
        .success();

    drop(root);
}

#[test]
fn exec_keeps_verifier_output_out_of_child_stdout() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "workspace-root", "version": "0.0.0" }).to_string(),
    )
    .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    let project = workspace.join("packages/project");
    fs::create_dir_all(&project).expect("create workspace project");
    write_manifest(&project, &project.join("marker.txt"));

    let output = pacquet_in(&project)
        .with_args([
            "exec",
            "node",
            "-e",
            r#"process.stdout.write(JSON.stringify({workspace:"test"}))"#,
        ])
        .output()
        .expect("spawn pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDOUT:\n{stdout}\n\nSTDERR:\n{stderr}\n");
    assert!(output.status.success(), "exec failed");
    assert_eq!(stdout, r#"{"workspace":"test"}"#);
    assert!(
        stderr.contains("Scope: all 2 workspace projects") && stderr.contains("Done in"),
        "the verifier install must report on stderr:\n{stderr}",
    );

    drop(root);
}

/// A filtered `exec` must not install the whole workspace: the install the
/// gate spawns has to select the same projects the command selected
/// ([pnpm/pnpm#11865](https://github.com/pnpm/pnpm/issues/11865)).
#[test]
fn filtered_exec_installs_only_the_selected_projects() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "workspace-root", "version": "0.0.0" }).to_string(),
    )
    .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    for name in ["project", "other"] {
        let project = workspace.join("packages").join(name);
        fs::create_dir_all(&project).expect("create workspace project");
        write_named_manifest(&project, name, &project.join("marker.txt"));
    }

    let output = pacquet_in(&workspace)
        .with_args([
            "--filter",
            "project",
            "exec",
            "node",
            "-e",
            r#"process.stdout.write("filtered")"#,
        ])
        .output()
        .expect("spawn pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the filtered exec must succeed:\n{stderr}");
    assert_eq!(stdout, "filtered");
    assert!(
        stderr.contains("Done in"),
        "the verify-deps gate must have spawned an install:\n{stderr}",
    );
    assert!(
        !stderr.contains("Scope: all"),
        "the filtered exec must not install the whole workspace:\n{stderr}",
    );

    drop(root);
}

/// A hoisted layout shares one `node_modules` between all projects, so the
/// filtered install the gate spawns must keep the packages that only the
/// unselected projects need
/// ([pnpm/pnpm#16483](https://github.com/pnpm/pnpm/issues/16483)).
#[test]
fn filtered_exec_keeps_the_packages_of_the_other_projects_in_a_hoisted_layout() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "workspace-root", "version": "0.0.0" }).to_string(),
    )
    .expect("write root package.json");
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("nodeLinker: hoisted\npackages:\n  - packages/*\n");
    fs::write(&workspace_yaml, &yaml).expect("write pnpm-workspace.yaml");
    for (name, dependency) in [("project", "@pnpm.e2e/foo"), ("other", "@pnpm.e2e/bar")] {
        let project = workspace.join("packages").join(name);
        fs::create_dir_all(&project).expect("create workspace project");
        write_named_manifest_with_dependency_groups(
            &project,
            name,
            &project.join("marker.txt"),
            json!({ "dependencies": { dependency: "100.0.0" } }),
        );
    }
    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert!(workspace.join("node_modules/@pnpm.e2e/bar").exists());

    yaml.push_str("dedupePeerDependents: false\n");
    fs::write(&workspace_yaml, &yaml).expect("write pnpm-workspace.yaml");
    bump_mtime(&workspace_yaml);

    let output = pacquet_in(&workspace)
        .with_args([
            "--filter",
            "project",
            "exec",
            "node",
            "-e",
            r#"process.stdout.write("filtered")"#,
        ])
        .output()
        .expect("spawn pacquet exec");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the filtered exec must succeed:\n{stderr}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "filtered");
    assert!(
        stderr.contains("Done in"),
        "the verify-deps gate must have spawned an install:\n{stderr}",
    );
    assert!(workspace.join("node_modules/@pnpm.e2e/foo").exists());
    assert!(
        workspace.join("node_modules/@pnpm.e2e/bar").exists(),
        "the spawned install must keep the packages of the unselected project:\n{stderr}",
    );

    drop((root, mock_instance));
}

/// Under dedicated per-project lockfiles the gate installs inside each selected
/// project directory, where the command's selectors need not select the
/// project. A recursive filtered run must not hand them to that install: a
/// selector that matches nothing would install nothing, while an install in a
/// project directory already covers the project.
#[test]
fn separate_lockfiles_recursive_exec_installs_the_selected_projects() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "verifyDepsBeforeRun: install\nsharedWorkspaceLockfile: false\npackages:\n  - packages/*\n",
    )
    .expect("write pnpm-workspace.yaml");
    for name in ["project", "other"] {
        let project = workspace.join("packages").join(name);
        fs::create_dir_all(&project).expect("create workspace project");
        write_named_manifest_with_dependency_groups(
            &project,
            name,
            &project.join("marker.txt"),
            json!({}),
        );
    }

    let output = pacquet_in(&workspace)
        .with_args([
            "--recursive",
            "--filter",
            "./packages/project",
            "exec",
            "node",
            "-e",
            r#"process.stdout.write("filtered")"#,
        ])
        .output()
        .expect("spawn recursive pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the recursive filtered exec must succeed in a per-project-lockfile workspace:\n{stderr}",
    );
    assert_eq!(stdout, "filtered");
    assert!(
        !stderr.contains("Scope: 0 of"),
        "the gate install must not select nothing in a project directory:\n{stderr}",
    );

    drop(root);
}

/// A filtered install leaves the projects it did not select without a modules
/// directory, so the next filtered command has to install the project it
/// selects instead of treating the recorded state as up to date
/// ([pnpm/pnpm#11865](https://github.com/pnpm/pnpm/issues/11865)).
#[test]
fn exec_installs_the_selected_project_after_a_filtered_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "workspace-root", "version": "0.0.0" }).to_string(),
    )
    .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    for name in ["foo", "bar"] {
        let project = workspace.join("packages").join(name);
        fs::create_dir_all(&project).expect("create workspace project");
        write_named_manifest_with_dependency_groups(
            &project,
            name,
            &project.join("marker.txt"),
            json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
        );
    }

    // the first filtered exec installs `foo` and leaves `bar` without a modules directory
    pacquet_in(&workspace)
        .with_args(["--filter", "foo", "exec", "node", "-e", r#"process.stdout.write("foo-ok")"#])
        .assert()
        .success();
    assert!(
        workspace.join("packages/foo/node_modules").exists(),
        "the first filtered exec must install the project it selected",
    );
    assert!(
        !workspace.join("packages/bar/node_modules").exists(),
        "the first filtered exec must not install the project it did not select",
    );

    // the second filtered exec must install `bar` instead of treating the
    // recorded filtered install as up to date
    let output = pacquet_in(&workspace)
        .with_args(["--filter", "bar", "exec", "node", "-e", r#"process.stdout.write("bar-ok")"#])
        .output()
        .expect("spawn pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the second filtered exec must succeed:\n{stderr}");
    assert_eq!(stdout, "bar-ok");
    assert!(
        workspace.join("packages/bar/node_modules/@pnpm.e2e/foo").exists(),
        "the second filtered exec must install the dependencies of the project it selected:\n{stderr}",
    );

    drop((root, mock_instance));
}

/// A selected project needs the workspace projects it depends on installed
/// too, so the install the gate spawns selects the dependencies of the
/// selected projects.
#[test]
fn filtered_exec_installs_the_workspace_dependencies_of_the_selected_projects() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "workspace-root", "version": "0.0.0" }).to_string(),
    )
    .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    let projects = [
        ("foo", json!({ "dependencies": { "bar": "workspace:*" } })),
        ("bar", json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } })),
        ("baz", json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } })),
    ];
    for (name, groups) in projects {
        let project = workspace.join("packages").join(name);
        fs::create_dir_all(&project).expect("create workspace project");
        write_named_manifest_with_dependency_groups(
            &project,
            name,
            &project.join("marker.txt"),
            groups,
        );
    }

    let output = pacquet_in(&workspace)
        .with_args(["--filter", "foo", "exec", "node", "-e", r#"process.stdout.write("foo-ok")"#])
        .output()
        .expect("spawn pacquet exec");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the filtered exec must succeed:\n{stderr}");
    assert!(
        workspace.join("packages/bar/node_modules/@pnpm.e2e/foo").exists(),
        "the filtered exec must install the workspace dependency of the selected project:\n{stderr}",
    );
    assert!(
        !workspace.join("packages/baz/node_modules").exists(),
        "the filtered exec must not install an unrelated project:\n{stderr}",
    );

    drop((root, mock_instance));
}

/// `--workspace-root` adds the root to a filtered selection. The install the
/// gate spawns runs from the workspace root and installs it as well.
#[test]
fn filtered_exec_with_workspace_root_installs_the_root() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_named_manifest_with_dependency_groups(
        &workspace,
        "workspace-root",
        &workspace.join("marker.txt"),
        json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
    );
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    for name in ["foo", "bar"] {
        let project = workspace.join("packages").join(name);
        fs::create_dir_all(&project).expect("create workspace project");
        write_named_manifest_with_dependency_groups(
            &project,
            name,
            &project.join("marker.txt"),
            json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
        );
    }

    let output = pacquet_in(&workspace)
        .with_args(["--filter", "foo", "--workspace-root", "exec", "node", "-e", "0"])
        .output()
        .expect("spawn pacquet exec");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the filtered exec must succeed:\n{stderr}");
    assert!(
        workspace.join("node_modules/@pnpm.e2e/foo").exists(),
        "the filtered exec must install the workspace root it selected:\n{stderr}",
    );
    assert!(
        workspace.join("packages/foo/node_modules/@pnpm.e2e/foo").exists(),
        "the filtered exec must install the project it selected:\n{stderr}",
    );
    assert!(
        !workspace.join("packages/bar/node_modules").exists(),
        "the filtered exec must not install a project it did not select:\n{stderr}",
    );

    drop((root, mock_instance));
}

/// A filtered install leaves the workspace dependencies of the projects it
/// selected without a modules directory too, but the install the gate spawns
/// for a filtered command selects them, so the status check has to hold them
/// to the modules-directory requirement as well
/// ([pnpm/tasks#45](https://github.com/pnpm/tasks/issues/45)).
#[test]
fn filtered_exec_installs_a_workspace_dependency_a_filtered_install_left_alone() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "workspace-root", "version": "0.0.0" }).to_string(),
    )
    .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    let projects = [
        ("foo", json!({ "dependencies": { "bar": "workspace:*" } })),
        ("bar", json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } })),
    ];
    for (name, groups) in projects {
        let project = workspace.join("packages").join(name);
        fs::create_dir_all(&project).expect("create workspace project");
        write_named_manifest_with_dependency_groups(
            &project,
            name,
            &project.join("marker.txt"),
            groups,
        );
    }

    // the filtered install selects `foo` alone, so its workspace dependency
    // `bar` has no modules directory
    pacquet_in(&workspace)
        .with_args(["--filter", "foo", "install"])
        .assert()
        .success();
    assert!(
        workspace.join("packages/foo/node_modules").exists(),
        "the filtered install must install the project it selected",
    );
    assert!(
        !workspace.join("packages/bar/node_modules").exists(),
        "the filtered install must not install the project it did not select",
    );

    // the next filtered exec must install the workspace dependency of the
    // project it selected instead of treating the recorded state as up to date
    let output = pacquet_in(&workspace)
        .with_args(["--filter", "foo", "exec", "node", "-e", r#"process.stdout.write("foo-ok")"#])
        .output()
        .expect("spawn pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the filtered exec must succeed:\n{stderr}");
    assert_eq!(stdout, "foo-ok");
    assert!(
        workspace.join("packages/bar/node_modules/@pnpm.e2e/foo").exists(),
        "the filtered exec must install the workspace dependency of the project it selected:\n{stderr}",
    );

    drop((root, mock_instance));
}

/// A negated selector reaches the install the gate spawns unchanged, so that
/// install never materializes the project it excludes. The status check must
/// not hold that project to the modules-directory requirement through the
/// workspace dependency edge of a selected project.
#[test]
fn filtered_exec_does_not_require_a_workspace_dependency_a_negated_selector_excludes() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "workspace-root", "version": "0.0.0" }).to_string(),
    )
    .expect("write root package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "verifyDepsBeforeRun: error\npackages:\n  - packages/*\n",
    )
    .expect("write pnpm-workspace.yaml");
    let projects = [
        ("foo", json!({ "dependencies": { "bar": "workspace:*" } })),
        ("bar", json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } })),
    ];
    for (name, groups) in projects {
        let project = workspace.join("packages").join(name);
        fs::create_dir_all(&project).expect("create workspace project");
        write_named_manifest_with_dependency_groups(
            &project,
            name,
            &project.join("marker.txt"),
            groups,
        );
    }

    pacquet_in(&workspace)
        .with_args(["--filter", "!bar", "install"])
        .assert()
        .success();
    assert!(
        !workspace.join("packages/bar/node_modules").exists(),
        "the filtered install must not install the project it excluded",
    );

    let output = pacquet_in(&workspace)
        .with_args(["--filter", "!bar", "exec", "node", "-e", r#"process.stdout.write("ok")"#])
        .output()
        .expect("spawn pacquet exec");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the filtered exec must pass the check:\n{stderr}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ok");

    drop((root, mock_instance));
}

#[test]
fn ndjson_exec_keeps_verifier_output_machine_readable() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &workspace.join("marker.txt"));

    let output = pacquet
        .with_args([
            "--reporter=ndjson",
            "exec",
            "node",
            "-e",
            r#"process.stdout.write(JSON.stringify({workspace:"test"}))"#,
        ])
        .output()
        .expect("spawn ndjson pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDOUT:\n{stdout}\n\nSTDERR:\n{stderr}\n");
    assert!(output.status.success(), "ndjson exec failed");
    assert_eq!(stdout, r#"{"workspace":"test"}"#);
    assert!(!stderr.is_empty(), "the verifier install must report NDJSON events");
    for line in stderr.lines() {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|err| panic!("invalid NDJSON line {line:?}: {err}"));
    }

    drop(root);
}

#[test]
fn silent_exec_suppresses_verifier_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &workspace.join("marker.txt"));

    let output = pacquet
        .with_args([
            "exec",
            "--silent",
            "node",
            "-e",
            r#"process.stdout.write(JSON.stringify({workspace:"test"}))"#,
        ])
        .output()
        .expect("spawn silent pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDOUT:\n{stdout}\n\nSTDERR:\n{stderr}\n");
    assert!(output.status.success(), "silent exec failed");
    assert_eq!(stdout, r#"{"workspace":"test"}"#);
    assert_eq!(stderr, "");

    drop(root);
}

#[test]
fn silent_recursive_exec_suppresses_verifier_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &workspace.join("marker.txt"));
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - .\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_args([
            "--silent",
            "--recursive",
            "exec",
            "node",
            "-e",
            r#"process.stdout.write(JSON.stringify({workspace:"test"}))"#,
        ])
        .output()
        .expect("spawn silent recursive pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDOUT:\n{stdout}\n\nSTDERR:\n{stderr}\n");
    assert!(output.status.success(), "silent recursive exec failed");
    assert_eq!(stdout, r#"{"workspace":"test"}"#);
    assert_eq!(stderr, "");

    drop(root);
}

#[test]
#[cfg_attr(not(unix), ignore = "the fixture script uses the POSIX `touch` command")]
fn silent_recursive_run_suppresses_verifier_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - .\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_args(["--silent", "--recursive", "run", "hello"])
        .output()
        .expect("spawn silent recursive pacquet run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDOUT:\n{stdout}\n\nSTDERR:\n{stderr}\n");
    assert!(output.status.success(), "silent recursive run failed");
    assert_eq!(stdout, "");
    assert_eq!(stderr, "");
    assert!(marker.exists(), "the script must run after the verifier install");

    drop(root);
}

#[test]
fn unreachable_lockfile_snapshots_do_not_trigger_reinstall_loop() {
    const PREPARE_MARKER: &str = "prepare-ran.txt";
    const ORPHANED: &str = "@pnpm.e2e/pkg-with-1-dep";
    const ORPHANED_HOISTED_LINK: &str =
        "node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep";

    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let marker = workspace.join(PREPARE_MARKER);
    let write_project = |dependencies: serde_json::Value| {
        fs::write(
            workspace.join("package.json"),
            json!({
                "name": "unreachable-snapshots-project",
                "version": "0.0.0",
                "scripts": {
                    "prepare": format!(
                        r#"node -e "require('fs').writeFileSync('{PREPARE_MARKER}', '')""#,
                    ),
                },
                "dependencies": dependencies,
            })
            .to_string(),
        )
        .expect("write the project manifest");
    };

    write_project(json!({
        "@pnpm.e2e/foo": "100.0.0",
        ORPHANED: "100.0.0",
    }));
    pacquet
        .with_arg("install")
        .assert()
        .success();

    // Drop the dependency from the manifest and from the lockfile's importer
    // without regenerating the lockfile, which is the state the issue reports:
    // the snapshots only that importer entry reached stay in `pnpm-lock.yaml`
    // with nothing referencing them.
    write_project(json!({ "@pnpm.e2e/foo": "100.0.0" }));
    let mut wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("read the wanted lockfile")
        .expect("the install wrote a wanted lockfile");
    let importer = wanted.importers.get_mut(".").expect("the root importer is in the lockfile");
    let orphaned = ORPHANED.parse().expect("the dependency name is valid");
    importer.dependencies
        .as_mut()
        .expect("the root importer has dependencies")
        .remove(&orphaned);
    wanted
        .save_to_path(&workspace.join("pnpm-lock.yaml"))
        .expect("save the wanted lockfile");
    bump_mtime(&workspace.join("package.json"));

    // The frozen install settles the tree on what the importers reach: it
    // materializes only that graph, and records the same graph as the current
    // lockfile.
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert!(
        !workspace
            .join("node_modules")
            .join(ORPHANED)
            .exists(),
        "the frozen install must unlink the dependency the importer no longer declares",
    );
    assert!(
        !workspace.join(ORPHANED_HOISTED_LINK).exists(),
        "the frozen install must unhoist a package only the unreachable snapshot reached",
    );
    let current = fs::read_to_string(workspace.join("node_modules/.pnpm/lock.yaml"))
        .expect("read the current lockfile");
    assert!(
        !current.contains(ORPHANED),
        "the current lockfile must record only what the importers reach, got:\n{current}",
    );

    // The wanted lockfile still carries the unreachable snapshots, so a second
    // frozen install faces the same input as the first. It has nothing left to
    // do: no re-import, and no lifecycle script rerun.
    fs::remove_file(&marker).expect("clear the prepare marker");
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert!(
        !marker.exists(),
        "a repeated frozen install must not re-materialize the tree and rerun `prepare`",
    );

    pacquet_in(&workspace)
        .with_args(["--config.verify-deps-before-run=error", "exec", "node", "-e", "0"])
        .assert()
        .success();

    drop((root, mock_instance));
}

/// `pnpm run` in a dependency-free project that the workspace patterns
/// leave out runs the script and writes nothing in that directory
/// ([pnpm/pnpm#16313](https://github.com/pnpm/pnpm/issues/16313)).
#[test]
fn run_in_a_dependency_free_project_the_workspace_leaves_out_writes_nothing() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write the root manifest");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - pkgs/*\n")
        .expect("write pnpm-workspace.yaml");
    let member = workspace.join("pkgs/a");
    fs::create_dir_all(&member).expect("create the workspace member");
    fs::write(member.join("package.json"), json!({ "name": "a", "version": "1.0.0" }).to_string())
        .expect("write the member manifest");
    let scripts = workspace.join("scripts");
    fs::create_dir_all(&scripts).expect("create the left-out project");
    let marker = scripts.join("ran.txt");
    fs::write(
        scripts.join("package.json"),
        json!({
            "scripts": {
                "hi": r#"node -e "require('fs').writeFileSync('ran.txt','ok')""#,
            },
        })
        .to_string(),
    )
    .expect("write the left-out manifest");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let output = pacquet_in(&scripts)
        .with_args(["run", "hi"])
        .output()
        .expect("run the script");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the script must run:\n{stderr}");
    assert_eq!(
        fs::read_to_string(&marker).unwrap_or_default(),
        "ok",
        "the script must write its marker",
    );
    assert!(
        !scripts.join("node_modules").exists(),
        "pnpm run must not install a project that declares no dependencies:\n{stderr}",
    );
    assert!(
        !scripts.join("pnpm-lock.yaml").exists(),
        "pnpm run must not write a lockfile for a project that declares no dependencies:\n{stderr}",
    );

    drop(root);
}

/// A left-out project whose only dependency is a required peer still
/// installs before pnpm run when auto-install-peers is on.
#[test]
fn run_in_a_left_out_project_with_a_required_peer_installs_it() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write the root manifest");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - pkgs/*\nautoInstallPeers: true\n",
    )
    .expect("write pnpm-workspace.yaml");
    let member = workspace.join("pkgs/a");
    fs::create_dir_all(&member).expect("create the workspace member");
    fs::write(member.join("package.json"), json!({ "name": "a", "version": "1.0.0" }).to_string())
        .expect("write the member manifest");
    fs::write(member.join("index.js"), "module.exports = 1\n").expect("write the member entry");
    let scripts = workspace.join("scripts");
    fs::create_dir_all(&scripts).expect("create the left-out project");
    let marker = scripts.join("ran.txt");
    fs::write(
        scripts.join("package.json"),
        json!({
            "name": "scripts",
            "peerDependencies": { "a": "file:../pkgs/a" },
            "scripts": {
                "hi": r#"node -e "require('a'); require('fs').writeFileSync('ran.txt','ok')""#,
            },
        })
        .to_string(),
    )
    .expect("write the left-out manifest");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let output = pacquet_in(&scripts)
        .with_args(["run", "hi"])
        .output()
        .expect("run the script");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the script must run:\n{stderr}");
    assert_eq!(
        fs::read_to_string(&marker).unwrap_or_default(),
        "ok",
        "the script must write its marker",
    );
    assert!(
        scripts.join("node_modules").exists(),
        "pnpm run must install a required peer:\n{stderr}",
    );
    assert!(
        scripts.join("pnpm-lock.yaml").exists(),
        "pnpm run must write a lockfile when it installs a required peer:\n{stderr}",
    );

    drop(root);
}

/// An optional peer is not fetched on its own, so pnpm run in a left-out
/// project that declares only that peer writes nothing.
#[test]
fn run_in_a_left_out_project_with_only_an_optional_peer_writes_nothing() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write the root manifest");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - pkgs/*\nautoInstallPeers: true\n",
    )
    .expect("write pnpm-workspace.yaml");
    let member = workspace.join("pkgs/a");
    fs::create_dir_all(&member).expect("create the workspace member");
    fs::write(member.join("package.json"), json!({ "name": "a", "version": "1.0.0" }).to_string())
        .expect("write the member manifest");
    let scripts = workspace.join("scripts");
    fs::create_dir_all(&scripts).expect("create the left-out project");
    let marker = scripts.join("ran.txt");
    fs::write(
        scripts.join("package.json"),
        json!({
            "peerDependencies": { "a": "1.0.0" },
            "peerDependenciesMeta": { "a": { "optional": true } },
            "scripts": {
                "hi": r#"node -e "require('fs').writeFileSync('ran.txt','ok')""#,
            },
        })
        .to_string(),
    )
    .expect("write the left-out manifest");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let output = pacquet_in(&scripts)
        .with_args(["run", "hi"])
        .output()
        .expect("run the script");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the script must run:\n{stderr}");
    assert_eq!(
        fs::read_to_string(&marker).unwrap_or_default(),
        "ok",
        "the script must write its marker",
    );
    assert!(
        !scripts.join("node_modules").exists(),
        "pnpm run must not install a project that declares only an optional peer:\n{stderr}",
    );
    assert!(
        !scripts.join("pnpm-lock.yaml").exists(),
        "pnpm run must not write a lockfile for a project that declares only an optional peer:\n{stderr}",
    );

    drop(root);
}

/// `pnpm run` in a never-installed project outside any workspace that has
/// nothing to install runs the script and writes nothing.
#[test]
fn run_in_a_project_with_nothing_to_install_writes_nothing() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_named_manifest_with_dependency_groups(&workspace, "scripts-only", &marker, json!({}));

    pacquet
        .with_args(["run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the script must run");
    assert!(!workspace.join("node_modules").exists(), "pnpm run must not install");
    assert!(!workspace.join("pnpm-lock.yaml").exists(), "pnpm run must not write a lockfile");

    drop(root);
}

/// An install lifecycle script is work for the install, so the first
/// `pnpm run` still installs and runs it.
#[cfg(unix)]
#[test]
fn run_installs_a_project_whose_only_install_work_is_a_lifecycle_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    let prepared = workspace.join("prepared.txt");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "prepare-only",
            "scripts": {
                "hello": format!(r#"touch "{}""#, marker.display()),
                "prepare": format!(r#"touch "{}""#, prepared.display()),
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet
        .with_args(["run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the script must run");
    assert!(prepared.exists(), "the gate must install and run the prepare script");

    drop(root);
}

/// With `ignoreScripts`, the install would not run a lifecycle script, so
/// the script alone does not start one.
#[test]
fn ignored_lifecycle_scripts_do_not_start_an_install() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "prepare-only", "scripts": { "prepare": "exit 1" } }).to_string(),
    )
    .expect("write package.json");

    pacquet
        .with_args(["--config.ignore-scripts=true", "exec", "node", "-e", "0"])
        .assert()
        .success();
    assert!(!workspace.join("node_modules").exists(), "the gate must not install");
    assert!(!workspace.join("pnpm-lock.yaml").exists(), "the gate must not write a lockfile");

    drop(root);
}

/// `pnpm:devPreinstall` runs only from the workspace root, so a member that
/// declares it gives the install nothing to do.
#[test]
fn a_member_dev_preinstall_does_not_start_an_install() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), json!({ "name": "root" }).to_string())
        .expect("write the root manifest");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - pkgs/*\n")
        .expect("write pnpm-workspace.yaml");
    let member = workspace.join("pkgs/a");
    fs::create_dir_all(&member).expect("create the workspace member");
    fs::write(
        member.join("package.json"),
        json!({ "name": "a", "scripts": { "pnpm:devPreinstall": "exit 1" } }).to_string(),
    )
    .expect("write the member manifest");

    pacquet
        .with_args(["exec", "node", "-e", "0"])
        .assert()
        .success();
    assert!(!workspace.join("node_modules").exists(), "the gate must not install");
    assert!(!workspace.join("pnpm-lock.yaml").exists(), "the gate must not write a lockfile");

    drop(root);
}

/// A pnpmfile's `readPackage` hook can add dependencies to a manifest that
/// declares none, so a pnpmfile counts as install work.
#[test]
fn a_pnpmfile_starts_an_install() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_named_manifest_with_dependency_groups(
        &workspace,
        "scripts-only",
        &workspace.join("marker.txt"),
        json!({}),
    );
    fs::write(workspace.join(".pnpmfile.cjs"), "module.exports = { hooks: {} }\n")
        .expect("write .pnpmfile.cjs");

    pacquet
        .with_args(["run", "hello"])
        .assert()
        .success();
    assert!(workspace.join("pnpm-lock.yaml").exists(), "the gate must install");

    drop(root);
}

/// Under separate lockfiles the install loads a pnpmfile from the project's
/// own lockfile directory, so one there counts as install work too.
#[test]
fn a_pnpmfile_beside_a_separate_lockfile_starts_an_install() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), json!({ "name": "root" }).to_string())
        .expect("write the root manifest");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - pkgs/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write pnpm-workspace.yaml");
    let member = workspace.join("pkgs/a");
    fs::create_dir_all(&member).expect("create the workspace member");
    write_named_manifest_with_dependency_groups(
        &member,
        "a",
        &member.join("marker.txt"),
        json!({}),
    );
    fs::write(member.join(".pnpmfile.cjs"), "module.exports = { hooks: {} }\n")
        .expect("write .pnpmfile.cjs");

    pacquet_in(&member)
        .with_args(["run", "hello"])
        .assert()
        .success();
    assert!(member.join("pnpm-lock.yaml").exists(), "the gate must install");

    drop(root);
}

/// A `lockfileDir` pinned away from the project leaves the importers to the
/// install, so the gate keeps it.
#[test]
fn a_pinned_lockfile_dir_starts_an_install() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let project = workspace.join("project");
    fs::create_dir_all(&project).expect("create the project");
    write_named_manifest_with_dependency_groups(
        &project,
        "scripts-only",
        &project.join("marker.txt"),
        json!({}),
    );

    let output = pacquet_in(&project)
        .with_args([&format!("--config.lockfile-dir={}", workspace.display()), "run", "hello"])
        .output()
        .expect("run the script");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the script must run:\n{stderr}");
    assert!(stderr.contains("Done in"), "the gate must install:\n{stderr}");
    assert!(workspace.join("pnpm-lock.yaml").exists());
    assert!(!project.join("pnpm-lock.yaml").exists());

    drop(root);
}
