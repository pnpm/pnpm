use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, Lockfile, dangling_links, deploy_graph_keys,
    deploy_optional_edges, fs, pacquet_cmd, virtual_store_entries, write_project, write_workspace,
};
use assert_cmd::assert::OutputAssertExt;

#[test]
fn production_deploy_does_not_require_dev_only_workspace_sources() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);

    pacquet.with_arg("install").assert().success();
    fs::remove_dir_all(workspace.join("packages/dev-only")).unwrap();

    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy"])
        .assert()
        .success();
    assert!(workspace.join("deploy/node_modules/lib").exists());
    assert!(!workspace.join("deploy/node_modules/dev-only").exists());

    drop((root, mock_instance));
}

#[test]
fn shared_lockfile_deploy_honors_no_optional_in_graph_and_virtual_store() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);
    write_project(
        &workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": {
                "lib": "workspace:*",
                "@pnpm.e2e/support-different-architectures": "1.0.0",
            },
            "devDependencies": { "dev-only": "workspace:*" },
            "optionalDependencies": { "optional-only": "workspace:*" },
        }),
    );
    write_project(
        &workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "optionalDependencies": { "@pnpm.e2e/qar": "100.0.0" },
        }),
    );
    write_project(
        &workspace,
        "optional-only",
        &serde_json::json!({
            "name": "optional-only",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
        }),
    );

    pacquet.with_arg("install").assert().success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy-with-optional"])
        .assert()
        .success();
    let with_optional = workspace.join("deploy-with-optional");
    assert!(with_optional.join("node_modules/optional-only").exists());
    let graph_keys = deploy_graph_keys(&with_optional);
    for included in ["@pnpm.e2e/qar@100.0.0", "@pnpm.e2e/foo@100.0.0"] {
        assert!(
            graph_keys.iter().any(|key| key.contains(included)),
            "default deploy lock graph should include {included}: {graph_keys:#?}",
        );
    }
    let optional_edges = deploy_optional_edges(&with_optional);
    assert!(
        optional_edges.iter().any(|(key, names)| key.contains("lib@file:")
            && names.iter().any(|name| name == "@pnpm.e2e/qar")),
        "default deploy should keep the optional edge on the retained production dependency: {optional_edges:#?}",
    );

    pacquet_cmd(&workspace)
        .with_args([
            "--filter",
            "app",
            "deploy",
            "--prod",
            "--no-optional",
            "deploy-without-optional",
        ])
        .assert()
        .success();
    let without_optional = workspace.join("deploy-without-optional");
    assert!(
        without_optional.join("node_modules/lib").exists(),
        "the production dependency carrying the optional edges should still be deployed",
    );
    assert!(!without_optional.join("node_modules/optional-only").exists());
    let graph_keys = deploy_graph_keys(&without_optional);
    for excluded in ["optional-only@file:", "@pnpm.e2e/qar@", "@pnpm.e2e/foo@"] {
        assert!(
            !graph_keys.iter().any(|key| key.contains(excluded)),
            "no-optional deploy lock graph should exclude {excluded}: {graph_keys:#?}",
        );
    }
    let virtual_store_entries = virtual_store_entries(&without_optional);
    for excluded in ["optional-only@file+", "@pnpm.e2e+qar@", "@pnpm.e2e+foo@"] {
        assert!(
            !virtual_store_entries.iter().any(|entry| entry.contains(excluded)),
            "no-optional deploy virtual store should exclude {excluded}: {virtual_store_entries:#?}",
        );
    }

    let retained_optional_edges = deploy_optional_edges(&without_optional);
    assert!(
        retained_optional_edges.is_empty(),
        "retained production snapshots must not keep pruned optional edges: {retained_optional_edges:#?}",
    );

    drop((root, mock_instance));
}

