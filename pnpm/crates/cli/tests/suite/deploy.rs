use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::Lockfile;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{fmt::Write as _, fs, path::Path, process::Command};

#[test]
fn deploy_from_shared_lockfile_installs_selected_project() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_reachability_workspace(&workspace);

    pacquet.with_arg("install").assert().success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("deploy");
    let deploy_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(deploy_dir.join("package.json")).unwrap())
            .unwrap();
    assert_eq!(deploy_manifest["name"], "app");
    assert!(
        deploy_manifest["dependencies"]["lib"]
            .as_str()
            .is_some_and(|version| version.starts_with("lib@file://")),
        "deployed manifest should point workspace dependencies at file URLs: {deploy_manifest:#}",
    );
    assert!(deploy_dir.join("index.js").exists());
    assert!(
        !deploy_dir.join("test.js").exists(),
        "deploy should copy the package packlist by default",
    );
    assert!(deploy_dir.join("pnpm-lock.yaml").exists());
    let lockfile = fs::read_to_string(deploy_dir.join("pnpm-lock.yaml")).unwrap();
    assert!(
        !lockfile.contains("injectWorkspacePackages: true"),
        "deploy lockfile should not preserve injectWorkspacePackages: true:\n{lockfile}",
    );

    let lib_link = deploy_dir.join("node_modules/lib");
    assert!(
        is_symlink_or_junction(&lib_link).unwrap(),
        "prod workspace dependency should be linked into the deploy dir",
    );
    assert!(
        !deploy_dir.join("node_modules/dev-only").exists(),
        "dev-only workspace dependency should not be linked with --prod",
    );

    let graph_keys = deploy_graph_keys(&deploy_dir);
    assert!(
        graph_keys.iter().any(|key| key.contains("@pnpm.e2e/pkg-with-1-dep@100.0.0")),
        "production dependency should remain in the deploy lock graph: {graph_keys:#?}",
    );
    assert!(
        graph_keys.iter().any(|key| key.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@")),
        "transitive production dependency should remain in the deploy lock graph: {graph_keys:#?}",
    );
    for excluded in
        ["dev-only@file:", "@pnpm.e2e/bar@100.0.0", "unused@file:", "@pnpm.e2e/qar@100.0.0"]
    {
        assert!(
            !graph_keys.iter().any(|key| key.contains(excluded)),
            "production deploy lock graph should exclude {excluded}: {graph_keys:#?}",
        );
    }

    let virtual_store_entries = virtual_store_entries(&deploy_dir);
    assert!(
        virtual_store_entries
            .iter()
            .any(|entry| entry.contains("@pnpm.e2e+dep-of-pkg-with-1-dep@")),
        "transitive production dependency should be materialized: {virtual_store_entries:#?}",
    );
    for excluded in
        ["dev-only@file+", "@pnpm.e2e+bar@100.0.0", "unused@file+", "@pnpm.e2e+qar@100.0.0"]
    {
        assert!(
            !virtual_store_entries.iter().any(|entry| entry.contains(excluded)),
            "production deploy virtual store should exclude {excluded}: {virtual_store_entries:#?}",
        );
    }

    drop((root, mock_instance));
}

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

#[test]
fn deploy_from_shared_lockfile_supports_catalog_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml = fs::read_to_string(&workspace_yaml_path).unwrap();
    workspace_yaml.push_str("catalog:\n  '@pnpm.e2e/foo': 100.0.0\n");
    fs::write(workspace_yaml_path, workspace_yaml).unwrap();
    let manifest_path = workspace.join("packages/app/package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["dependencies"]["@pnpm.e2e/foo"] = serde_json::Value::String("catalog:".to_string());
    fs::write(manifest_path, manifest.to_string()).unwrap();

    pacquet.with_arg("install").assert().success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("deploy");
    assert!(deploy_dir.join("node_modules/@pnpm.e2e/foo").exists());
    let lockfile = fs::read_to_string(deploy_dir.join("pnpm-lock.yaml")).unwrap();
    assert!(!lockfile.contains("catalogs:"), "unexpected catalog snapshot:\n{lockfile}");

    drop((root, mock_instance));
}

