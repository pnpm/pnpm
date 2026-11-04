use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path, process::Command};

#[test]
fn relative_file_peers_resolve_from_the_declaring_package() {
    for (peer_specifier, explicit_provider) in [
        (None, false),
        (Some("file:../b"), false),
        (Some("file:../b"), true),
        (Some("workspace:../b"), false),
    ] {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let base = workspace.join("peer-repro");
        write_fixture(&base, peer_specifier, explicit_provider);

        crate::_utils::pacquet_in(&base.join("examples"))
            .with_args([
                "install",
                "--offline",
                "--ignore-scripts",
                "--lockfile-only",
                "--no-frozen-lockfile",
                "--config.preferFrozenLockfile=false",
            ])
            .with_arg("--store-dir")
            .with_arg(root.path().join("store"))
            .assert()
            .success();

        crate::_utils::pacquet_in(&base.join("examples"))
            .with_args(["install", "--offline", "--ignore-scripts", "--frozen-lockfile"])
            .with_arg("--store-dir")
            .with_arg(root.path().join("store"))
            .assert()
            .success();

        Command::new("node")
            .args(["-p", "require('a')"])
            .current_dir(base.join("examples"))
            .assert()
            .success()
            .stdout("correct-local-b\n");
        drop((root, npmrc_info));
    }
}

fn write_fixture(base: &Path, peer_specifier: Option<&str>, explicit_provider: bool) {
    let mut consumer = serde_json::json!({
        "name": "consumer",
        "version": "1.0.0",
        "private": true,
        "dependencies": { "a": "workspace:../packages/a" }
    });
    if explicit_provider {
        consumer["dependencies"]["b"] = serde_json::json!("workspace:../packages/b");
    }
    write_manifest(base, "examples", consumer);
    let mut package_a = serde_json::json!({
        "name": "a",
        "version": "1.0.0",
        "dependencies": { "b": "workspace:../b" }
    });
    if let Some(peer_specifier) = peer_specifier {
        package_a["peerDependencies"] = serde_json::json!({ "b": peer_specifier });
    }
    write_manifest(base, "packages/a", package_a);
    write_manifest(
        base,
        "packages/b",
        serde_json::json!({ "name": "b", "version": "1.0.0", "main": "index.js" }),
    );
    fs::write(base.join("packages/a/index.js"), "module.exports = require('b');\n")
        .expect("write a entry point");
    fs::write(base.join("packages/b/index.js"), "module.exports = 'correct-local-b';\n")
        .expect("write b entry point");
    for dir in ["examples", "packages/a", "packages/b"] {
        fs::write(
            base.join(dir).join("pnpm-workspace.yaml"),
            "packages: []\nsharedWorkspaceLockfile: false\ninjectWorkspacePackages: true\nignoreScripts: true\npmOnFail: ignore\n",
        )
        .expect("write workspace config");
    }
}

fn write_manifest(base: &Path, dir: &str, manifest: serde_json::Value) {
    let dir = base.join(dir);
    fs::create_dir_all(&dir).expect("create package directory");
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package manifest");
}
