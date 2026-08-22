//! The `lockfileDir` setting and its `--lockfile-dir` flag.
//!
//! Pinning the lockfile directory moves the whole shared layout with it:
//! `pnpm-lock.yaml`, the root `node_modules` holding the virtual store,
//! and the importer ids, which become the paths from the pin down to each
//! project. Every project keeps its own `node_modules` of symlinks.

use crate::_utils::{append_workspace_yaml_key, pacquet_in};

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::json;
use std::{fs, path::Path, process::Command};

/// Whether an install reported the project as already up to date. Read
/// off the NDJSON stream (which the reporter writes to stderr) because
/// the default reporter only prints a message whose prefix is the current
/// directory, and a pinned `lockfileDir` is by definition somewhere else.
fn reported_up_to_date(ndjson: &str) -> bool {
    ndjson.lines().any(|line| line.contains(r#""message":"Already up to date""#))
}

/// The `importers:` keys of the lockfile at `lockfile_dir`, sorted —
/// the parsed map is a `HashMap`, so only the serialized file is ordered.
fn importer_ids(lockfile_dir: &Path) -> Vec<String> {
    let mut ids: Vec<String> = pnpm_lockfile::Lockfile::load_wanted_from_dir(lockfile_dir)
        .expect("load pnpm-lock.yaml")
        .expect("pnpm-lock.yaml exists at the pinned lockfile dir")
        .importers
        .into_keys()
        .collect();
    ids.sort_unstable();
    ids
}

/// `--lockfile-dir` puts `pnpm-lock.yaml` and the virtual store in a
/// directory above the project, and names the project by its path from
/// there. The project still gets its own `node_modules` with the
/// dependency linked in. Ports pnpm's "install with external lockfile
/// directory".
#[test]
fn external_lockfile_dir_holds_the_lockfile_and_the_virtual_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let lockfile_dir = workspace.join("nested");
    let project_dir = lockfile_dir.join("project");
    fs::create_dir_all(&project_dir).expect("create the project dir");
    fs::write(project_dir.join("package.json"), r#"{"name":"project","version":"1.0.0"}"#)
        .expect("write package.json");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&project_dir)
        .with_args(["install", "is-positive@1.0.0", "--lockfile-dir", ".."])
        .assert()
        .success();

    assert_eq!(importer_ids(&lockfile_dir), ["project"]);
    assert!(
        !workspace.join("pnpm-lock.yaml").exists(),
        "no lockfile may be written at the workspace root the pin moved away from",
    );
    assert!(
        lockfile_dir.join("node_modules/.pnpm/is-positive@1.0.0").is_dir(),
        "the virtual store must live under the pinned lockfile dir",
    );
    assert!(
        project_dir.join("node_modules/is-positive/package.json").is_file(),
        "the project keeps its own node_modules of symlinks into the virtual store",
    );

    drop((root, mock_instance));
}

/// The `lockfileDir` setting is read from `pnpm-workspace.yaml` and
/// resolved against it, so a workspace can keep its lockfile one level up
/// without passing a flag on every command.
#[test]
fn lockfile_dir_setting_is_read_from_the_workspace_manifest() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), r#"{"name":"project","version":"1.0.0"}"#)
        .expect("write package.json");
    append_workspace_yaml_key(&workspace, "lockfileDir", "..");

    pacquet.with_args(["install", "is-positive@1.0.0"]).assert().success();

    let lockfile_dir = root.path();
    assert_eq!(importer_ids(lockfile_dir), ["workspace"]);
    assert!(
        !workspace.join("pnpm-lock.yaml").exists(),
        "no lockfile may be written at the workspace root the setting moved away from",
    );
    assert!(
        lockfile_dir.join("node_modules/.pnpm/is-positive@1.0.0").is_dir(),
        "the virtual store must live under the configured lockfile dir",
    );
    assert!(
        workspace.join("node_modules/is-positive/package.json").is_file(),
        "the project keeps its own node_modules of symlinks into the virtual store",
    );

    drop(mock_instance);
}

