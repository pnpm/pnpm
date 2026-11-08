//! The repeat-install fast path deciding from the workspace state's manifest
//! records, without reading the project manifests whose recorded stat still
//! holds.

pub use _utils::*;

use crate::_utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

#[cfg(target_os = "linux")]
mod opens;

/// The debug event the fast path logs when the records decided.
pub(crate) const DECIDED_FROM_RECORDS: &str = "the recorded manifests decided the install";

/// The span that reads the project manifests.
const READ_PROJECTS_SPAN: &str = "discover_workspace_projects";

/// A workspace whose `a` depends on `b` through `workspace:*` and links its
/// command, beside a `packages/*` directory that holds no project.
fn write_workspace(workspace: &Path) {
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("packages:\n  - packages/*\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");
    let write_manifest = |dir: &str, manifest: serde_json::Value| {
        fs::create_dir_all(workspace.join(dir)).expect("create the project directory");
        fs::write(workspace.join(dir).join("package.json"), manifest.to_string())
            .expect("write package.json");
    };
    write_manifest(".", serde_json::json!({ "name": "root", "private": true }));
    write_manifest(
        "packages/a",
        serde_json::json!({ "name": "a", "version": "1.0.0", "dependencies": { "b": "workspace:*" } }),
    );
    write_manifest(
        "packages/b",
        serde_json::json!({ "name": "b", "version": "1.0.0", "bin": { "b": "cli.js" } }),
    );
    fs::write(workspace.join("packages/b/cli.js"), "#!/usr/bin/env node\n").expect("write cli.js");
    fs::create_dir_all(workspace.join("packages/no-project")).expect("create packages/no-project");
}

/// On a filesystem that keeps whole seconds, a stat recorded within two
/// seconds of the write is not trusted. There, wait past that window, so the
/// next install records the stats trusted.
fn wait_past_whole_second_rounding(workspace: &Path) {
    let modified = fs::metadata(workspace.join("pnpm-lock.yaml"))
        .expect("stat the lockfile")
        .modified()
        .expect("read the mtime");
    let whole_seconds = modified
        .duration_since(std::time::UNIX_EPOCH)
        .expect("an mtime after the epoch")
        .subsec_nanos()
        == 0;
    if whole_seconds {
        std::thread::sleep(std::time::Duration::from_millis(2_100));
    }
}

/// Install once more, past the rounding window when the filesystem keeps
/// whole seconds. The first install can record the lockfile in the tick it
/// wrote it, and the next install then rehashes it and writes the state. This
/// one takes that write, so the state is settled and its records trusted.
pub(crate) fn trust_recorded_stats(workspace: &Path, registry: &str) {
    wait_past_whole_second_rounding(workspace);
    pacquet_in(workspace)
        .with_env("PNPM_CONFIG_REGISTRY", registry)
        .with_arg("install")
        .assert()
        .success();
}

/// `pnpm install` with the fast path's trace, returning its output.
pub(crate) fn traced_install(workspace: &Path, registry: &str) -> String {
    let output = pacquet_in(workspace)
        .with_env("PNPM_CONFIG_REGISTRY", registry)
        .with_env("TRACE", "pacquet::install=debug")
        .with_arg("install")
        .assert()
        .success()
        .get_output()
        .clone();
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn an_unchanged_workspace_is_up_to_date_from_the_records() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());

    let output = traced_install(&workspace, mock_instance.url());

    assert!(output.contains("Already up to date"), "{output}");
    assert!(output.contains(DECIDED_FROM_RECORDS), "{output}");
    assert!(!output.contains(READ_PROJECTS_SPAN), "{output}");
    drop((root, mock_instance));
}

/// A state whose records are missing, such as one an older pnpm wrote, gets
/// them from the first check that reads the manifests.
#[test]
fn a_check_on_the_files_records_missing_manifest_records() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    remove_manifest_records(&workspace);
    wait_past_whole_second_rounding(&workspace);

    let from_files = traced_install(&workspace, mock_instance.url());
    let from_records = traced_install(&workspace, mock_instance.url());

    assert!(!from_files.contains(DECIDED_FROM_RECORDS), "{from_files}");
    assert!(from_records.contains(DECIDED_FROM_RECORDS), "{from_records}");
    drop((root, mock_instance));
}