#[test]
fn deploy_refuses_non_empty_target_without_force() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    fs::create_dir_all(workspace.join("deploy")).unwrap();
    fs::write(workspace.join("deploy/keep.txt"), "keep").unwrap();

    let output = pacquet
        .with_args(["--filter", "app", "deploy", "--legacy", "deploy"])
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_DEPLOY_DIR_NOT_EMPTY") && stderr.contains("empty"),
        "unexpected stderr:\n{stderr}",
    );
    assert_eq!(fs::read_to_string(workspace.join("deploy/keep.txt")).unwrap(), "keep");

    drop((root, mock_instance));
}

#[test]
fn shared_lockfile_deploy_refuses_non_injected_workspace_before_target_mutation() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);

    let output = pacquet
        .with_args(["--filter", "app", "deploy", "deploy"])
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("inject-workspace-packages=true"), "unexpected stderr:\n{stderr}");
    assert!(
        !workspace.join("deploy").exists(),
        "non-injected shared-lockfile deploy must fail before creating the target",
    );

    drop((root, mock_instance));
}

#[test]
fn force_deploy_rejects_out_of_scope_target_without_deleting_it() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    let outside = root.path().join("outside-deploy");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), "keep").unwrap();

    let output = pacquet
        .with_args(["--filter", "app", "deploy", "--legacy", "--force", outside.to_str().unwrap()])
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let flattened = flatten_miette_report(&stderr);
    assert!(
        flattened.contains("unsafe target") && flattened.contains("outside the workspace"),
        "unexpected stderr:\n{stderr}",
    );
    assert_eq!(fs::read_to_string(outside.join("keep.txt")).unwrap(), "keep");

    drop((root, mock_instance));
}

#[cfg(unix)]
#[test]
fn deploy_all_files_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    let mut workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap();
    workspace_yaml.push_str("deployAllFiles: true\n");
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml).unwrap();
    let outside = root.path().join("outside-source");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    symlink(&outside, workspace.join("packages/app/outside")).unwrap();

    let output = pacquet
        .with_args(["--filter", "app", "deploy", "--legacy", "deploy"])
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let flattened = flatten_miette_report(&stderr);
    assert!(
        flattened.contains("ERR_PNPM_DIRECTORY_FETCHER_PATH_ESCAPE")
            && flattened.contains("resolves outside source directory"),
        "unexpected stderr:\n{stderr}",
    );
    assert!(
        !workspace.join("deploy/outside/secret.txt").exists(),
        "deploy must not copy files reached through an outside symlink",
    );

    drop((root, mock_instance));
}

#[cfg(unix)]
#[test]
fn deploy_rejects_symlinked_target_parent() {
    use std::os::unix::fs::symlink;

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    let outside = root.path().join("outside-target");
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, workspace.join("out")).unwrap();

    let output = pacquet
        .with_args(["--filter", "app", "deploy", "--legacy", "--force", "out/deploy"])
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let flattened = flatten_miette_report(&stderr);
    assert!(
        flattened.contains("ERR_PNPM_INVALID_DEPLOY_TARGET")
            && flattened.contains("contains a symlink"),
        "unexpected stderr:\n{stderr}",
    );
    assert!(
        !outside.join("deploy").exists(),
        "deploy must not create output through a symlinked target parent",
    );

    drop((root, mock_instance));
}

