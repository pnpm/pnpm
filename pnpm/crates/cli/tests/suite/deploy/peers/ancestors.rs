//! Peers of linked workspace packages that their ancestors provide.

use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, Lockfile, PkgName, fs, pacquet_cmd,
    write_ambiguous_peer_workspace, write_peer_workspace, write_project,
};
use assert_cmd::assert::OutputAssertExt;
use std::path::Path;

/// `app` depends on the peer itself, so injecting `lib` would bind `app`'s
/// version, whichever other versions the deployed graph holds.
#[test]
fn shared_lockfile_deploy_binds_a_linked_peer_to_the_version_its_parent_provides() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_ambiguous_peer_workspace(&workspace);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    assert_eq!(dependency_version(&deploy_dir, "lib", "@pnpm.e2e/peer-a"), "1.0.0");
    assert_eq!(dependency_version(&deploy_dir, "other", "@pnpm.e2e/peer-a"), "1.0.1");

    drop((root, mock_instance));
}

/// `lib` declares `workspace:^` for its peer and lists the same as a dev
/// dependency, which the source lockfile records only in `lib`'s
/// devDependencies. `app` depends on the workspace project itself, and `other`
/// brings a registry copy into the deployed graph. `lib` binds to what `app`
/// provides, and `other` keeps its registry copy.
#[test]
fn shared_lockfile_deploy_binds_a_workspace_protocol_peer_to_the_workspace_project_its_parent_provides()
 {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_protocol_peer_workspace(&workspace);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let workspace_lockfile = Lockfile::load_wanted_from_dir(&workspace).unwrap().unwrap();
    let peer_a: PkgName = "@pnpm.e2e/peer-a".parse().unwrap();
    let lib_importer = &workspace_lockfile.importers["packages/lib"];
    assert!(
        lib_importer.dependencies
            .as_ref()
            .is_none_or(|dependencies| !dependencies.contains_key(&peer_a)),
        "the source lockfile should not record the peer as a prod dependency of lib",
    );
    let dev_version = &lib_importer.dev_dependencies.as_ref().unwrap()[&peer_a].version;
    assert_eq!(dev_version.to_string(), "link:../peer-a");
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    assert_eq!(dependency_version(&deploy_dir, "lib", "@pnpm.e2e/peer-a"), "2.0.0");
    assert_eq!(dependency_version(&deploy_dir, "other", "@pnpm.e2e/peer-a"), "1.0.1");

    drop((root, mock_instance));
}

/// `app` depends on a registry copy of the peer while `other` brings the
/// workspace project into the deployed graph. `lib`'s `workspace:` peer binds
/// to what its parent provides, as a direct dependency overrides the peer.
#[test]
fn shared_lockfile_deploy_binds_a_workspace_protocol_peer_to_the_registry_copy_its_parent_provides()
{
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_protocol_peer_workspace(&workspace);
    write_project(
        &workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "peerDependencies": { "@pnpm.e2e/peer-a": "workspace:^" },
        }),
    );
    write_project(
        &workspace,
        "other",
        &serde_json::json!({
            "name": "other",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "@pnpm.e2e/peer-a": "workspace:^" },
        }),
    );
    write_project(
        &workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": {
                "lib": "workspace:*",
                "other": "workspace:*",
                "@pnpm.e2e/peer-a": "1.0.0",
            },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    assert_eq!(dependency_version(&deploy_dir, "lib", "@pnpm.e2e/peer-a"), "1.0.0");
    assert_eq!(dependency_version(&deploy_dir, "other", "@pnpm.e2e/peer-a"), "2.0.0");

    drop((root, mock_instance));
}

/// `app` provides the peer under the alias `lib` declares it with, so the
/// binding keeps the alias and resolves to the project the alias names.
#[test]
fn shared_lockfile_deploy_binds_an_aliased_peer_to_what_its_parent_provides() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_protocol_peer_workspace(&workspace);
    write_project(
        &workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "peerDependencies": { "compat": "workspace:@pnpm.e2e/peer-a@*" },
            "devDependencies": { "compat": "workspace:@pnpm.e2e/peer-a@*" },
        }),
    );
    write_project(
        &workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": {
                "lib": "workspace:*",
                "other": "workspace:*",
                "compat": "workspace:@pnpm.e2e/peer-a@*",
            },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    assert_eq!(dependency_version(&deploy_dir, "lib", "compat"), "2.0.0");

    drop((root, mock_instance));
}

/// `mid` does not depend on `lib`'s peer, so the search continues to `app`,
/// which does.
#[test]
fn shared_lockfile_deploy_binds_a_linked_peer_to_what_an_ancestor_provides() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_protocol_peer_workspace(&workspace);
    write_project(
        &workspace,
        "mid",
        &serde_json::json!({
            "name": "mid",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "lib": "workspace:*" },
        }),
    );
    write_project(
        &workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": {
                "mid": "workspace:*",
                "other": "workspace:*",
                "@pnpm.e2e/peer-a": "workspace:^",
            },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    let lib_dir = fs::canonicalize(deploy_dir.join("node_modules/mid"))
        .unwrap()
        .parent()
        .unwrap()
        .join("lib");
    let peer_manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            fs::canonicalize(lib_dir)
                .unwrap()
                .parent()
                .unwrap()
                .join("@pnpm.e2e/peer-a/package.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(peer_manifest["version"], "2.0.0");

    drop((root, mock_instance));
}