/// An edit the lockfile still satisfies leaves the install up to date, but
/// the edited manifest has to be read to tell.
#[test]
fn an_edited_manifest_is_read_again() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    fs::write(
        workspace.join("packages/b/package.json"),
        serde_json::json!({
            "name": "b",
            "version": "1.0.0",
            "description": "edited",
            "bin": { "b": "cli.js" },
        })
        .to_string(),
    )
    .expect("edit packages/b/package.json");

    let output = traced_install(&workspace, mock_instance.url());

    assert!(output.contains("Already up to date"), "{output}");
    assert!(!output.contains(DECIDED_FROM_RECORDS), "{output}");
    drop((root, mock_instance));
}

#[test]
fn a_new_project_is_read_and_installed() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    fs::write(
        workspace.join("packages/no-project/package.json"),
        serde_json::json!({ "name": "c", "version": "1.0.0", "dependencies": { "b": "workspace:*" } })
            .to_string(),
    )
    .expect("write packages/no-project/package.json");

    let output = traced_install(&workspace, mock_instance.url());

    assert!(!output.contains(DECIDED_FROM_RECORDS), "{output}");
    assert!(workspace.join("packages/no-project/node_modules/b").exists(), "{output}");
    drop((root, mock_instance));
}

/// The records stand in for the dependencies the link check reads, so a
/// removed link of an unchanged project is still found.
#[test]
fn a_removed_workspace_link_is_relinked() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());
    let link = workspace.join("packages/a/node_modules/b");
    pnpm_fs::remove_dirent(&link).expect("remove the workspace link");

    let output = traced_install(&workspace, mock_instance.url());

    assert!(!output.contains(DECIDED_FROM_RECORDS), "{output}");
    assert!(output.contains("the recorded manifests cannot decide"), "{output}");
    assert!(output.contains("a direct dependency link is missing"), "{output}");
    assert!(link.exists(), "{output}");
    drop((root, mock_instance));
}

/// Strip the manifest records from the workspace state, as an older pnpm
/// would have written it.
fn remove_manifest_records(workspace: &Path) {
    let state_path = workspace.join("node_modules/.pnpm-workspace-state-v1.json");
    let mut state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("read the state"))
            .expect("parse the state");
    state["validation"]
        .as_object_mut()
        .expect("a validation record")
        .remove("manifestFiles")
        .expect("manifest records");
    fs::write(&state_path, state.to_string()).expect("write the state");
}

fn state_text(workspace: &Path) -> String {
    fs::read_to_string(workspace.join("node_modules/.pnpm-workspace-state-v1.json"))
        .expect("read the state")
}

/// A record of a manifest dated in the future is never trusted, so writing it
/// would only repeat the write on the next run.
#[test]
fn a_future_dated_manifest_does_not_rewrite_the_state() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());
    pnpm_testing_utils::fs::set_mtime(
        &workspace.join("packages/b/package.json"),
        std::time::SystemTime::now() + std::time::Duration::from_hours(1),
    );
    let before = state_text(&workspace);

    let output = traced_install(&workspace, mock_instance.url());

    assert!(output.contains("Already up to date"), "{output}");
    assert_eq!(state_text(&workspace), before);
    drop((root, mock_instance));
}

/// The run gate leaves inputs such as local file dependencies unchecked, so
/// only an install refreshes the manifest records.
#[test]
fn the_run_gate_leaves_missing_records_to_the_install() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("verifyDepsBeforeRun: install\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true, "scripts": { "noop": "node -e 0" } })
            .to_string(),
    )
    .expect("write package.json");
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());
    remove_manifest_records(&workspace);

    pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["run", "noop"])
        .assert()
        .success();
    assert!(!state_text(&workspace).contains("manifestFiles"), "the run gate refreshed records");
    wait_past_whole_second_rounding(&workspace);

    let from_files = traced_install(&workspace, mock_instance.url());
    let from_records = traced_install(&workspace, mock_instance.url());
    assert!(!from_files.contains(DECIDED_FROM_RECORDS), "{from_files}");
    assert!(from_records.contains(DECIDED_FROM_RECORDS), "{from_records}");
    drop((root, mock_instance));
}