#[cfg(windows)]
#[test]
fn deploy_rejects_linked_target_parent() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    let outside = root.path().join("outside-target");
    fs::create_dir_all(&outside).unwrap();
    pnpm_fs::symlink_dir(&outside, &workspace.join("out")).unwrap();

    let output = pacquet
        .with_args(["--filter", "app", "deploy", "--legacy", "--force", "out/deploy"])
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let flattened = flatten_miette_report(&stderr);
    assert!(
        flattened.contains("ERR_PNPM_INVALID_DEPLOY_TARGET")
            && flattened.contains("contains a symlink or junction"),
        "unexpected stderr:\n{stderr}",
    );
    assert!(
        !outside.join("deploy").exists(),
        "deploy must not create output through a linked target parent",
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

#[test]
fn legacy_deploy_installs_selected_project() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);

    pacquet.with_arg("install").assert().success();
    let workspace_lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--legacy", "--prod", "legacy-deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("legacy-deploy");
    assert!(deploy_dir.join("index.js").exists());
    assert!(!deploy_dir.join("test.js").exists());
    assert!(deploy_dir.join("node_modules/lib").exists());
    assert!(!deploy_dir.join("node_modules/dev-only").exists());
    assert_workspace_lockfile_untouched(&workspace, &workspace_lockfile);

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_prefers_workspace_lockfile_versions() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    set_app_foo_dependency(&workspace, "100.0.0");

    pacquet.with_arg("install").assert().success();
    let source_lockfile_path = workspace.join(Lockfile::FILE_NAME);
    let source_wanted_lockfile = Lockfile::load_wanted_from_dir(&workspace)
        .expect("load source wanted lockfile")
        .expect("source wanted lockfile exists");
    let source_foo_key: pnpm_lockfile::PackageKey =
        "@pnpm.e2e/foo@100.0.0".parse().expect("parse source package key");
    assert!(
        source_wanted_lockfile
            .snapshots
            .as_ref()
            .is_some_and(|snapshots| snapshots.contains_key(&source_foo_key)),
        "the source fixture must pin foo at 100.0.0",
    );
    let source_lockfile = fs::read(&source_lockfile_path).expect("read source lockfile");
    set_app_foo_dependency(&workspace, "^100.0.0");

    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--legacy", "--prod", "legacy-deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("legacy-deploy");
    assert_eq!(fs::read(&source_lockfile_path).unwrap(), source_lockfile);
    assert_eq!(
        deployed_package_version(&deploy_dir, "@pnpm.e2e/foo"),
        "100.0.0",
        "legacy deploy should prefer the satisfying version pinned by the source workspace lockfile",
    );
    let deploy_manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(deploy_dir.join("package.json")).expect("read deploy manifest"),
    )
    .expect("parse deploy manifest");
    assert_eq!(deploy_manifest["name"], "app");
    assert_eq!(deploy_manifest["dependencies"]["@pnpm.e2e/foo"], "^100.0.0");
    assert_eq!(deploy_manifest["dependenciesMeta"]["lib"]["injected"], true);

    let virtual_store_dir = deploy_dir.join("node_modules/.pnpm");
    let current_lockfile = Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        .expect("load deploy current lockfile")
        .expect("deploy current lockfile exists");
    assert_eq!(
        current_lockfile.importers.keys().map(String::as_str).collect::<Vec<_>>(),
        vec![Lockfile::ROOT_IMPORTER_KEY],
        "the post-hook deploy manifest should be the sole root importer",
    );
    let root_importer = current_lockfile.root_project().expect("root deploy importer exists");
    let foo_name =
        pnpm_lockfile::PkgName::parse("@pnpm.e2e/foo").expect("parse fixture package name");
    let foo_dependency = root_importer
        .dependencies
        .as_ref()
        .expect("root deploy dependencies exist")
        .get(&foo_name)
        .expect("root deploy importer contains foo");
    assert_eq!(foo_dependency.specifier, "^100.0.0");
    assert_eq!(foo_dependency.version.to_string(), "100.0.0");
    assert_eq!(
        root_importer.dependencies_meta.as_ref().expect("root deploy dependenciesMeta exists")["lib"]
            ["injected"],
        true,
    );

    let modules = pnpm_modules_yaml::read_modules_layout::<pnpm_modules_yaml::Host>(
        &deploy_dir.join("node_modules"),
    )
    .expect("read deploy .modules.yaml")
    .expect("deploy .modules.yaml exists");
    assert_eq!(
        dunce::canonicalize(&modules.virtual_store_dir)
            .expect("canonicalize recorded deploy virtual store"),
        dunce::canonicalize(&virtual_store_dir)
            .expect("canonicalize expected deploy virtual store"),
        "deploy .modules.yaml should record the target-local virtual store",
    );
    let deployed_foo_dir = dunce::canonicalize(deploy_dir.join("node_modules/@pnpm.e2e/foo"))
        .expect("canonicalize deployed foo directory");
    assert!(
        deployed_foo_dir.starts_with(
            dunce::canonicalize(&virtual_store_dir)
                .expect("canonicalize expected deploy virtual store"),
        ),
        "the deployed package should resolve inside the target virtual store: {}",
        deployed_foo_dir.display(),
    );
    assert!(
        !deploy_dir.join(Lockfile::FILE_NAME).exists(),
        "legacy deploy must not copy the source wanted lockfile into the target",
    );

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_prefers_dedicated_lockfile_versions() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    let workspace_manifest_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_manifest =
        fs::read_to_string(&workspace_manifest_path).expect("read workspace manifest");
    writeln!(workspace_manifest, "sharedWorkspaceLockfile: false")
        .expect("configure dedicated lockfiles");
    fs::write(workspace_manifest_path, workspace_manifest).expect("write workspace manifest");
    set_app_foo_dependency(&workspace, "100.0.0");

    pacquet.with_args(["--filter", "app", "install"]).assert().success();
    let app_dir = workspace.join("packages/app");
    let source_lockfile_path = app_dir.join(Lockfile::FILE_NAME);
    assert!(
        !workspace.join(Lockfile::FILE_NAME).exists(),
        "a dedicated install must not write the wanted lockfile at the workspace root",
    );
    let source_wanted_lockfile = Lockfile::load_wanted_from_dir(&app_dir)
        .expect("load source project wanted lockfile")
        .expect("source project wanted lockfile exists");
    let source_foo_key: pnpm_lockfile::PackageKey =
        "@pnpm.e2e/foo@100.0.0".parse().expect("parse source package key");
    assert!(
        source_wanted_lockfile
            .snapshots
            .as_ref()
            .is_some_and(|snapshots| snapshots.contains_key(&source_foo_key)),
        "the source project fixture must pin foo at 100.0.0",
    );
    let source_lockfile = fs::read(&source_lockfile_path).expect("read source project lockfile");
    set_app_foo_dependency(&workspace, "^100.0.0");

    pacquet_cmd(&workspace)
        .with_args([
            "--filter",
            "app",
            "deploy",
            "--legacy",
            "--prod",
            "legacy-deploy-with-dedicated-lockfile",
        ])
        .assert()
        .success();

    let deploy_dir = workspace.join("legacy-deploy-with-dedicated-lockfile");
    assert_eq!(
        fs::read(&source_lockfile_path).expect("reread source project lockfile"),
        source_lockfile,
    );
    assert!(
        !workspace.join(Lockfile::FILE_NAME).exists(),
        "legacy deploy must not write a wanted lockfile at the workspace root",
    );
    assert_eq!(
        deployed_package_version(&deploy_dir, "@pnpm.e2e/foo"),
        "100.0.0",
        "legacy deploy should prefer the satisfying version pinned by the source project lockfile",
    );
    assert!(
        !deploy_dir.join(Lockfile::FILE_NAME).exists(),
        "legacy deploy must not copy the source project wanted lockfile into the target",
    );

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_without_dedicated_lockfile_fresh_resolves() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    let workspace_manifest_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_manifest =
        fs::read_to_string(&workspace_manifest_path).expect("read workspace manifest");
    writeln!(workspace_manifest, "sharedWorkspaceLockfile: false")
        .expect("configure dedicated lockfiles");
    fs::write(workspace_manifest_path, workspace_manifest).expect("write workspace manifest");
    set_app_foo_dependency(&workspace, "^100.0.0");

    let app_dir = workspace.join("packages/app");
    assert!(!workspace.join(Lockfile::FILE_NAME).exists());
    assert!(!app_dir.join(Lockfile::FILE_NAME).exists());
    pacquet_cmd(&workspace)
        .with_args([
            "--filter",
            "app",
            "deploy",
            "--legacy",
            "--prod",
            "legacy-deploy-without-dedicated-lockfile",
        ])
        .assert()
        .success();

    let deploy_dir = workspace.join("legacy-deploy-without-dedicated-lockfile");
    assert_eq!(deployed_package_version(&deploy_dir, "@pnpm.e2e/foo"), "100.1.0");
    assert!(!workspace.join(Lockfile::FILE_NAME).exists());
    assert!(!app_dir.join(Lockfile::FILE_NAME).exists());
    assert!(!deploy_dir.join(Lockfile::FILE_NAME).exists());

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_ignores_malformed_dedicated_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    let workspace_manifest_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_manifest =
        fs::read_to_string(&workspace_manifest_path).expect("read workspace manifest");
    writeln!(workspace_manifest, "sharedWorkspaceLockfile: false")
        .expect("configure dedicated lockfiles");
    fs::write(workspace_manifest_path, workspace_manifest).expect("write workspace manifest");
    set_app_foo_dependency(&workspace, "100.0.0");

    pacquet.with_args(["--filter", "app", "install"]).assert().success();
    set_app_foo_dependency(&workspace, "^100.0.0");
    let app_dir = workspace.join("packages/app");
    let source_lockfile_path = app_dir.join(Lockfile::FILE_NAME);
    let source_lockfile =
        fs::read_to_string(&source_lockfile_path).expect("read source project lockfile");
    fs::write(&source_lockfile_path, format!("{source_lockfile}\nlockfileVersion: '9.0'\n"))
        .expect("duplicate a source project lockfile key");
    let malformed_source_lockfile =
        fs::read(&source_lockfile_path).expect("read malformed source project lockfile");

    let output = pacquet_cmd(&workspace)
        .with_args([
            "--reporter=ndjson",
            "--filter",
            "app",
            "deploy",
            "--legacy",
            "--prod",
            "legacy-deploy-with-malformed-dedicated-lockfile",
        ])
        .output()
        .expect("deploy with a malformed source project lockfile");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        output.status.success(),
        "legacy deploy should ignore the malformed source project lockfile:\n{combined}",
    );
    let canonical_app_dir = dunce::canonicalize(&app_dir).expect("canonicalize app directory");
    assert!(
        combined
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .any(|event| {
                event["name"] == "pnpm"
                    && event["level"] == "warn"
                    && event["prefix"] == canonical_app_dir.to_string_lossy().as_ref()
                    && event["message"]
                        .as_str()
                        .is_some_and(|message| message.starts_with("Ignoring broken lockfile at "))
            }),
        "expected a warning for the ignored source project lockfile; got:\n{combined}",
    );
    let deploy_dir = workspace.join("legacy-deploy-with-malformed-dedicated-lockfile");
    assert_eq!(deployed_package_version(&deploy_dir, "@pnpm.e2e/foo"), "100.1.0");
    assert!(!workspace.join(Lockfile::FILE_NAME).exists());
    assert!(!deploy_dir.join(Lockfile::FILE_NAME).exists());
    assert_eq!(
        fs::read(&source_lockfile_path).expect("reread malformed source project lockfile"),
        malformed_source_lockfile,
    );

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_preserves_source_pnpmfile_hooks() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    set_app_foo_dependency(&workspace, "^100.0.0");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = { hooks: { readPackage (pkg) {
  if (pkg.name === 'app') pkg.dependencies['@pnpm.e2e/foo'] = '100.0.0'
  return pkg
} } }
",
    )
    .expect("write source pnpmfile");

    pacquet_cmd(&workspace)
        .with_args([
            "--filter",
            "app",
            "deploy",
            "--legacy",
            "--prod",
            "legacy-deploy-with-pnpmfile",
        ])
        .assert()
        .success();

    let deploy_dir = workspace.join("legacy-deploy-with-pnpmfile");
    assert!(!workspace.join(Lockfile::FILE_NAME).exists());
    assert!(!deploy_dir.join(".pnpmfile.cjs").exists());
    assert_eq!(deployed_package_version(&deploy_dir, "@pnpm.e2e/foo"), "100.0.0");
    let deploy_manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(deploy_dir.join("package.json")).expect("read deploy manifest"),
    )
    .expect("parse deploy manifest");
    assert_eq!(deploy_manifest["dependencies"]["@pnpm.e2e/foo"], "^100.0.0");
    let current_lockfile =
        Lockfile::load_current_from_virtual_store_dir(&deploy_dir.join("node_modules/.pnpm"))
            .expect("load deploy current lockfile")
            .expect("deploy current lockfile exists");
    let foo_name =
        pnpm_lockfile::PkgName::parse("@pnpm.e2e/foo").expect("parse fixture package name");
    let foo_dependency = current_lockfile
        .root_project()
        .expect("root deploy importer exists")
        .dependencies
        .as_ref()
        .expect("root deploy dependencies exist")
        .get(&foo_name)
        .expect("root deploy importer contains foo");
    assert_eq!(foo_dependency.specifier, "100.0.0");
    assert_eq!(foo_dependency.version.to_string(), "100.0.0");

    drop((root, mock_instance));
}

