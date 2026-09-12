use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, Lockfile, PNPMFILE_SENTINEL, PackageKey,
    PkgName, append_workspace_yaml_key, assert_ignored_broken_source_lockfile,
    assert_workspace_lockfile_untouched, deployed_package_version, fs, pack_pnpmfile_with_project,
    pacquet_cmd, set_app_foo_dependency, virtual_store_entries, write_project,
    write_reachability_workspace, write_recording_pnpmfile, write_root_project_depending_on_lib,
    write_workspace,
};
use assert_cmd::assert::OutputAssertExt;

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
fn legacy_deploy_excludes_fetched_dependencies_of_unselected_projects() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_reachability_workspace(&workspace);

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    pacquet_cmd(&workspace).with_arg("fetch").assert().success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app...", "install", "--frozen-lockfile", "--offline"])
        .assert()
        .success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--legacy", "--prod", "legacy-deploy"])
        .assert()
        .success();

    let virtual_store_entries = virtual_store_entries(&workspace.join("legacy-deploy"));
    assert!(
        virtual_store_entries.iter().any(|entry| entry.starts_with("@pnpm.e2e+pkg-with-1-dep@")),
        "the deploy virtual store should include the selected dependency closure: {virtual_store_entries:#?}",
    );
    for excluded in ["@pnpm.e2e+bar@", "@pnpm.e2e+qar@"] {
        assert!(
            !virtual_store_entries.iter().any(|entry| entry.starts_with(excluded)),
            "the deploy virtual store should exclude packages reachable only from unselected projects: {virtual_store_entries:#?}",
        );
    }

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_injects_transitive_workspace_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    write_project(
        &workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "leaf": "workspace:*" },
        }),
    );
    write_project(
        &workspace,
        "leaf",
        &serde_json::json!({
            "name": "leaf",
            "version": "1.0.0",
            "files": ["index.js"],
        }),
    );

    pacquet.with_arg("install").assert().success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--legacy", "--prod", "legacy-deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("legacy-deploy");
    let virtual_store_entries = virtual_store_entries(&deploy_dir);
    let lib_entry = virtual_store_entries
        .iter()
        .find(|entry| entry.starts_with("lib@file+"))
        .expect("lib should be injected into the deploy virtual store");
    let leaf_entry = virtual_store_entries
        .iter()
        .find(|entry| entry.starts_with("leaf@file+"))
        .expect("transitive leaf should be injected into the deploy virtual store");
    let nested_leaf =
        deploy_dir.join("node_modules/.pnpm").join(lib_entry).join("node_modules/leaf");
    let deployed_leaf =
        deploy_dir.join("node_modules/.pnpm").join(leaf_entry).join("node_modules/leaf");
    let deploy_dir = fs::canonicalize(deploy_dir).expect("resolve the deploy directory");
    let deployed_lib = fs::canonicalize(deploy_dir.join("node_modules/lib"))
        .expect("resolve the deployed lib package");
    let nested_leaf = fs::canonicalize(nested_leaf).expect("resolve lib's leaf dependency");
    let deployed_leaf = fs::canonicalize(deployed_leaf).expect("resolve the deployed leaf package");
    for deployed_package in [&deployed_lib, &nested_leaf, &deployed_leaf] {
        assert!(
            deployed_package.starts_with(&deploy_dir),
            "{deployed_package:?} should resolve inside {deploy_dir:?}",
        );
    }
    assert_eq!(nested_leaf, deployed_leaf);

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
    let source_foo_key: PackageKey =
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
    let foo_name = PkgName::parse("@pnpm.e2e/foo").expect("parse fixture package name");
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
    append_workspace_yaml_key(&workspace, "sharedWorkspaceLockfile", false);
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
    let source_foo_key: PackageKey =
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
fn legacy_deploy_prefers_git_branch_lockfile_versions() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    fs::create_dir(workspace.join(".git")).expect("create source git directory");
    fs::write(workspace.join(".git/HEAD"), "ref: refs/heads/feature\n")
        .expect("write source git branch");
    append_workspace_yaml_key(&workspace, "gitBranchLockfile", true);
    set_app_foo_dependency(&workspace, "100.0.0");

    pacquet.with_arg("install").assert().success();
    let source_lockfile_path = workspace.join("pnpm-lock.feature.yaml");
    let source_lockfile = fs::read(&source_lockfile_path).expect("read source branch lockfile");
    assert!(!workspace.join(Lockfile::FILE_NAME).exists());
    set_app_foo_dependency(&workspace, "^100.0.0");

    pacquet_cmd(&workspace)
        .with_args([
            "--filter",
            "app",
            "deploy",
            "--legacy",
            "--prod",
            "legacy-deploy-with-branch-lockfile",
        ])
        .assert()
        .success();

    let deploy_dir = workspace.join("legacy-deploy-with-branch-lockfile");
    assert_eq!(deployed_package_version(&deploy_dir, "@pnpm.e2e/foo"), "100.0.0");
    assert_eq!(
        fs::read(&source_lockfile_path).expect("reread source branch lockfile"),
        source_lockfile,
    );
    assert!(!workspace.join(Lockfile::FILE_NAME).exists());
    assert!(!deploy_dir.join(Lockfile::FILE_NAME).exists());

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_without_dedicated_lockfile_fresh_resolves() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    append_workspace_yaml_key(&workspace, "sharedWorkspaceLockfile", false);
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
    append_workspace_yaml_key(&workspace, "sharedWorkspaceLockfile", false);
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
    assert_ignored_broken_source_lockfile(&output, &app_dir);
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
  if (pkg.name === 'app') {
    pkg.dependencies['@pnpm.e2e/foo'] =
      pkg.dependencies['@pnpm.e2e/foo'] === '^100.0.0' ? '100.0.0' : '100.1.0'
  }
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
    let foo_name = PkgName::parse("@pnpm.e2e/foo").expect("parse fixture package name");
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
    assert_ignored_broken_source_lockfile(&output, &workspace);
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

