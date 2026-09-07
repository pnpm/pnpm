use crate::_utils::append_workspace_yaml_key;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{Lockfile, PkgName};
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

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

/// A pinned `lockfileDir` moves the shared lockfile `deploy` reads and
/// the importer id naming the selected project in it. Reading either from
/// the workspace root instead drops the deploy to its "shared lockfile not
/// found" fallback, which installs the project without a lockfile and can
/// resolve versions the workspace never pinned.
#[test]
fn deploy_from_shared_lockfile_follows_a_pinned_lockfile_dir() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_reachability_workspace(&workspace);
    append_workspace_yaml_key(&workspace, "lockfileDir", "..");

    pacquet.with_arg("install").assert().success();
    assert!(
        root.path().join("pnpm-lock.yaml").is_file(),
        "the install must have written the lockfile at the pin",
    );

    let output = pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy"])
        .output()
        .expect("spawn pacquet deploy");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "deploy must succeed:\n{stdout}");
    assert!(
        !stdout.contains("Shared lockfile not found"),
        "deploy must find the lockfile at the pin:\n{stdout}",
    );

    let deploy_dir = workspace.join("deploy");
    assert!(
        deploy_dir.join("pnpm-lock.yaml").is_file(),
        "the deployed project gets its own lockfile, not the pinned one",
    );
    assert!(
        is_symlink_or_junction(&deploy_dir.join("node_modules/lib")).unwrap(),
        "the prod workspace dependency must be linked into the deploy dir",
    );

    drop((root, mock_instance));
}

/// A pin that does not contain the workspace gives every project an
/// importer id that climbs out of the lockfile dir, and deploy resolves
/// each of them by joining onto that dir — paths it refuses on principle.
/// The shared path cannot describe the layout, so it hands over to the
/// legacy installer rather than failing the command.
#[test]
fn deploy_falls_back_when_the_pinned_lockfile_dir_does_not_contain_the_workspace() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_reachability_workspace(&workspace);
    fs::create_dir_all(root.path().join("side")).unwrap();
    append_workspace_yaml_key(&workspace, "lockfileDir", "../side");

    pacquet.with_arg("install").assert().success();

    let output = pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy"])
        .output()
        .expect("spawn pacquet deploy");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "deploy must fall back, not fail:\n{stdout}\n{stderr}");
    assert!(
        stdout.contains("does not contain the workspace, so its importer paths cannot be deployed"),
        "the fallback must say why the shared lockfile was unusable:\n{stdout}",
    );
    assert!(
        is_symlink_or_junction(&workspace.join("deploy/node_modules/lib")).unwrap(),
        "the legacy deploy still links the prod workspace dependency",
    );

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
fn shared_lockfile_deploy_supports_non_injected_workspace() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    write_project(
        &workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "nested": "workspace:*", "@pnpm.e2e/foo": "100.0.0" },
        }),
    );
    write_project(
        &workspace,
        "nested",
        &serde_json::json!({
            "name": "nested",
            "version": "1.0.0",
            "files": ["index.js"],
        }),
    );

    pacquet.with_arg("install").assert().success();
    // The deployed lockfile stores the workspace sources as paths relative to
    // the target this deploy is handed, while the reinstall below resolves
    // them from the target's canonical path. Deploy to the canonical path so
    // the two agree: a target reached through a symlink that changes the
    // path's depth resolves those entries somewhere else entirely, which is
    // its own defect and not what this test is about.
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    let lib_link = deploy_dir.join("node_modules/lib");
    assert!(
        is_symlink_or_junction(&lib_link).unwrap(),
        "the linked workspace dependency should be materialized in the deploy directory",
    );

    let deploy_lockfile = Lockfile::load_wanted_from_dir(&deploy_dir).unwrap().unwrap();
    let importer = deploy_lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY).unwrap();
    let dependencies = importer.dependencies.as_ref().expect("deploy importer dependencies");
    let lib_name: PkgName = "lib".parse().unwrap();
    let lib_version =
        dependencies.get(&lib_name).expect("deployed lib dependency").version.to_string();
    assert!(
        lib_version.starts_with("lib@file:"),
        "the dedicated deploy lockfile should rewrite the linked workspace dependency: {lib_version}",
    );

    let lib_real = fs::canonicalize(&lib_link).unwrap();
    assert!(
        lib_real.starts_with(&deploy_dir),
        "the deployed workspace dependency should stay inside {}: {}",
        deploy_dir.display(),
        lib_real.display(),
    );

    let lib_modules = lib_real.parent().expect("the deployed lib's node_modules");
    assert!(lib_modules.join("@pnpm.e2e/foo").exists());
    let nested_real = fs::canonicalize(lib_modules.join("nested")).unwrap();
    assert!(
        nested_real.starts_with(&deploy_dir),
        "a transitively linked workspace dependency should stay inside {}: {}",
        deploy_dir.display(),
        nested_real.display(),
    );

    fs::copy(&npmrc_path, deploy_dir.join(".npmrc")).unwrap();
    fs::remove_dir_all(deploy_dir.join("node_modules")).unwrap();
    pacquet_cmd(&deploy_dir).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert!(deploy_dir.join("node_modules/lib/index.js").is_file());
    let dangling = dangling_links(&deploy_dir.join("node_modules"));
    assert!(
        dangling.is_empty(),
        "installing the deployed lockfile must not create dangling symlinks: {dangling:#?}",
    );

    drop((root, mock_instance));
}

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