#[test]
fn deploy_from_shared_lockfile_installs_the_workspace_root_without_its_nested_projects() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);
    write_root_project_depending_on_lib(&workspace);

    pacquet.with_arg("install").assert().success();
    let workspace_lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    pacquet_cmd(&workspace)
        .with_args(["--filter", ".", "deploy", "--prod", "deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("deploy");
    assert!(deploy_dir.join("node_modules/lib").exists());
    let deploy_lockfile = Lockfile::load_wanted_from_dir(&deploy_dir).unwrap().unwrap();
    assert_eq!(
        deploy_lockfile.importers.keys().collect::<Vec<_>>(),
        vec![Lockfile::ROOT_IMPORTER_KEY],
    );
    assert_workspace_lockfile_untouched(&workspace, &workspace_lockfile);

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_of_the_workspace_root_injects_its_workspace_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    write_root_project_depending_on_lib(&workspace);

    pacquet.with_arg("install").assert().success();
    let workspace_lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    pacquet_cmd(&workspace)
        .with_args(["--filter", ".", "deploy", "--legacy", "--prod", "legacy-deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("legacy-deploy");
    let deploy_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(deploy_dir.join("package.json")).unwrap())
            .unwrap();
    assert_eq!(
        deploy_manifest["name"], "root",
        "`--filter .` should deploy the project in the current directory, not the projects nested under it: {deploy_manifest:#}",
    );
    assert!(deploy_dir.join("node_modules/lib").exists());
    let virtual_store_entries = virtual_store_entries(&deploy_dir);
    assert!(
        virtual_store_entries.iter().any(|entry| entry.starts_with("lib@file+")),
        "the root's workspace dependency should be injected into the deploy virtual store: {virtual_store_entries:#?}",
    );
    assert_workspace_lockfile_untouched(&workspace, &workspace_lockfile);

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_without_lockfile_installs_selected_project_at_root() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    set_app_foo_dependency(&workspace, "100.0.0");

    pacquet.with_arg("install").assert().success();
    let source_lockfile_path = workspace.join(Lockfile::FILE_NAME);
    let source_lockfile = fs::read(&source_lockfile_path).expect("read source lockfile");
    set_app_foo_dependency(&workspace, "^100.0.0");

    pacquet_cmd(&workspace)
        .with_env("PNPM_CONFIG_LOCKFILE", "false")
        .with_args(["--filter", "app", "deploy", "--legacy", "--prod", "legacy-deploy-no-lockfile"])
        .assert()
        .success();

    let deploy_dir = workspace.join("legacy-deploy-no-lockfile");
    assert!(deploy_dir.join("node_modules/lib").exists());
    assert_eq!(deployed_package_version(&deploy_dir, "@pnpm.e2e/foo"), "100.1.0");
    assert!(!deploy_dir.join("legacy-deploy-no-lockfile/node_modules").exists());
    assert!(!deploy_dir.join(Lockfile::FILE_NAME).exists());
    assert_eq!(fs::read(&source_lockfile_path).expect("reread source lockfile"), source_lockfile);

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_without_source_lockfile_fresh_resolves() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    set_app_foo_dependency(&workspace, "^100.0.0");

    pacquet_cmd(&workspace)
        .with_args([
            "--filter",
            "app",
            "deploy",
            "--legacy",
            "--prod",
            "legacy-deploy-without-source-lockfile",
        ])
        .assert()
        .success();

    let deploy_dir = workspace.join("legacy-deploy-without-source-lockfile");
    assert_eq!(deployed_package_version(&deploy_dir, "@pnpm.e2e/foo"), "100.1.0");
    assert!(!workspace.join(Lockfile::FILE_NAME).exists());
    assert!(!deploy_dir.join(Lockfile::FILE_NAME).exists());

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_ignores_malformed_source_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    set_app_foo_dependency(&workspace, "100.0.0");

    pacquet.with_arg("install").assert().success();
    set_app_foo_dependency(&workspace, "^100.0.0");
    let source_lockfile_path = workspace.join(Lockfile::FILE_NAME);
    let source_lockfile = fs::read_to_string(&source_lockfile_path).expect("read source lockfile");
    fs::write(&source_lockfile_path, format!("{source_lockfile}\nlockfileVersion: '9.0'\n"))
        .expect("duplicate a source lockfile key");
    let malformed_source_lockfile =
        fs::read(&source_lockfile_path).expect("read malformed source lockfile");

    let output = pacquet_cmd(&workspace)
        .with_args([
            "--reporter=ndjson",
            "--filter",
            "app",
            "deploy",
            "--legacy",
            "--prod",
            "legacy-deploy-with-malformed-source-lockfile",
        ])
        .output()
        .expect("deploy with a malformed source lockfile");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        output.status.success(),
        "legacy deploy should ignore the malformed source lockfile:\n{combined}",
    );
    let canonical_workspace = dunce::canonicalize(&workspace).expect("canonicalize workspace");
    assert!(
        combined
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .any(|event| {
                event["name"] == "pnpm"
                    && event["level"] == "warn"
                    && event["prefix"] == canonical_workspace.to_string_lossy().as_ref()
                    && event["message"]
                        .as_str()
                        .is_some_and(|message| message.starts_with("Ignoring broken lockfile at "))
            }),
        "expected a warning for the ignored source lockfile; got:\n{combined}",
    );
    let deploy_dir = workspace.join("legacy-deploy-with-malformed-source-lockfile");
    assert_eq!(deployed_package_version(&deploy_dir, "@pnpm.e2e/foo"), "100.1.0");
    assert!(
        !deploy_dir.join(Lockfile::FILE_NAME).exists(),
        "legacy deploy must not write a wanted lockfile after ignoring the malformed source lockfile",
    );
    assert_eq!(
        fs::read(&source_lockfile_path).expect("reread malformed source lockfile"),
        malformed_source_lockfile,
    );

    drop((root, mock_instance));
}

/// Undo miette's report wrapping so phrase assertions don't depend on
/// where the temp-path length lands the wrap point: drop the box-gutter
/// glyphs and collapse the message back onto one line.
fn flatten_miette_report(stderr: &str) -> String {
    stderr
        .split_whitespace()
        .filter(|token| !matches!(*token, "│" | "×" | "╰─▶"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn write_root_project_depending_on_lib(workspace: &Path) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
            "dependencies": { "lib": "workspace:*" },
        })
        .to_string(),
    )
    .unwrap();
}