/// Without a shared lockfile the deploy takes the legacy path, where the
/// pnpmfile an install of the selected project loads is the project's own.
#[test]
fn legacy_deploy_ignores_the_pnpmfile_copied_into_the_deploy_dir() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);
    append_workspace_yaml_key(&workspace, "sharedWorkspaceLockfile", false);
    let project_dir = workspace.join("packages/app");
    write_recording_pnpmfile(&project_dir);
    pack_pnpmfile_with_project(&project_dir);

    pacquet.with_arg("install").assert().success();
    fs::remove_file(project_dir.join(PNPMFILE_SENTINEL)).expect("the install ran the pnpmfile");

    pacquet_cmd(&workspace).with_args(["--filter", "app", "deploy", "deploy"]).assert().success();

    assert!(
        project_dir.join(PNPMFILE_SENTINEL).exists(),
        "the legacy deploy install should run the selected project's pnpmfile",
    );
    let deploy_dir = workspace.join("deploy");
    assert!(
        deploy_dir.join(".pnpmfile.mjs").exists(),
        "the deployed packlist should have carried the project's pnpmfile over",
    );
    assert!(
        !deploy_dir.join(PNPMFILE_SENTINEL).exists(),
        "the legacy deploy install must not load the pnpmfile it just copied",
    );

    drop((root, mock_instance));
}

/// A deploy install resolves the deployed project, not the workspace, and
/// never saves the workspace lockfile — so it has not merged the branch
/// lockfiles and must not delete them.
#[test]
fn legacy_deploy_keeps_the_workspace_branch_lockfiles() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_reachability_workspace(&workspace);

    pacquet.with_arg("install").assert().success();

    fs::create_dir(workspace.join(".git")).unwrap();
    fs::write(workspace.join(".git/HEAD"), "ref: refs/heads/feature\n").unwrap();
    let mut workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap();
    workspace_yaml.push_str("mergeGitBranchLockfiles: true\n");
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml).unwrap();
    let branch_lockfile = workspace.join("pnpm-lock.other.yaml");
    fs::write(&branch_lockfile, "lockfileVersion: '9.0'\n").unwrap();

    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--legacy", "--prod", "deploy"])
        .assert()
        .success();

    assert!(
        branch_lockfile.exists(),
        "a deploy install never saves the workspace lockfile, so it cannot have merged them",
    );

    drop((root, mock_instance));
}
