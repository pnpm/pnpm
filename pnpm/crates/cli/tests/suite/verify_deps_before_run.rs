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
    fs::{backdate_existing_files, bump_mtime},
};
use serde_json::json;
use std::{fs, path::Path};

fn write_manifest(workspace: &Path, marker: &Path) {
    write_named_manifest_with_dependency_groups(
        workspace,
        "verify-deps-project",
        marker,
        json!({}),
    );
}

#[cfg(unix)]
fn write_named_manifest(workspace: &Path, name: &str, marker: &Path) {
    write_named_manifest_with_dependency_groups(workspace, name, marker, json!({}));
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

    // Deleting pnpm-lock.yaml in a dependency-less project leaves no
    // current lockfile to stand in for it, so the check fails like
    // pnpm's RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND — and the pre-run check
    // must not recreate the file (pnpm's run path never restores the
    // lockfile; only the install command does).
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
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
