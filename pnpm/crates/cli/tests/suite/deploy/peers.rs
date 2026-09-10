use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, Lockfile, PkgName, fs, pacquet_cmd,
    write_ambiguous_peer_workspace, write_peer_workspace, write_project,
};
use assert_cmd::assert::OutputAssertExt;

/// The workspace resolves `lib`'s peer from `lib`'s own devDependencies, which
/// a production deploy leaves behind. The deployed graph carries exactly one
/// resolution of that peer, so the synthesized snapshot can bind it.
#[test]
fn shared_lockfile_deploy_binds_a_singleton_peer_of_a_linked_workspace_package() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_workspace(&workspace);

    pacquet.with_arg("install").assert().success();
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    let lib_real = fs::canonicalize(deploy_dir.join("node_modules/lib")).unwrap();
    let peer = lib_real.parent().unwrap().join("@pnpm.e2e/peer-a");
    assert!(peer.exists(), "the deployed workspace package should resolve its peer");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(fs::canonicalize(&peer).unwrap().join("package.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["version"], "1.0.0");

    drop((root, mock_instance));
}

/// Deploy does not adjudicate peer ranges. Injecting the package binds the
/// consumer's version even when it falls outside the declared range — pnpm
/// treats that as a resolution-time warning — so the non-injected path binds
/// it too rather than inventing a stricter rule for linked packages.
#[test]
fn shared_lockfile_deploy_binds_a_singleton_peer_outside_the_declared_range() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_workspace(&workspace);
    write_project(
        &workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            // app pins 1.0.0, which does not satisfy this.
            "peerDependencies": { "@pnpm.e2e/peer-a": "1.0.1" },
        }),
    );

    pacquet.with_arg("install").assert().success();
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    let lib_real = fs::canonicalize(deploy_dir.join("node_modules/lib")).unwrap();
    let peer = lib_real.parent().unwrap().join("@pnpm.e2e/peer-a");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(fs::canonicalize(&peer).unwrap().join("package.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["version"], "1.0.0");

    drop((root, mock_instance));
}

#[test]
fn shared_lockfile_deploy_refuses_a_linked_workspace_package_with_an_ambiguous_peer() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_ambiguous_peer_workspace(&workspace);

    pacquet.with_arg("install").assert().success();
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    let output = pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The rendered wording is shared with the TypeScript CLI's
    // ERR_PNPM_DEPLOY_AMBIGUOUS_PEER; keep the two in step.
    for expected in [
        "ERR_PNPM_DEPLOY_AMBIGUOUS_PEER",
        "Workspace package 'lib' declares a peer dependency on '@pnpm.e2e/peer-a'",
        "more than one version (1.0.0, 1.0.1)",
        r#"Pin '@pnpm.e2e/peer-a' to a single version with an "overrides" entry"#,
    ] {
        assert!(stderr.contains(expected), "stderr should mention {expected}:\n{stderr}");
    }

    drop((root, mock_instance));
}

/// The remedy `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER` suggests: collapsing the peer to
/// one version makes the binding unambiguous, so the deploy goes through
/// without injecting the workspace or falling back to the legacy implementation.
#[test]
fn an_override_collapsing_the_peer_unblocks_a_non_injected_deploy() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_ambiguous_peer_workspace(&workspace);
    let mut workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap();
    workspace_yaml.push_str("overrides:\n  '@pnpm.e2e/peer-a': 1.0.0\n");
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml).unwrap();

    pacquet.with_arg("install").assert().success();
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    let lib_real = fs::canonicalize(deploy_dir.join("node_modules/lib")).unwrap();
    let peer = lib_real.parent().unwrap().join("@pnpm.e2e/peer-a");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(fs::canonicalize(&peer).unwrap().join("package.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["version"], "1.0.0");

    drop((root, mock_instance));
}

/// A peer that the package also declares as an optional dependency is already
/// bound. Re-binding it would copy it into the required map and quietly promote
/// it, changing what `--no-optional` and a failed fetch mean for it.
#[test]
fn shared_lockfile_deploy_keeps_an_optional_peer_optional() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_workspace(&workspace);
    write_project(
        &workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "peerDependencies": { "@pnpm.e2e/peer-a": "*" },
            "optionalDependencies": { "@pnpm.e2e/peer-a": "1.0.0" },
        }),
    );

    pacquet.with_arg("install").assert().success();
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    let deploy_lockfile = Lockfile::load_wanted_from_dir(&deploy_dir).unwrap().unwrap();
    let peer: PkgName = "@pnpm.e2e/peer-a".parse().unwrap();
    let lib = deploy_lockfile
        .snapshots
        .as_ref()
        .expect("deploy snapshots")
        .iter()
        .find(|(key, _)| key.name.to_string() == "lib")
        .map(|(_, snapshot)| snapshot)
        .expect("the deployed lib snapshot");
    assert!(
        lib.optional_dependencies.as_ref().is_some_and(|deps| deps.contains_key(&peer)),
        "the peer should stay in the optional map: {lib:#?}",
    );
    assert!(
        !lib.dependencies.as_ref().is_some_and(|deps| deps.contains_key(&peer)),
        "the peer should not also be copied into the required map: {lib:#?}",
    );

    drop((root, mock_instance));
}

/// `--no-optional` clears the optional map before the binding step, so the
/// binder cannot see that the peer was already bound by an optional edge.
/// Re-binding it there would resurrect a dependency the flag excluded.
#[test]
fn shared_lockfile_deploy_does_not_resurrect_an_excluded_optional_peer() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_workspace(&workspace);
    write_project(
        &workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "peerDependencies": { "@pnpm.e2e/peer-a": "*" },
            "optionalDependencies": { "@pnpm.e2e/peer-a": "1.0.0" },
        }),
    );

    pacquet.with_arg("install").assert().success();
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--no-optional"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    let deploy_lockfile = Lockfile::load_wanted_from_dir(&deploy_dir).unwrap().unwrap();
    let peer: PkgName = "@pnpm.e2e/peer-a".parse().unwrap();
    let lib = deploy_lockfile
        .snapshots
        .as_ref()
        .expect("deploy snapshots")
        .iter()
        .find(|(key, _)| key.name.to_string() == "lib")
        .map(|(_, snapshot)| snapshot)
        .expect("the deployed lib snapshot");
    assert!(
        !lib.dependencies.as_ref().is_some_and(|deps| deps.contains_key(&peer)),
        "an excluded optional peer must not come back as a required dependency: {lib:#?}",
    );

    drop((root, mock_instance));
}