/// The debug event the verify-deps gate logs when the records decided.
const GATE_DECIDED_FROM_RECORDS: &str = "the recorded manifests decided the dependencies";

/// [`write_workspace`] with a `noop` root script that fails the run when the
/// gate finds the dependencies outdated.
fn write_gated_workspace(workspace: &Path) {
    write_workspace(workspace);
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("verifyDepsBeforeRun: error\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true, "scripts": { "noop": "node -e 0" } })
            .to_string(),
    )
    .expect("write package.json");
}

/// The span that lists the workspace projects for the records attempt.
const LIST_PROJECTS_SPAN: &str = "list_workspace_projects";

/// `pnpm run noop` with the gate's trace and the listing span.
fn traced_run(workspace: &Path, registry: &str) -> std::process::Output {
    pacquet_in(workspace)
        .with_env("PNPM_CONFIG_REGISTRY", registry)
        .with_env("TRACE", "pacquet::run=debug,pacquet::install=debug")
        .with_args(["run", "noop"])
        .output()
        .expect("run pnpm run")
}

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn the_run_gate_finds_an_unchanged_workspace_up_to_date_from_the_records() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_gated_workspace(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());

    let output = traced_run(&workspace, mock_instance.url());

    let text = combined_output(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains(GATE_DECIDED_FROM_RECORDS), "{text}");
    drop((root, mock_instance));
}

/// The gate reports what the manifest files say, so an edit the lockfile does
/// not satisfy is read and reported.
#[test]
fn the_run_gate_reads_an_edited_manifest_and_reports_it_outdated() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_gated_workspace(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());
    fs::write(
        workspace.join("packages/b/package.json"),
        serde_json::json!({
            "name": "b",
            "version": "1.0.0",
            "bin": { "b": "cli.js" },
            "dependencies": { "a": "workspace:*" },
        })
        .to_string(),
    )
    .expect("edit packages/b/package.json");

    let output = traced_run(&workspace, mock_instance.url());

    let text = combined_output(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN"), "{text}");
    assert!(text.contains("Workspace package b has dependencies"), "{text}");
    assert!(text.contains(LIST_PROJECTS_SPAN), "the gate tried the records first: {text}");
    assert!(!text.contains(GATE_DECIDED_FROM_RECORDS), "{text}");
    drop((root, mock_instance));
}

/// The debug event of a fast path taken once the config setup has run.
const TAKEN_AFTER_CONFIG: &str = "repeat-install fast path taken after the config setup";

/// [`write_workspace`] with a pnpmfile, which keeps the fast path from
/// running before the config setup.
fn write_workspace_with_pnpmfile(workspace: &Path) {
    write_workspace(workspace);
    fs::write(workspace.join(".pnpmfile.cjs"), "module.exports = { hooks: {} }\n")
        .expect("write .pnpmfile.cjs");
}

#[test]
fn a_workspace_with_a_pnpmfile_is_up_to_date_from_the_records() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_pnpmfile(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());

    let output = traced_install(&workspace, mock_instance.url());

    assert!(output.contains("Already up to date"), "{output}");
    assert!(output.contains(TAKEN_AFTER_CONFIG), "{output}");
    assert!(output.contains(DECIDED_FROM_RECORDS), "{output}");
    drop((root, mock_instance));
}

/// An edit the records cannot stand in for leaves the decision to the
/// install, which reads the manifests as it always did.
#[test]
fn a_workspace_with_a_pnpmfile_reads_an_edited_manifest() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_pnpmfile(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());
    let edited = serde_json::json!({
        "name": "b",
        "version": "1.0.0",
        "bin": { "b": "cli.js" },
        "description": "edited",
    });
    fs::write(workspace.join("packages/b/package.json"), edited.to_string())
        .expect("edit packages/b/package.json");

    let output = traced_install(&workspace, mock_instance.url());

    assert!(output.contains("Already up to date"), "{output}");
    assert!(
        output.contains("repeat-install fast path not taken after the config setup"),
        "{output}",
    );
    assert!(!output.contains(TAKEN_AFTER_CONFIG), "{output}");
    drop((root, mock_instance));
}