fn set_app_foo_dependency(workspace: &Path, specifier: &str) {
    let manifest_path = workspace.join("packages/app/package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).expect("read app manifest"))
            .expect("parse app manifest");
    manifest["dependencies"]["@pnpm.e2e/foo"] = specifier.into();
    fs::write(manifest_path, manifest.to_string()).expect("write app manifest");
}

fn deployed_package_version(deploy_dir: &Path, package_name: &str) -> String {
    let package_manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(deploy_dir.join("node_modules").join(package_name).join("package.json"))
            .expect("read deployed package manifest"),
    )
    .expect("parse deployed package manifest");
    package_manifest["version"].as_str().expect("deployed package has a version").to_string()
}

fn assert_workspace_lockfile_untouched(workspace: &Path, before: &str) {
    let after = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    assert_eq!(after, before, "deploy must not rewrite the workspace lockfile");
}

fn pacquet_cmd(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn deploy_graph_keys(deploy_dir: &Path) -> Vec<String> {
    let deploy_lockfile = Lockfile::load_wanted_from_dir(deploy_dir).unwrap().unwrap();
    deploy_lockfile
        .packages
        .iter()
        .flatten()
        .map(|(key, _)| key.to_string())
        .chain(deploy_lockfile.snapshots.iter().flatten().map(|(key, _)| key.to_string()))
        .collect()
}

/// Every snapshot of the deployed lockfile that still carries optional edges,
/// as `(snapshot key, optional dependency names)`.
fn deploy_optional_edges(deploy_dir: &Path) -> Vec<(String, Vec<String>)> {
    let deploy_lockfile = Lockfile::load_wanted_from_dir(deploy_dir).unwrap().unwrap();
    deploy_lockfile
        .snapshots
        .iter()
        .flatten()
        .filter_map(|(key, snapshot)| {
            let names =
                snapshot.optional_dependencies.as_ref()?.keys().map(ToString::to_string).collect();
            Some((key.to_string(), names))
        })
        .collect()
}

fn virtual_store_entries(deploy_dir: &Path) -> Vec<String> {
    fs::read_dir(deploy_dir.join("node_modules/.pnpm"))
        .expect("read the deploy virtual store")
        .map(|entry| {
            entry.expect("read a virtual store entry").file_name().to_string_lossy().into_owned()
        })
        .collect()
}

fn write_workspace(workspace: &Path, inject_workspace_packages: bool) {
    let mut workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap();
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    writeln!(
        workspace_yaml,
        "injectWorkspacePackages: {}",
        if inject_workspace_packages { "true" } else { "false" },
    )
    .unwrap();
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml).unwrap();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0", "private": true }).to_string(),
    )
    .unwrap();

    write_project(
        workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "lib": "workspace:*" },
            "devDependencies": { "dev-only": "workspace:*" },
        }),
    );
    write_project(
        workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
        }),
    );
    write_project(
        workspace,
        "dev-only",
        &serde_json::json!({
            "name": "dev-only",
            "version": "1.0.0",
            "files": ["index.js"],
        }),
    );
}

fn write_reachability_workspace(workspace: &Path) {
    write_workspace(workspace, true);
    write_project(
        workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        }),
    );
    write_project(
        workspace,
        "dev-only",
        &serde_json::json!({
            "name": "dev-only",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "@pnpm.e2e/bar": "100.0.0" },
        }),
    );
    write_project(
        workspace,
        "unused",
        &serde_json::json!({
            "name": "unused",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "@pnpm.e2e/qar": "100.0.0" },
        }),
    );
}

fn write_project(workspace: &Path, dirname: &str, manifest: &serde_json::Value) {
    let dir = workspace.join("packages").join(dirname);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("package.json"), manifest.to_string()).unwrap();
    fs::write(dir.join("index.js"), "").unwrap();
    fs::write(dir.join("test.js"), "").unwrap();
}
