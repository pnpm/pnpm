use super::write_tarball;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use std::{fs, process::Command};

#[test]
fn local_tarball_peer_versions_are_checked_against_the_manifest() {
    for (overrides, version) in
        [(false, "1.0.0"), (false, "2.0.0"), (true, "1.0.0"), (true, "2.0.0")]
    {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        write_tarball(
            root.path(),
            "provider.tgz",
            &serde_json::json!({ "name": "@repro/provider", "version": version }),
        );
        write_tarball(
            root.path(),
            "consumer.tgz",
            &serde_json::json!({
                "name": "@repro/consumer", "version": "1.0.0",
                "peerDependencies": { "@repro/provider": "^1.0.0" },
            }),
        );
        let dependencies = serde_json::json!({
            "@repro/provider": "file:../provider.tgz",
            "@repro/consumer": "file:../consumer.tgz",
        });
        fs::write(
            workspace.join("package.json"),
            serde_json::json!({ "name": "app", "private": true, "dependencies": dependencies })
                .to_string(),
        )
        .expect("write app manifest");
        let mut config = serde_json::json!({
            "autoInstallPeers": false,
            "strictPeerDependencies": true,
            "storeDir": root.path().join("store"),
        });
        if overrides {
            config["overrides"] = dependencies;
        }
        fs::write(workspace.join("pnpm-workspace.yaml"), config.to_string())
            .expect("write workspace configuration");

        let install = pacquet
            .with_args(["install", "--offline", "--ignore-scripts", "--no-frozen-lockfile"])
            .output()
            .expect("install local tarballs");
        let check = Command::cargo_bin("pnpm")
            .expect("find pnpm")
            .without_ambient_pnpm_config()
            .with_current_dir(&workspace)
            .with_args(["peers", "check"])
            .output()
            .expect("check local tarball peers");
        for output in [install, check] {
            let stdout = String::from_utf8(output.stdout).expect("read stdout");
            let stderr = String::from_utf8(output.stderr).expect("read stderr");
            let combined = format!("{stdout}\n{stderr}");
            assert_eq!(
                output.status.success(),
                version == "1.0.0",
                "provider {version}, overrides {overrides}:\n{combined}",
            );
            if version == "2.0.0" {
                assert!(combined.contains("Installed: 2.0.0"), "{combined}");
                assert!(combined.contains("^1.0.0"), "{combined}");
            }
        }
    }
}

#[test]
fn inherited_and_own_tarball_providers_do_not_collapse() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_tarball(
        root.path(),
        "provider-a.tgz",
        &serde_json::json!({ "name": "@repro/provider", "version": "1.0.0" }),
    );
    write_tarball(
        root.path(),
        "provider-b.tgz",
        &serde_json::json!({ "name": "@repro/provider", "version": "1.0.0" }),
    );
    write_tarball(
        root.path(),
        "consumer.tgz",
        &serde_json::json!({
            "name": "@repro/consumer", "version": "1.0.0",
            "peerDependencies": { "@repro/provider": "^1.0.0" },
        }),
    );
    write_tarball(
        root.path(),
        "middle.tgz",
        &serde_json::json!({
            "name": "@repro/middle", "version": "1.0.0",
            "dependencies": {
                "@repro/consumer": "file:../consumer.tgz",
                "@repro/provider": "file:../provider-b.tgz",
            },
        }),
    );
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "app", "private": true,
            "dependencies": {
                "@repro/provider": "file:../provider-a.tgz",
                "@repro/middle": "file:../middle.tgz",
            },
        })
        .to_string(),
    )
    .expect("write app manifest");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        serde_json::json!({
            "autoInstallPeers": false,
            "strictPeerDependencies": true,
            "storeDir": root.path().join("store"),
        })
        .to_string(),
    )
    .expect("write workspace configuration");

    let install = pacquet
        .with_args(["install", "--offline", "--ignore-scripts", "--no-frozen-lockfile"])
        .output()
        .expect("install local tarballs");
    let stdout = String::from_utf8(install.stdout).expect("read stdout");
    let stderr = String::from_utf8(install.stderr).expect("read stderr");
    assert!(install.status.success(), "install failed:\n{stdout}\n{stderr}");

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains(
            "@repro/consumer@file:../consumer.tgz(@repro/provider@file:../provider-b.tgz)"
        ),
        "consumer peer dependency must bind to provider-b.tgz:\n{lockfile}",
    );
}