/// A filter leaves the selection to the install, so a filter that matches no
/// project still fails under `--fail-if-no-match`.
#[test]
fn a_filtered_install_with_a_pnpmfile_still_fails_on_no_match() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_pnpmfile(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());

    pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--filter", "no-such-project", "--fail-if-no-match"])
        .assert()
        .failure();
    drop((root, mock_instance));
}

/// CI freezes a non-empty lockfile by default, and a frozen install never
/// short-circuits, so its validation still runs.
#[test]
fn a_ci_install_with_a_pnpmfile_is_not_decided_from_the_records() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_pnpmfile(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());

    let output = pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_env("TRACE", "pacquet::install=debug")
        .with_env("CI", "true")
        // The test runner pins the `ci` setting off.
        .with_env("PNPM_CONFIG_CI", "true")
        .with_arg("install")
        .output()
        .expect("run pnpm install");

    let text = combined_output(&output);
    assert!(output.status.success(), "{text}");
    assert!(!text.contains(TAKEN_AFTER_CONFIG), "{text}");
    drop((root, mock_instance));
}

/// A malformed declaration installs only while a `readPackage` hook repairs
/// it. The records keep it as written, so they leave it to the install,
/// which fails once the hook no longer repairs it.
#[test]
fn a_malformed_declaration_is_left_to_the_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    fs::write(
        workspace.join("packages/b/package.json"),
        r#"{"name":"b","version":"1.0.0","bin":{"b":"cli.js"},"dependencies":{"x":1,"c":"workspace:*"}}"#,
    )
    .expect("write packages/b/package.json");
    fs::create_dir_all(workspace.join("packages/c")).expect("create packages/c");
    fs::write(workspace.join("packages/c/package.json"), r#"{"name":"c","version":"1.0.0"}"#)
        .expect("write packages/c/package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { readPackage (pkg) {\n  if (process.env.REPAIR && pkg.dependencies && pkg.dependencies.x === 1) delete pkg.dependencies.x\n  return pkg\n} } }\n",
    )
    .expect("write .pnpmfile.cjs");
    let install = |repair: bool| {
        let mut command = pacquet_in(&workspace);
        command.env("PNPM_CONFIG_REGISTRY", mock_instance.url());
        if repair {
            command.env("REPAIR", "1");
        }
        command
            .arg("install")
            .output()
            .expect("run pnpm install")
    };
    assert!(install(true).status.success());
    wait_past_whole_second_rounding(&workspace);
    assert!(install(true).status.success());

    let output = install(false);

    assert!(!output.status.success(), "{}", combined_output(&output));
    drop((root, mock_instance));
}

/// `--workspace-packages` replaces the manifest's patterns, so a project
/// only the flag's patterns reach is a project of the install.
#[test]
fn an_install_with_workspace_package_patterns_finds_a_new_project() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    fs::create_dir_all(workspace.join("extra")).expect("create extra");
    let install = || {
        pacquet_in(&workspace)
            .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
            .with_args(["--workspace-packages", "packages/*", "--workspace-packages", "extra/*"])
            .with_arg("install")
            .assert()
            .success();
    };
    install();
    fs::create_dir_all(workspace.join("extra/new")).expect("create extra/new");
    fs::write(
        workspace.join("extra/new/package.json"),
        serde_json::json!({ "name": "new", "version": "1.0.0", "dependencies": { "b": "workspace:*" } })
            .to_string(),
    )
    .expect("write extra/new/package.json");

    install();

    assert!(workspace.join("extra/new/node_modules/b").exists());
    drop((root, mock_instance));
}

