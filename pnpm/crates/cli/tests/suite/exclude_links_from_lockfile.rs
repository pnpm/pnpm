//! End-to-end coverage for the `excludeLinksFromLockfile` setting.

use crate::_utils;
use _utils::append_workspace_yaml_key;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{
    Lockfile,
    PackageKey,
    PkgName,
    SnapshotDepRef,
};
use pnpm_testing_utils::{
    bin::{
        AddMockedRegistry,
        CommandTempCwd,
    },
    command_env::CommandTestExt,
    fs::is_symlink_or_junction,
};
use std::{
    fs,
    path::Path,
    process::Command,
    str::FromStr,
};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

#[test]
fn plain_range_workspace_link_is_materialized_when_excluded_from_lockfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");
    append_workspace_yaml_key(&workspace, "packages", "['packages/*']");
    append_workspace_yaml_key(&workspace, "excludeLinksFromLockfile", true);
    append_workspace_yaml_key(&workspace, "linkWorkspacePackages", true);
    write_project(
        &workspace,
        "packages/app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "workspace-dep": "^1.0.0" },
        }),
    );
    write_project(
        &workspace,
        "packages/workspace-dep",
        &serde_json::json!({ "name": "workspace-dep", "version": "1.0.0" }),
    );

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let lockfile_text =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    let lockfile: Lockfile = serde_saphyr::from_str(&lockfile_text).expect("parse pnpm-lock.yaml");
    let app = lockfile.importers.get("packages/app").expect("app importer");
    assert!(
        app.dependencies
            .as_ref()
            .is_none_or(|dependencies| {
                !dependencies.contains_key(&PkgName::parse("workspace-dep").unwrap())
            }),
        "the workspace link must stay out of the lockfile",
    );

    let app_modules = workspace.join("packages/app/node_modules");
    let link = app_modules.join("workspace-dep");
    assert!(is_symlink_or_junction(&link).unwrap());
    assert_eq!(
        link.canonicalize().unwrap(),
        workspace
            .join("packages/workspace-dep")
            .canonicalize()
            .unwrap(),
    );

    fs::remove_dir_all(&app_modules).expect("remove app node_modules");
    pacquet_at(&workspace)
        .with_arg("install")
        .with_arg("--frozen-lockfile")
        .assert()
        .success();
    assert!(is_symlink_or_junction(&link).unwrap());

    drop((root, mock_instance));
}

#[test]
fn excluded_dependency_groups_do_not_materialize_plain_range_workspace_links() {
    for (args, expected) in [
        (&["--prod"][..], [true, false, true]),
        (&["--dev"][..], [false, true, false]),
        (&["--no-optional"][..], [true, true, false]),
    ] {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;
        fs::write(
            workspace.join("package.json"),
            serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true })
                .to_string(),
        )
        .expect("write root package.json");
        append_workspace_yaml_key(&workspace, "packages", "['packages/*']");
        append_workspace_yaml_key(&workspace, "excludeLinksFromLockfile", true);
        append_workspace_yaml_key(&workspace, "linkWorkspacePackages", true);
        write_project(
            &workspace,
            "packages/app",
            &serde_json::json!({
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "prod-dep": "^1.0.0" },
                "devDependencies": { "dev-dep": "^1.0.0" },
                "optionalDependencies": { "optional-dep": "^1.0.0" },
            }),
        );
        for name in ["prod-dep", "dev-dep", "optional-dep"] {
            write_project(
                &workspace,
                &format!("packages/{name}"),
                &serde_json::json!({
                    "name": name,
                    "version": "1.0.0",
                    "bin": "bin.js",
                }),
            );
            fs::write(workspace.join(format!("packages/{name}/bin.js")), "").unwrap();
        }

        pacquet_at(&workspace)
            .with_arg("install")
            .assert()
            .success();
        pacquet_at(&workspace)
            .with_arg("install")
            .with_args(args)
            .assert()
            .success();
        for (name, should_exist) in
            ["prod-dep", "dev-dep", "optional-dep"].into_iter().zip(expected)
        {
            assert_eq!(
                workspace
                    .join("packages/app/node_modules")
                    .join(name)
                    .exists(),
                should_exist,
                "dependency {name} with arguments {args:?}",
            );
            assert_eq!(
                workspace
                    .join("packages/app/node_modules/.bin")
                    .join(name)
                    .exists(),
                should_exist,
                "bin of dependency {name} with arguments {args:?}",
            );
        }

        drop((root, mock_instance));
    }
}

/// The setting only keeps the machine-dependent path of an *external*
/// link out of the lockfile. A workspace-internal link resolving a peer
/// dependency is already stable across machines, so its peer suffix and
/// snapshot edge must come out exactly as they do with the setting off.
#[test]
fn workspace_internal_link_peer_is_unaffected_by_exclude_links_from_lockfile() {
    let mut with_setting = install_workspace_with_linked_peer(true);
    let without_setting = install_workspace_with_linked_peer(false);

    let snapshot_key = PackageKey::from_str(concat!(
        "@pnpm.e2e/abc@1.0.0",
        "(@pnpm.e2e/peer-a@packages+peer-a)",
        "(@pnpm.e2e/peer-b@1.0.0)",
        "(@pnpm.e2e/peer-c@1.0.0)",
    ))
    .expect("parse the expected snapshot key");
    let peer_name = PkgName::parse("@pnpm.e2e/peer-a").expect("parse the peer name");
    for (lockfile, exclude_links) in [(&with_setting, true), (&without_setting, false)] {
        let snapshots = lockfile.snapshots.as_ref().expect("the lockfile has snapshots");
        dbg!(snapshots.keys().collect::<Vec<_>>());
        let snapshot = snapshots
            .get(&snapshot_key)
            .unwrap_or_else(|| {
                panic!(
                    "the workspace link must keep its own path in the peer suffix with \
                 excludeLinksFromLockfile: {exclude_links}",
                )
            });
        assert_eq!(
            snapshot.dependencies
                .as_ref()
                .and_then(|deps| deps.get(&peer_name)),
            Some(&SnapshotDepRef::Link("packages/peer-a".to_string())),
            "the workspace link must not be remapped to the importer's node_modules with \
             excludeLinksFromLockfile: {exclude_links}",
        );
    }

    // Structural rather than textual: the invariant is that the setting
    // changes nothing else, not that the two files serialize identically.
    let settings = with_setting.settings.as_mut().expect("the lockfile records its settings");
    settings.exclude_links_from_lockfile = false;
    dbg!(&with_setting, &without_setting);
    assert_eq!(with_setting, without_setting, "only the recorded setting itself may differ");
}

/// Workspace whose `packages/app` depends on a registry package with
/// peer dependencies, one of which is provided by the sibling workspace
/// package `packages/peer-a`. Returns the resulting `pnpm-lock.yaml`.
fn install_workspace_with_linked_peer(exclude_links_from_lockfile: bool) -> Lockfile {
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

    pacquet
        .with_arg("install")
        .with_arg("--lockfile-only")
        .assert()
        .success();
    let text = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    let lockfile = serde_saphyr::from_str(&text).expect("parse pnpm-lock.yaml");

    drop((root, mock_instance));
    lockfile
}

fn write_project(workspace: &Path, relative_dir: &str, manifest: &serde_json::Value) {
    let project_dir = workspace.join(relative_dir);
    fs::create_dir_all(&project_dir).expect("create project directory");
    fs::write(project_dir.join("package.json"), manifest.to_string())
        .expect("write project manifest");
}