/// Adds a second workspace project that pulls in a different version of the
/// peer, so the deployed graph offers two candidates for `lib`'s binding.
fn write_ambiguous_peer_workspace(workspace: &Path) {
    write_peer_workspace(workspace);
    write_project(
        workspace,
        "other",
        &serde_json::json!({
            "name": "other",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "@pnpm.e2e/peer-a": "1.0.1" },
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
                "@pnpm.e2e/peer-a": "1.0.0",
            },
        }),
    );
}

/// `lib`'s peer is satisfied in the workspace by its own devDependencies, which
/// a production deploy leaves behind — so the deployed snapshot reaches the
/// binding step with that peer still unresolved.
fn write_peer_workspace(workspace: &Path) {
    let mut workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap();
    workspace_yaml.push_str("packages:\n  - 'packages/*'\nautoInstallPeers: false\n");
    workspace_yaml.push_str("injectWorkspacePackages: false\n");
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
            "dependencies": { "lib": "workspace:*", "@pnpm.e2e/peer-a": "1.0.0" },
        }),
    );
    write_project(
        workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "peerDependencies": { "@pnpm.e2e/peer-a": "*" },
            "devDependencies": { "@pnpm.e2e/peer-a": "1.0.1" },
        }),
    );
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

    pacquet
        .with_env("PNPM_CONFIG_LOCKFILE", "false")
        .with_args(["--filter", "app", "deploy", "--legacy", "--prod", "legacy-deploy-no-lockfile"])
        .assert()
        .success();

    let deploy_dir = workspace.join("legacy-deploy-no-lockfile");
    assert!(deploy_dir.join("node_modules/lib").exists());
    assert!(!deploy_dir.join("legacy-deploy-no-lockfile/node_modules").exists());
    assert!(!deploy_dir.join("pnpm-lock.yaml").exists());

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

fn assert_workspace_lockfile_untouched(workspace: &Path, before: &str) {
    let after = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    assert_eq!(after, before, "deploy must not rewrite the workspace lockfile");
}

fn pacquet_cmd(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// Every link under `dir`, at any depth, whose target does not exist.
fn dangling_links(dir: &Path) -> Vec<PathBuf> {
    let mut dangling = Vec::new();
    let mut queue = vec![dir.to_path_buf()];
    while let Some(current) = queue.pop() {
        for entry in fs::read_dir(&current).expect("read a deployed directory") {
            let path = entry.expect("read a deployed entry").path();
            if is_symlink_or_junction(&path).expect("stat a deployed entry") {
                if !path.exists() {
                    dangling.push(path);
                }
            } else if path.is_dir() {
                queue.push(path);
            }
        }
    }
    dangling
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

/// `deploy` copies a project through the directory fetcher's packlist
/// mode, so the project's `files` field is what decides the deployed
/// file set.
#[test]
fn deployed_files_field_does_not_match_at_depth() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);
    write_project(
        &workspace,
        "packs-its-own-src",
        &serde_json::json!({
            "name": "packs-its-own-src",
            "version": "1.0.0",
            "main": "src/index.js",
            "files": ["src"],
        }),
    );
    let project = workspace.join("packages/packs-its-own-src");
    for path in ["src/index.js", "example/src/App.js"] {
        let file = project.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "").unwrap();
    }

    pacquet.with_arg("install").assert().success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "packs-its-own-src", "deploy", "deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("deploy");
    assert!(deploy_dir.join("src/index.js").exists(), "the published src is deployed");
    assert!(!deploy_dir.join("example").exists(), "the example app is not deployed");

    drop((root, mock_instance));
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