/// Config dependencies, like a pnpmfile, keep the fast path from running
/// before the config setup, but not from deciding without the manifests
/// after it.
#[test]
fn a_workspace_with_config_dependencies_is_up_to_date_from_the_records() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("configDependencies:\n  '@pnpm.e2e/foo': 100.0.0\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());

    let output = traced_install(&workspace, mock_instance.url());

    assert!(output.contains("Already up to date"), "{output}");
    assert!(output.contains(TAKEN_AFTER_CONFIG), "{output}");
    assert!(output.contains(DECIDED_FROM_RECORDS), "{output}");
    assert!(workspace.join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json").exists());
    drop((root, mock_instance));
}

/// The config setup already warns about the root manifest, so the fast path
/// after it does not warn again.
#[test]
fn an_install_with_a_pnpmfile_warns_about_the_root_manifest_once() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_pnpmfile(&workspace);
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true, "pnpm": { "overrides": {} } })
            .to_string(),
    )
    .expect("write package.json");
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());

    let output = traced_install(&workspace, mock_instance.url());

    assert!(output.contains(TAKEN_AFTER_CONFIG), "{output}");
    assert_eq!(output.matches("is no longer read by pnpm").count(), 1, "{output}");
    drop((root, mock_instance));
}

/// `--no-trust-lockfile` overrides the configured `trustLockfile` only in
/// the install's lockfile options, so neither fast path may decide.
#[test]
fn a_trust_lockfile_flag_that_overrides_the_setting_runs_the_install() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_pnpmfile(&workspace);
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("trustLockfile: true\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());

    let output = pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_env("TRACE", "pacquet::install=debug")
        .with_args(["install", "--no-trust-lockfile"])
        .output()
        .expect("run pnpm install");

    let text = combined_output(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("a flag overrides trustLockfile"), "{text}");
    assert!(!text.contains(TAKEN_AFTER_CONFIG), "{text}");
    assert!(text.contains("phase complete"), "the install ran its phases: {text}");
    drop((root, mock_instance));
}

/// The debug event of a fast path taken before the config setup.
const TAKEN_BEFORE_SETUP: &str = "repeat-install fast path taken before the install setup";

/// [`write_workspace`] with a lockfile per project.
fn write_workspace_with_project_lockfiles(workspace: &Path) {
    write_workspace(workspace);
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("sharedWorkspaceLockfile: false\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");
}

#[test]
fn a_workspace_with_project_lockfiles_is_up_to_date_from_the_records() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_project_lockfiles(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());

    let output = traced_install(&workspace, mock_instance.url());

    assert!(output.contains(TAKEN_BEFORE_SETUP), "{output}");
    assert_eq!(output.matches("Already up to date").count(), 1, "{output}");
    drop((root, mock_instance));
}

/// An edit the records cannot stand in for leaves the workspace to the
/// install's run per project.
#[test]
fn a_workspace_with_project_lockfiles_reads_an_edited_manifest() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_project_lockfiles(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());
    fs::write(
        workspace.join("packages/a/package.json"),
        serde_json::json!({ "name": "a", "version": "1.0.0" }).to_string(),
    )
    .expect("edit packages/a/package.json");

    let output = traced_install(&workspace, mock_instance.url());

    assert!(!output.contains(TAKEN_BEFORE_SETUP), "{output}");
    assert!(!workspace.join("packages/a/node_modules/b").exists(), "{output}");
    drop((root, mock_instance));
}

/// An injected copy follows its source in a workspace with a lockfile per
/// project, the up-to-date check sends the install to the projects whose
/// injected source may have changed.
#[test]
fn a_workspace_with_project_lockfiles_refreshes_an_injected_copy() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_project_lockfiles(&workspace);
    fs::write(
        workspace.join("packages/a/package.json"),
        serde_json::json!({
            "name": "a",
            "version": "1.0.0",
            "dependencies": { "b": "workspace:*" },
            "dependenciesMeta": { "b": { "injected": true } },
        })
        .to_string(),
    )
    .expect("write packages/a/package.json");
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());
    let injected = workspace.join("packages/a/node_modules/b/cli.js");
    assert_eq!(
        fs::read_to_string(&injected).expect("read the injected copy"),
        "#!/usr/bin/env node\n",
    );
    fs::write(workspace.join("packages/b/cli.js"), "edited\n").expect("edit packages/b/cli.js");

    let output = traced_install(&workspace, mock_instance.url());

    assert!(!output.contains(TAKEN_BEFORE_SETUP), "{output}");
    assert_eq!(
        fs::read_to_string(&injected).expect("read the injected copy"),
        "edited\n",
        "{output}",
    );
    drop((root, mock_instance));
}

