//! The manifest files an up-to-date command opens, counted through inotify.
//! No filesystem seam reaches the project discovery and the checks, so the
//! kernel counts the opens instead.

use super::{
    GATE_DECIDED_FROM_RECORDS, combined_output, traced_run, trust_recorded_stats,
    write_gated_workspace, write_workspace,
};
use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    open_counter::count_opens,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The manifests of the projects other than the workspace root, which the
/// command runs in and reads anyway.
fn project_manifests(workspace: &Path) -> Vec<PathBuf> {
    ["packages/a", "packages/b"]
        .map(|dir| workspace.join(dir).join("package.json"))
        .to_vec()
}

fn install(workspace: &Path, registry: &str) {
    pacquet_in(workspace)
        .with_env("PNPM_CONFIG_REGISTRY", registry)
        .with_arg("install")
        .assert()
        .success();
}

#[test]
fn an_up_to_date_install_opens_no_project_manifest() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    install(&workspace, mock_instance.url());
    trust_recorded_stats(&workspace, mock_instance.url());

    let opens =
        count_opens(&project_manifests(&workspace), || install(&workspace, mock_instance.url()));

    assert_eq!(opens, 0);
    drop((root, mock_instance));
}

/// The control for the count: an edit sends the install to the manifest
/// files, which opens the unchanged ones too.
#[test]
fn an_install_after_an_edit_opens_the_project_manifests() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    install(&workspace, mock_instance.url());
    trust_recorded_stats(&workspace, mock_instance.url());
    let edited = serde_json::json!({
        "name": "b",
        "version": "1.0.0",
        "bin": { "b": "cli.js" },
        "description": "edited",
    });
    fs::write(workspace.join("packages/b/package.json"), edited.to_string())
        .expect("edit packages/b/package.json");

    let opens = count_opens(&[workspace.join("packages/a/package.json")], || {
        install(&workspace, mock_instance.url());
    });

    assert!(opens > 0, "the unchanged manifest was not read");
    drop((root, mock_instance));
}

#[test]
fn an_up_to_date_gated_run_opens_no_project_manifest() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_gated_workspace(&workspace);
    install(&workspace, mock_instance.url());
    trust_recorded_stats(&workspace, mock_instance.url());

    let mut output = None;
    let opens = count_opens(&project_manifests(&workspace), || {
        output = Some(traced_run(&workspace, mock_instance.url()));
    });

    let output = output.expect("the run ran");
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(opens, 0);
    drop((root, mock_instance));
}

/// A recursive command reads the projects to run in, but its gate still
/// decides from the records.
#[test]
fn a_recursive_exec_gate_decides_from_the_records() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_gated_workspace(&workspace);
    install(&workspace, mock_instance.url());
    trust_recorded_stats(&workspace, mock_instance.url());

    let output = pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_env("TRACE", "pacquet::run=debug")
        .with_args(["-r", "exec", "node", "-e", "0"])
        .output()
        .expect("run pnpm -r exec");

    let text = combined_output(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains(GATE_DECIDED_FROM_RECORDS), "{text}");
    drop((root, mock_instance));
}