/// A global install owns the lockfile in its own group directory, so
/// pnpm refuses to let `--lockfile-dir` redirect it.
#[test]
fn lockfile_dir_conflicts_with_global() {
    let CommandTempCwd { pacquet, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let output = pacquet
        .with_args(["add", "--global", "is-positive@1.0.0", "--lockfile-dir", "."])
        .output()
        .expect("spawn pacquet add");

    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(!output.status.success(), "the conflicting flags must fail the command:\n{stderr}");
    assert!(
        stderr.contains("ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_DIR_WITH_GLOBAL"),
        "the error must carry pnpm's code:\n{stderr}",
    );

    drop((root, mock_instance));
}

/// Adopting `lockfileDir` after an install without it must not let the
/// repeat-install short-circuit answer for the old layout: the state and
/// lockfile the previous install left at the workspace root say "up to
/// date", but nothing has been written at the pin yet.
#[test]
fn adopting_lockfile_dir_re_installs_at_the_pin() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();
    assert!(workspace.join("pnpm-lock.yaml").is_file(), "the first install writes in place");

    append_workspace_yaml_key(&workspace, "lockfileDir", "..");
    let output = pacquet_in(&workspace)
        .with_args(["install", "--reporter=ndjson"])
        .output()
        .expect("spawn pacquet install");
    let ndjson = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the re-anchored install must succeed:\n{ndjson}");
    assert!(
        !reported_up_to_date(&ndjson),
        "the pin has no lockfile yet, so the install cannot report up to date:\n{ndjson}",
    );

    let lockfile_dir = root.path();
    assert_eq!(importer_ids(lockfile_dir), ["workspace"]);
    assert!(
        lockfile_dir.join("node_modules/.pnpm/is-positive@1.0.0").is_dir(),
        "the virtual store must be materialized under the pin",
    );

    drop(mock_instance);
}

/// A repeat install against a pinned lockfile reads the state the
/// previous one wrote at the pin, so it short-circuits — and the
/// `verifyDepsBeforeRun` gate reads the same state, so `pnpm run` finds
/// the dependencies current instead of reinstalling before every script.
#[cfg(unix)]
#[test]
fn a_pinned_install_is_current_for_repeat_installs_and_for_run() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let marker = workspace.join("marker.txt");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
            "scripts": { "hello": format!(r#"touch "{}""#, marker.display()) },
        })
        .to_string(),
    )
    .expect("write package.json");
    append_workspace_yaml_key(&workspace, "lockfileDir", "..");

    pacquet.with_arg("install").assert().success();

    let output = pacquet_in(&workspace)
        .with_args(["install", "--reporter=ndjson"])
        .output()
        .expect("spawn pacquet install");
    let ndjson = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the repeat install must succeed:\n{ndjson}");
    assert!(
        reported_up_to_date(&ndjson),
        "the repeat install must short-circuit on the state written at the pin:\n{ndjson}",
    );

    let output =
        pacquet_in(&workspace).with_args(["run", "hello"]).output().expect("spawn pacquet run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "the script must run:\n{stdout}");
    assert!(marker.is_file(), "the script must have run:\n{stdout}");
    assert!(
        !stdout.contains("Cannot check whether dependencies are outdated"),
        "the gate must find the state the pinned install wrote:\n{stdout}",
    );

    drop((root, mock_instance));
}

/// `sharedWorkspaceLockfile: false` asks for a lockfile per project, but
/// an explicit `lockfileDir` names the one directory they all share, so
/// the pin wins — as it does in pnpm, whose recursive dispatch routes any
/// run with a `lockfileDir` through its shared-lockfile branch.
#[test]
fn a_pin_overrides_dedicated_per_project_lockfiles() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_dir = workspace.join("packages/a");
    fs::create_dir_all(&package_dir).expect("create the workspace package dir");
    fs::write(
        package_dir.join("package.json"),
        json!({
            "name": "a",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write the package manifest");
    fs::write(workspace.join("package.json"), r#"{"name":"root","version":"1.0.0"}"#)
        .expect("write the root manifest");
    append_workspace_yaml_key(&workspace, "packages", "['packages/*']");
    append_workspace_yaml_key(&workspace, "sharedWorkspaceLockfile", false);
    append_workspace_yaml_key(&workspace, "lockfileDir", "..");

    pacquet.with_arg("install").assert().success();

    let lockfile_dir = root.path();
    assert_eq!(importer_ids(lockfile_dir), ["workspace", "workspace/packages/a"]);
    assert!(
        !package_dir.join("pnpm-lock.yaml").exists() && !workspace.join("pnpm-lock.yaml").exists(),
        "the pin replaces the per-project lockfiles and leaves none at the workspace root",
    );
    assert!(
        package_dir.join("node_modules/is-positive/package.json").is_file(),
        "the workspace package still links its dependency",
    );

    drop(mock_instance);
}