#[test]
fn a_workspace_with_project_lockfiles_installs_a_new_project() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_project_lockfiles(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());
    fs::write(
        workspace.join("packages/no-project/package.json"),
        serde_json::json!({ "name": "c", "version": "1.0.0", "dependencies": { "b": "workspace:*" } })
            .to_string(),
    )
    .expect("write packages/no-project/package.json");

    let output = traced_install(&workspace, mock_instance.url());

    assert!(!output.contains(TAKEN_BEFORE_SETUP), "{output}");
    assert!(workspace.join("packages/no-project/node_modules/b").exists(), "{output}");
    drop((root, mock_instance));
}

/// The install verifies the patches against every project lockfile, so a
/// patch only a removed project used still fails the install.
#[test]
fn a_workspace_with_project_lockfiles_verifies_patches_after_a_project_is_removed() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_project_lockfiles(&workspace);
    fs::create_dir_all(workspace.join("patches")).expect("create patches");
    fs::write(
        workspace.join("patches/is-positive@1.0.0.patch"),
        crate::repeat_install::IS_POSITIVE_PATCH,
    )
    .expect("write the patch");
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("patchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");
    fs::create_dir_all(workspace.join("packages/patched")).expect("create packages/patched");
    fs::write(
        workspace.join("packages/patched/package.json"),
        serde_json::json!({ "name": "patched", "version": "1.0.0", "dependencies": { "is-positive": "1.0.0" } })
            .to_string(),
    )
    .expect("write packages/patched/package.json");
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());
    fs::remove_dir_all(workspace.join("packages/patched")).expect("remove packages/patched");

    let output = pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .output()
        .expect("run pnpm install");

    let text = combined_output(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ERR_PNPM_UNUSED_PATCH"), "{text}");
    drop((root, mock_instance));
}

/// [`write_workspace_with_project_lockfiles`] with a gate that fails a
/// command on outdated dependencies, and a project `c` that also links `b`.
fn write_gated_workspace_with_project_lockfiles(workspace: &Path) {
    write_workspace_with_project_lockfiles(workspace);
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("verifyDepsBeforeRun: error\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");
    fs::create_dir_all(workspace.join("packages/c")).expect("create packages/c");
    fs::write(
        workspace.join("packages/c/package.json"),
        serde_json::json!({ "name": "c", "version": "1.0.0", "dependencies": { "b": "workspace:*" } })
            .to_string(),
    )
    .expect("write packages/c/package.json");
}

fn recursive_exec(workspace: &Path, registry: &str) -> std::process::Output {
    pacquet_in(workspace)
        .with_env("PNPM_CONFIG_REGISTRY", registry)
        .with_args(["-r", "exec", "node", "-e", "0"])
        .output()
        .expect("run pnpm -r exec")
}

#[test]
fn a_recursive_exec_gate_with_project_lockfiles_passes_an_up_to_date_workspace() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_gated_workspace_with_project_lockfiles(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());

    let output = recursive_exec(&workspace, mock_instance.url());

    assert!(output.status.success(), "{}", combined_output(&output));
    drop((root, mock_instance));
}

#[test]
fn a_recursive_exec_gate_with_project_lockfiles_reports_an_outdated_project() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_gated_workspace_with_project_lockfiles(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());
    fs::write(
        workspace.join("packages/c/package.json"),
        serde_json::json!({ "name": "c", "version": "1.0.0", "dependencies": { "a": "workspace:*" } })
            .to_string(),
    )
    .expect("edit packages/c/package.json");

    let output = recursive_exec(&workspace, mock_instance.url());

    let text = combined_output(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN"), "{text}");
    drop((root, mock_instance));
}