/// A deployed lockfile must never reference a package the graph prune drops:
/// a later install in the deploy directory would link the missing package and
/// leave the dangling symlinks of <https://github.com/pnpm/pnpm/issues/13623>.
#[test]
fn shared_lockfile_deploy_drops_excluded_direct_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);
    write_project(
        &workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
            "devDependencies": { "@pnpm.e2e/bar": "100.0.0" },
            "optionalDependencies": { "@pnpm.e2e/qar": "100.0.0" },
            "peerDependencies": {
                "@pnpm.e2e/bar": "*",
                "@pnpm.e2e/peer-c": "1.0.0",
            },
            "peerDependenciesMeta": {
                "@pnpm.e2e/bar": { "optional": true },
                "@pnpm.e2e/peer-c": { "optional": true },
            },
        }),
    );

    pacquet.with_arg("install").assert().success();
    // Deploying outside the workspace keeps the follow-up install standalone.
    let deploy_dir = root.path().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "--no-optional"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    let deploy_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(deploy_dir.join("package.json")).unwrap())
            .unwrap();
    assert!(
        deploy_manifest["dependencies"]["@pnpm.e2e/foo"].is_string(),
        "the production dependency should survive: {deploy_manifest:#}",
    );
    assert!(
        deploy_manifest["dependencies"]["@pnpm.e2e/peer-c"].is_string(),
        "the auto-installed peer should survive: {deploy_manifest:#}",
    );
    for excluded in ["devDependencies", "optionalDependencies"] {
        assert_eq!(
            deploy_manifest[excluded],
            serde_json::json!({}),
            "the deployed manifest should drop {excluded}: {deploy_manifest:#}",
        );
    }
    assert_eq!(
        deploy_manifest["peerDependencies"],
        serde_json::json!({ "@pnpm.e2e/peer-c": "1.0.0" }),
    );
    assert_eq!(
        deploy_manifest["peerDependenciesMeta"],
        serde_json::json!({ "@pnpm.e2e/peer-c": { "optional": true } }),
    );

    let deploy_lockfile = Lockfile::load_wanted_from_dir(&deploy_dir).unwrap().unwrap();
    let importer = deploy_lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY).unwrap();
    assert!(importer.dev_dependencies.is_none(), "{:#?}", importer.dev_dependencies);
    assert!(importer.optional_dependencies.is_none(), "{:#?}", importer.optional_dependencies);
    let graph_keys = deploy_graph_keys(&deploy_dir);
    for excluded in ["@pnpm.e2e/bar@", "@pnpm.e2e/qar@"] {
        assert!(
            !graph_keys.iter().any(|key| key.contains(excluded)),
            "the deploy lock graph should exclude {excluded}: {graph_keys:#?}",
        );
    }

    fs::copy(&npmrc_path, deploy_dir.join(".npmrc")).unwrap();
    fs::remove_dir_all(deploy_dir.join("node_modules")).unwrap();
    pacquet_cmd(&deploy_dir).with_args(["install", "--frozen-lockfile"]).assert().success();
    let dangling = dangling_links(&deploy_dir.join("node_modules"));
    assert!(
        dangling.is_empty(),
        "installing the deployed lockfile must not create dangling symlinks: {dangling:#?}",
    );

    let dev_deploy_dir = root.path().join("dev-deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--dev", "--no-optional"])
        .with_arg(&dev_deploy_dir)
        .assert()
        .success();
    let dev_deploy_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dev_deploy_dir.join("package.json")).unwrap())
            .unwrap();
    assert_eq!(
        dev_deploy_manifest["dependencies"],
        serde_json::json!({ "@pnpm.e2e/peer-c": "1.0.0" }),
    );
    assert_eq!(
        dev_deploy_manifest["devDependencies"],
        serde_json::json!({ "@pnpm.e2e/bar": "100.0.0" }),
    );
    assert_eq!(
        dev_deploy_manifest["peerDependencies"],
        serde_json::json!({
            "@pnpm.e2e/bar": "*",
            "@pnpm.e2e/peer-c": "1.0.0",
        }),
    );
    assert_eq!(
        dev_deploy_manifest["peerDependenciesMeta"],
        serde_json::json!({
            "@pnpm.e2e/bar": { "optional": true },
            "@pnpm.e2e/peer-c": { "optional": true },
        }),
    );

    drop((root, mock_instance));
}

/// The invocation pnpm's release tooling (`bundle-deps.ts`) forwards: every
/// option ahead of the `deploy` subcommand, `--config.*` overrides for
/// settings the workspace yaml doesn't enable, and `--force` so optional
/// dependencies of every platform are materialized into the deploy dir.
#[test]
fn release_style_deploy_accepts_pre_subcommand_flags_and_forces_foreign_platform_optionals() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    write_project(
        &workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "lib": "workspace:*" },
            "devDependencies": { "dev-only": "workspace:*" },
            "optionalDependencies": { "@pnpm.e2e/not-compatible-with-any-os": "1.0.0" },
        }),
    );

    pacquet.with_arg("install").assert().success();

    let incompatible = "node_modules/@pnpm.e2e/not-compatible-with-any-os";
    pacquet_cmd(&workspace)
        .with_args([
            "--config.inject-workspace-packages=true",
            "--config.node-linker=hoisted",
            "--ignore-scripts",
            "--filter=app",
            "--prod",
            "deploy",
            "plain-deploy",
        ])
        .assert()
        .success();
    assert!(
        !workspace.join("plain-deploy").join(incompatible).exists(),
        "without --force the platform-incompatible optional dependency stays skipped",
    );
    assert!(
        !workspace.join("plain-deploy/node_modules/dev-only").exists(),
        "the hoisted deploy install must not materialize dev dependencies with --prod",
    );

    pacquet_cmd(&workspace)
        .with_args([
            "--config.inject-workspace-packages=true",
            "--config.node-linker=hoisted",
            "--ignore-scripts",
            "--force",
            "--filter=app",
            "--prod",
            "deploy",
            "release-deploy",
        ])
        .assert()
        .success();

    let deploy_dir = workspace.join("release-deploy");
    assert!(deploy_dir.join("index.js").exists());
    assert!(
        deploy_dir.join(incompatible).exists(),
        "--force must install optional dependencies regardless of platform",
    );
    assert!(
        !deploy_dir.join("node_modules/dev-only").exists(),
        "dev-only workspace dependency should not be linked with --prod",
    );
    assert!(
        deploy_dir.join("node_modules/.modules.yaml").exists(),
        "the hoisted deploy install should write the modules state file",
    );

    drop((root, mock_instance));
}
