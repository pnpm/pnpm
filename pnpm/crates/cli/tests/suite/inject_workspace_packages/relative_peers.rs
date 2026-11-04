use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{fs, process::Command};

#[test]
fn relative_path_peer_is_auto_installed_from_its_declaring_directory() {
    struct Case {
        also_dependency: bool,
        explicit_provider: bool,
        peer_specifier: &'static str,
    }
    for Case {
        also_dependency,
        explicit_provider,
        peer_specifier,
    } in [
        Case {
            also_dependency: false,
            explicit_provider: false,
            peer_specifier: "workspace:../provider",
        },
        Case {
            also_dependency: true,
            explicit_provider: false,
            peer_specifier: "workspace:../provider",
        },
        Case {
            also_dependency: true,
            explicit_provider: true,
            peer_specifier: "workspace:../provider",
        },
        Case {
            also_dependency: true,
            explicit_provider: false,
            peer_specifier: r"workspace:..\provider",
        },
        Case {
            also_dependency: false,
            explicit_provider: false,
            peer_specifier: "file:../provider",
        },
        Case {
            also_dependency: true,
            explicit_provider: false,
            peer_specifier: "file:../provider",
        },
        Case { also_dependency: true, explicit_provider: true, peer_specifier: "file:../provider" },
        Case {
            also_dependency: true,
            explicit_provider: false,
            peer_specifier: r"file:..\provider",
        },
    ] {
        let CommandTempCwd { mut pacquet, root, workspace, .. } = CommandTempCwd::init();
        let mut library = json!({
            "name": "library", "version": "1.0.0",
            "peerDependencies": { "provider": peer_specifier },
        });
        if also_dependency {
            library["dependencies"] = json!({ "provider": "workspace:../provider" });
        }
        let mut consumer_manifest = json!({
            "name": "consumer", "version": "1.0.0", "dependencies": {
                "library": "workspace:../packages/library",
            },
        });
        if explicit_provider {
            consumer_manifest["dependencies"]["provider"] = json!("workspace:../packages/provider");
        }
        for (dir, manifest, source) in [
            ("examples", consumer_manifest, "module.exports = require('library');"),
            ("packages/library", library, "module.exports = require('provider');"),
            (
                "packages/provider",
                json!({ "name": "provider", "version": "1.0.0" }),
                "module.exports = 'local-provider';",
            ),
        ] {
            let project = workspace.join(dir);
            fs::create_dir_all(&project).unwrap();
            fs::write(project.join("package.json"), manifest.to_string()).unwrap();
            fs::write(project.join("index.js"), source).unwrap();
            fs::write(
            project.join("pnpm-workspace.yaml"),
            "packages: []\nsharedWorkspaceLockfile: false\ninjectWorkspacePackages: true\nignoreScripts: true\n",
        )
        .unwrap();
        }
        let consumer = workspace.join("examples");
        pacquet
            .current_dir(&consumer)
            .args(["install", "--offline", "--ignore-scripts", "--store-dir"])
            .arg(root.path().join("store"));
        pacquet
            .args([
                "--lockfile-only",
                "--no-frozen-lockfile",
                "--config.preferFrozenLockfile=false",
            ])
            .assert()
            .success();
        let lockfile_path = consumer.join("pnpm-lock.yaml");
        let lockfile = fs::read_to_string(&lockfile_path).unwrap();
        assert!(lockfile.contains(peer_specifier), "{lockfile}");
        super::pnpm_in(&consumer)
            .with_args([
                "install",
                "--offline",
                "--ignore-scripts",
                "--frozen-lockfile",
                "--store-dir",
            ])
            .with_arg(root.path().join("store"))
            .assert()
            .success();
        Command::new("node")
            .with_current_dir(&consumer)
            .with_args(["-p", "require('library')"])
            .assert()
            .success()
            .stdout("local-provider\n");
        assert_eq!(fs::read_to_string(lockfile_path).unwrap(), lockfile);
    }
}

#[test]
fn relative_path_peer_uses_each_importers_directory_when_discovery_is_cached() {
    for (shared_lockfile, peer_specifier) in [
        (true, "workspace:../provider"),
        (false, "workspace:../provider"),
        (true, "file:../provider"),
        (false, "file:../provider"),
    ] {
        let CommandTempCwd { mut pacquet, root, workspace, .. } = CommandTempCwd::init();
        fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - 'packages/*'\n  - 'apps/*'\n  - 'apps/nested/*'\nsharedWorkspaceLockfile: {shared_lockfile}\ninjectWorkspacePackages: true\ndedupeInjectedDeps: false\nresolvePeersFromWorkspaceRoot: false\nignoreScripts: true\n",
        ),
    )
    .unwrap();
        for (dir, manifest, source) in [
            (
                "packages/provider",
                json!({ "name": "provider", "version": "1.0.0" }),
                "module.exports = 'local-provider';",
            ),
            (
                "packages/library",
                json!({ "name": "library", "version": "1.0.0", "peerDependencies": {
                "provider": peer_specifier,
            } }),
                "module.exports = require('provider');",
            ),
            (
                "packages/bridge",
                json!({ "name": "bridge", "version": "1.0.0", "dependencies": {
                "library": "workspace:../library",
            } }),
                "module.exports = require('library');",
            ),
            (
                "apps/first",
                json!({ "name": "first", "version": "1.0.0", "dependencies": {
                "bridge": "workspace:*", "library": "workspace:*",
            } }),
                "module.exports = require('bridge');",
            ),
            (
                "apps/nested/second",
                json!({ "name": "second", "version": "1.0.0", "dependencies": {
                "bridge": "workspace:*", "library": "workspace:*",
            } }),
                "module.exports = require('bridge');",
            ),
        ] {
            let project = workspace.join(dir);
            fs::create_dir_all(&project).unwrap();
            fs::write(project.join("package.json"), manifest.to_string()).unwrap();
            fs::write(project.join("index.js"), source).unwrap();
        }
        pacquet
            .args(["install", "--offline", "--store-dir"])
            .arg(root.path().join("store"));
        pacquet.assert().success();
        pacquet
            .with_arg("--frozen-lockfile")
            .assert()
            .success();
        for consumer in ["apps/first", "apps/nested/second"] {
            for dependency in ["bridge", "library"] {
                Command::new("node")
                    .with_current_dir(workspace.join(consumer))
                    .with_args(["-p", &format!("require('{dependency}')")])
                    .assert()
                    .success()
                    .stdout("local-provider\n");
            }
        }
    }
}
