//! End-to-end coverage for the `excludeLinksFromLockfile` setting.

pub mod _utils;

use _utils::append_workspace_yaml_key;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

/// The setting only keeps the machine-dependent path of an *external*
/// link out of the lockfile. A workspace-internal link resolving a peer
/// dependency is already stable across machines, so its peer suffix and
/// snapshot edge must come out exactly as they do with the setting off.
#[test]
fn workspace_internal_link_peer_is_unaffected_by_exclude_links_from_lockfile() {
    let with_setting = install_workspace_with_linked_peer(true);
    let without_setting = install_workspace_with_linked_peer(false);

    let snapshot_key = concat!(
        "@pnpm.e2e/abc@1.0.0",
        "(@pnpm.e2e/peer-a@packages+peer-a)",
        "(@pnpm.e2e/peer-b@1.0.0)",
        "(@pnpm.e2e/peer-c@1.0.0)",
    );
    for (lockfile, exclude_links) in [(&with_setting, true), (&without_setting, false)] {
        assert!(
            lockfile.contains(snapshot_key),
            "the workspace link must keep its own path in the peer suffix with \
             excludeLinksFromLockfile: {exclude_links}\n{lockfile}",
        );
        assert!(
            !lockfile.contains("node_modules+peer-a"),
            "the workspace link must not be remapped to the importer's node_modules with \
             excludeLinksFromLockfile: {exclude_links}\n{lockfile}",
        );
    }
    let with_setting_normalized =
        with_setting.replace("excludeLinksFromLockfile: true", "excludeLinksFromLockfile: false");
    eprintln!("WITH SETTING:\n{with_setting_normalized}\n");
    eprintln!("WITHOUT SETTING:\n{without_setting}\n");
    assert_eq!(
        with_setting_normalized, without_setting,
        "only the recorded setting itself may differ",
    );
}

/// Workspace whose `packages/app` depends on a registry package with
/// peer dependencies, one of which is provided by the sibling workspace
/// package `packages/peer-a`. Returns the resulting `pnpm-lock.yaml`.
fn install_workspace_with_linked_peer(exclude_links_from_lockfile: bool) -> String {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");

    append_workspace_yaml_key(&workspace, "packages", "['packages/*']");
    append_workspace_yaml_key(&workspace, "excludeLinksFromLockfile", exclude_links_from_lockfile);

    write_project(
        &workspace,
        "packages/app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/abc": "1.0.0",
                "@pnpm.e2e/peer-a": "workspace:*",
                "@pnpm.e2e/peer-b": "1.0.0",
                "@pnpm.e2e/peer-c": "1.0.0",
            },
        }),
    );
    write_project(
        &workspace,
        "packages/peer-a",
        &serde_json::json!({ "name": "@pnpm.e2e/peer-a", "version": "1.0.0" }),
    );

    pacquet.with_arg("install").with_arg("--lockfile-only").assert().success();
    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");

    drop((root, mock_instance));
    lockfile
}

fn write_project(workspace: &Path, relative_dir: &str, manifest: &serde_json::Value) {
    let project_dir = workspace.join(relative_dir);
    fs::create_dir_all(&project_dir).expect("create project directory");
    fs::write(project_dir.join("package.json"), manifest.to_string())
        .expect("write project manifest");
}
