use super::{run_peers, write_linked_chain_project};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, process::Command};

/// In a workspace with autoInstallPeers: false and
/// strictPeerDependencies: true, a linked workspace package's peer dependency
/// is satisfied by dependencies installed in the monorepo root.
#[test]
fn linked_workspace_package_peer_satisfied_by_workspace_root_is_not_reported_missing() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nautoInstallPeers: false\nstrictPeerDependencies: true\n",
    )
    .expect("write workspace manifest");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "devDependencies": { "@pnpm.e2e/foo": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write root manifest");
    write_linked_chain_project(
        &workspace,
        "lib",
        serde_json::json!({ "peerDependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
    );
    write_linked_chain_project(
        &workspace,
        "app",
        serde_json::json!({ "dependencies": { "lib": "workspace:*" } }),
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["peers", "check", "--lockfile-only"])
        .output()
        .expect("run pnpm peers check");
    assert!(
        output.status.success(),
        "peers check must succeed when root provides peer: {output:?}",
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("No peer dependency issues found"), "stdout:\n{stdout}");

    let issues = run_peers(&workspace, &["peers", "check", "--lockfile-only", "--json"]);
    for (_project, report) in issues.as_object().expect("issues map") {
        assert_eq!(report["missing"].as_object().map(serde_json::Map::len), Some(0));
        assert_eq!(report["bad"].as_object().map(serde_json::Map::len), Some(0));
    }

    drop((root, mock_instance));
}

#[test]
fn linked_workspace_package_peer_incompatible_in_workspace_root_is_reported_unmet() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nautoInstallPeers: false\n",
    )
    .expect("write workspace manifest");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "devDependencies": { "@pnpm.e2e/foo": "2.0.0" },
        })
        .to_string(),
    )
    .expect("write root manifest");
    write_linked_chain_project(
        &workspace,
        "lib",
        serde_json::json!({ "peerDependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
    );
    write_linked_chain_project(
        &workspace,
        "app",
        serde_json::json!({ "dependencies": { "lib": "workspace:*" } }),
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["peers", "check", "--lockfile-only"])
        .output()
        .expect("run pnpm peers check");
    assert_eq!(
        output.status.code(),
        Some(1),
        "incompatible root peer must be reported: {output:?}",
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("✕ unmet peer @pnpm.e2e/foo"), "stdout:\n{stdout}");
    assert!(stdout.contains("Installed: 2.0.0"), "stdout:\n{stdout}");

    drop((root, mock_instance));
}

#[test]
fn linked_package_peer_not_resolved_from_root_when_setting_is_disabled() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nautoInstallPeers: false\nresolvePeersFromWorkspaceRoot: false\n",
    )
    .expect("write workspace manifest");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "devDependencies": { "@pnpm.e2e/foo": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write root manifest");
    write_linked_chain_project(
        &workspace,
        "lib",
        serde_json::json!({ "peerDependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
    );
    write_linked_chain_project(
        &workspace,
        "app",
        serde_json::json!({ "dependencies": { "lib": "workspace:*" } }),
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["peers", "check", "--lockfile-only"])
        .output()
        .expect("run pnpm peers check");
    assert_eq!(
        output.status.code(),
        Some(1),
        "root peer must not resolve when disabled: {output:?}",
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("✕ missing peer @pnpm.e2e/foo"), "stdout:\n{stdout}");

    drop((root, mock_instance));
}

#[test]
fn workspace_peer_reuse_settings_matrix() {
    use crate::_utils::{
        ManifestDeps, WorkspaceFixture, assert_success, ndjson_records, read_v11_bundle_path,
    };

    let bundle = read_v11_bundle_path();
    for use_v11 in [false, true] {
        if use_v11 && bundle.is_none() {
            continue;
        }
        for auto_install in [false, true] {
            for dedupe in [false, true] {
                for from_root in [false, true] {
                    for provider_location in ["root", "sibling", "transitive"] {
                        let root_provider = provider_location == "root";
                        let fixture = WorkspaceFixture::new();
                        fixture.append_workspace_yaml(&format!(
                            "autoInstallPeers: {auto_install}\ndedupePeerDependents: {dedupe}\nresolvePeersFromWorkspaceRoot: {from_root}\n",
                        ));
                        let providers = ManifestDeps {
                            prod: &[("@pnpm.e2e/peer-a", "1.0.0"), ("@pnpm.e2e/peer-c", "1.0.0")],
                            ..ManifestDeps::default()
                        };
                        fixture.write_root_manifest(
                            "root",
                            if root_provider { providers } else { ManifestDeps::default() },
                        );
                        if !root_provider {
                            if provider_location == "transitive" {
                                crate::_utils::write_project_manifest(
                                    &fixture.workspace.join("carrier"),
                                    "carrier",
                                    providers,
                                );
                                fixture.project(
                                    "provider",
                                    "provider",
                                    ManifestDeps {
                                        prod: &[("carrier", "file:../../carrier")],
                                        ..ManifestDeps::default()
                                    },
                                );
                            } else {
                                fixture.project("provider", "provider", providers);
                            }
                        }
                        fixture.project(
                            "consumer",
                            "consumer",
                            ManifestDeps {
                                prod: &[("@pnpm.e2e/abc-optional-peers", "1.0.0")],
                                ..ManifestDeps::default()
                            },
                        );
                        let run = |args: &[&str]| {
                            if use_v11 {
                                fixture.v11_command_at(
                                    bundle.as_ref().unwrap(),
                                    &fixture.workspace,
                                    args,
                                )
                            } else {
                                fixture.command_at(&fixture.workspace, args)
                            }
                        };
                        let output = run(&["install", "--lockfile-only"]);
                        assert_success(&output);
                        let lock_path = fixture.workspace.join("pnpm-lock.yaml");
                        let fresh = fs::read_to_string(&lock_path).unwrap();
                        let lock: serde_json::Value = serde_saphyr::from_str(&fresh).unwrap();
                        let version = lock["importers"]["packages/consumer"]["dependencies"]
                            ["@pnpm.e2e/abc-optional-peers"]["version"]
                            .as_str()
                            .unwrap();
                        let optional_resolved =
                            auto_install || dedupe || (root_provider && from_root);
                        assert_eq!(
                            version.contains("peer-c@1.0.0"),
                            optional_resolved,
                            "v11={use_v11}, auto={auto_install}, dedupe={dedupe}, root={root_provider}, root scope={from_root}: {version}"
                        );
                        assert_eq!(version.contains("peer-a@"), optional_resolved);
                        let issues = ndjson_records(&output)
                            .into_iter()
                            .find(|record| record["name"] == "pnpm:peer-dependency-issues");
                        let missing = issues
                            .as_ref()
                            .and_then(|record| {
                                record["issuesByProjects"]["packages/consumer"]["missing"]
                                    .as_object()
                            });
                        assert_eq!(
                            missing.is_some_and(|peers| peers.contains_key("@pnpm.e2e/peer-a")),
                            !optional_resolved
                        );

                        assert_success(&run(&["install", "--lockfile-only", "--frozen-lockfile"]));
                        assert_eq!(fs::read_to_string(&lock_path).unwrap(), fresh);
                    }
                }
            }
        }
    }
}

#[test]
fn reused_required_peer_executable_availability() {
    use crate::_utils::{ManifestDeps, WorkspaceFixture, assert_success, read_v11_bundle_path};

    let bundle = read_v11_bundle_path();
    for use_v11 in [false, true] {
        if use_v11 && bundle.is_none() {
            continue;
        }
        let fixture = WorkspaceFixture::new();
        fixture.append_workspace_yaml("autoInstallPeers: false\ndedupePeerDependents: true\n");
        fixture.write_root_manifest("root", ManifestDeps::default());
        fixture.project(
            "provider",
            "provider",
            ManifestDeps {
                prod: &[("@pnpm.e2e/peer-with-bin", "1.0.0")],
                ..ManifestDeps::default()
            },
        );
        let consumer = fixture.project(
            "consumer",
            "consumer",
            ManifestDeps {
                prod: &[("@pnpm.e2e/pkg-with-peer-having-bin", "1.0.0")],
                ..ManifestDeps::default()
            },
        );
        let output = if use_v11 {
            fixture.v11_command_at(bundle.as_ref().unwrap(), &fixture.workspace, ["install"])
        } else {
            fixture.command_at(&fixture.workspace, ["install"])
        };
        assert_success(&output);
        let lock = fs::read_to_string(fixture.workspace.join("pnpm-lock.yaml")).unwrap();
        assert!(
            lock.contains("(peer-with-bin@") || lock.contains("(@pnpm.e2e/peer-with-bin@"),
            "{lock}"
        );
        let bin = if cfg!(windows) { "peer-with-bin.cmd" } else { "peer-with-bin" };
        // Peer reuse currently exposes the executable at the importer only in v11.
        assert_eq!(
            consumer
                .join("node_modules/.bin")
                .join(bin)
                .exists(),
            use_v11,
            "v11={use_v11}"
        );
        assert!(
            fixture.workspace
                .join("packages/provider/node_modules/.bin")
                .join(bin)
                .exists()
        );
    }
}
