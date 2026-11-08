//! The manifest files an up-to-date command opens, counted through
//! [`record_opens`].

use super::{
    GATE_DECIDED_FROM_RECORDS, combined_output, traced_run, trust_recorded_stats,
    write_gated_workspace, write_gated_workspace_with_project_lockfiles, write_workspace,
    write_workspace_with_pnpmfile, write_workspace_with_project_lockfiles,
};
use crate::_utils::{inotify::record_opens, pacquet_in};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// How many times `paths` are opened while `run` runs.
fn count_opens(paths: &[PathBuf], run: impl FnOnce()) -> usize {
    record_opens(paths, run).len()
}

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

/// A pnpmfile keeps the fast path from running before the config setup, but
/// not from deciding without the manifests after it.
#[test]
fn an_up_to_date_install_with_a_pnpmfile_opens_no_project_manifest() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_pnpmfile(&workspace);
    install(&workspace, mock_instance.url());
    trust_recorded_stats(&workspace, mock_instance.url());

    let opens =
        count_opens(&project_manifests(&workspace), || install(&workspace, mock_instance.url()));

    assert_eq!(opens, 0);
    drop((root, mock_instance));
}

#[test]
fn an_up_to_date_install_with_project_lockfiles_opens_no_project_manifest() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_project_lockfiles(&workspace);
    install(&workspace, mock_instance.url());
    trust_recorded_stats(&workspace, mock_instance.url());

    let opens =
        count_opens(&project_manifests(&workspace), || install(&workspace, mock_instance.url()));

    assert_eq!(opens, 0);
    drop((root, mock_instance));
}

/// The gates of a recursive command share one discovery of the workspace
/// projects. The gates add two reads of `a`'s manifest to the command's own,
/// that discovery and `a`'s own gate, however many projects link workspace
/// commands.
#[test]
fn a_recursive_exec_with_project_lockfiles_discovers_the_projects_once() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_gated_workspace_with_project_lockfiles(&workspace);
    install(&workspace, mock_instance.url());

    let opens = count_opens(&[workspace.join("packages/a/package.json")], || {
        pacquet_in(&workspace)
            .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
            .with_args(["-r", "exec", "true"])
            .assert()
            .success();
    });

    assert!((3..=4).contains(&opens), "packages/a/package.json was opened {opens} times");
    drop((root, mock_instance));
}

/// A gate that finds its project out of date leaves the next gate to
/// discover the workspace projects again, since an install in between may
/// have changed their manifests.
#[test]
fn a_recursive_exec_with_project_lockfiles_rediscovers_after_an_outdated_gate() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_gated_workspace_with_project_lockfiles(&workspace);
    install(&workspace, mock_instance.url());
    let outdated = serde_json::json!({
        "name": "b",
        "version": "1.0.0",
        "bin": { "b": "cli.js" },
        "peerDependencies": { "is-positive": "1.0.0" },
    });
    fs::write(workspace.join("packages/b/package.json"), outdated.to_string())
        .expect("edit packages/b/package.json");

    let opens = count_opens(&[workspace.join("packages/a/package.json")], || {
        pacquet_in(&workspace)
            .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
            .with_env("PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN", "warn")
            .with_args(["-r", "exec", "true"])
            .assert()
            .success();
    });

    assert!(opens >= 5, "packages/a/package.json was opened {opens} times");
    drop((root, mock_instance));
}