/// `mid` and `mid-2` both provide `lib`'s peer, as two different packages at
/// the same version, so injecting `lib` under each would bind a different one.
#[test]
fn shared_lockfile_deploy_refuses_a_peer_its_parents_provide_as_different_packages() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_workspace(&workspace);
    for (name, peer) in [("mid", "1.0.0"), ("mid-2", "npm:@pnpm.e2e/peer-b@1.0.0")] {
        write_project(
            &workspace,
            name,
            &serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "files": ["index.js"],
                "dependencies": { "lib": "workspace:*", "@pnpm.e2e/peer-a": peer },
            }),
        );
    }
    write_project(
        &workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "mid": "workspace:*", "mid-2": "workspace:*" },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    let output = pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    for expected in ["ERR_PNPM_DEPLOY_AMBIGUOUS_PEER", "(1.0.0, @pnpm.e2e/peer-b@1.0.0)"] {
        assert!(stderr.contains(expected), "stderr should mention {expected}:\n{stderr}");
    }

    drop((root, mock_instance));
}

/// `consumer` declares `lib` as a peer, which `autoInstallPeers` records as
/// one of its dependencies. That does not make `consumer` a parent of `lib`, so
/// its own copy of `lib`'s peer does not compete with `app`'s.
#[test]
fn shared_lockfile_deploy_does_not_treat_a_peer_consumer_as_a_parent() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_workspace(&workspace);
    let workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .unwrap()
        .replace("autoInstallPeers: false", "autoInstallPeers: true");
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml).unwrap();
    write_project(
        &workspace,
        "consumer",
        &serde_json::json!({
            "name": "consumer",
            "version": "1.0.0",
            "files": ["index.js"],
            "peerDependencies": { "lib": "workspace:*" },
            "dependencies": { "@pnpm.e2e/peer-a": "1.0.1" },
        }),
    );
    write_project(
        &workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": {
                "lib": "workspace:*",
                "consumer": "workspace:*",
                "@pnpm.e2e/peer-a": "1.0.0",
            },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let workspace_lockfile = Lockfile::load_wanted_from_dir(&workspace).unwrap().unwrap();
    let lib: PkgName = "lib".parse().unwrap();
    assert!(
        workspace_lockfile.importers["packages/consumer"].dependencies
            .as_ref()
            .is_some_and(|dependencies| dependencies.contains_key(&lib)),
        "the source lockfile should record lib as a dependency of consumer",
    );
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    assert_eq!(dependency_version(&deploy_dir, "lib", "@pnpm.e2e/peer-a"), "1.0.0");

    drop((root, mock_instance));
}

/// Without a dev dependency on its peer, `autoInstallPeers` installs it as a
/// dependency of `lib`, which binds the peer before the deploy has to.
#[test]
fn shared_lockfile_deploy_keeps_an_auto_installed_workspace_protocol_peer() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_protocol_peer_workspace(&workspace);
    let workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .unwrap()
        .replace("autoInstallPeers: false", "autoInstallPeers: true");
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml).unwrap();
    write_project(
        &workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "peerDependencies": { "@pnpm.e2e/peer-a": "workspace:^" },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    assert_eq!(dependency_version(&deploy_dir, "lib", "@pnpm.e2e/peer-a"), "2.0.0");

    drop((root, mock_instance));
}

/// `app` depends on the workspace copy of `@pnpm.e2e/peer-a` and on `lib`,
/// whose peer and dev dependency on it are both `workspace:^`. `other`
/// depends on a registry copy.
fn write_workspace_protocol_peer_workspace(workspace: &Path) {
    write_ambiguous_peer_workspace(workspace);
    write_project(
        workspace,
        "peer-a",
        &serde_json::json!({
            "name": "@pnpm.e2e/peer-a",
            "version": "2.0.0",
            "files": ["index.js"],
        }),
    );
    write_project(
        workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "peerDependencies": { "@pnpm.e2e/peer-a": "workspace:^" },
            "devDependencies": { "@pnpm.e2e/peer-a": "workspace:^" },
        }),
    );
    write_project(
        workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": {
                "lib": "workspace:*",
                "other": "workspace:*",
                "@pnpm.e2e/peer-a": "workspace:^",
            },
        }),
    );
}

/// The version of the package the deployed `package` resolves as `dependency`.
fn dependency_version(deploy_dir: &Path, package: &str, dependency: &str) -> serde_json::Value {
    let package_real = fs::canonicalize(deploy_dir.join("node_modules").join(package)).unwrap();
    let resolved = package_real
        .parent()
        .unwrap()
        .join(dependency);
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(fs::canonicalize(&resolved).unwrap().join("package.json")).unwrap(),
    )
    .unwrap();
    manifest["version"].clone()
}

/// `consumer` lists `lib` as both a dependency and a peer, so the dependency
/// wins and `consumer` is `lib`'s parent, the only one.
#[test]
fn shared_lockfile_deploy_binds_to_a_parent_that_also_declares_the_linked_package_as_a_peer() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_workspace(&workspace);
    write_project(
        &workspace,
        "consumer",
        &serde_json::json!({
            "name": "consumer",
            "version": "1.0.0",
            "files": ["index.js"],
            "peerDependencies": { "lib": "workspace:*" },
            "dependencies": { "lib": "workspace:*", "@pnpm.e2e/peer-a": "1.0.1" },
        }),
    );
    write_project(
        &workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "consumer": "workspace:*", "@pnpm.e2e/peer-a": "1.0.0" },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    let lib_dir = fs::canonicalize(deploy_dir.join("node_modules/consumer"))
        .unwrap()
        .parent()
        .unwrap()
        .join("lib");
    let peer_manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            fs::canonicalize(lib_dir)
                .unwrap()
                .parent()
                .unwrap()
                .join("@pnpm.e2e/peer-a/package.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(peer_manifest["version"], "1.0.1");

    drop((root, mock_instance));
}
