use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, Lockfile, Path, dangling_links, fs,
    pack_pnpmfile_with_project, pacquet_cmd, write_peer_workspace, write_project,
    write_reachability_workspace, write_recording_pnpmfile, write_workspace,
};
use assert_cmd::assert::OutputAssertExt;

/// Post-Deploy Self-Containment Oracle:
/// 1. `pnpm --dir <deploy_dir> --config.verify-deps-before-run=error run start`
///    passes immediately after `pnpm deploy` without re-installing.
/// 2. After removing `deploy_dir/node_modules`,
///    `pnpm --dir <deploy_dir> install --frozen-lockfile` succeeds with no
///    dangling symlinks and `verifyDepsBeforeRun: error` passes again.
fn assert_deployed_project_is_self_contained(deploy_dir: &Path, npmrc_path: &Path) {
    pacquet_cmd(deploy_dir)
        .with_args(["--config.verify-deps-before-run=error", "run", "start"])
        .assert()
        .success();

    fs::copy(npmrc_path, deploy_dir.join(".npmrc")).unwrap();
    fs::remove_dir_all(deploy_dir.join("node_modules")).unwrap();
    pacquet_cmd(deploy_dir)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    let dangling = dangling_links(&deploy_dir.join("node_modules"));
    assert!(
        dangling.is_empty(),
        "post-deploy frozen install must not leave dangling symlinks: {dangling:#?}",
    );

    pacquet_cmd(deploy_dir)
        .with_args(["--config.verify-deps-before-run=error", "run", "start"])
        .assert()
        .success();
}

fn enable_start_script(project_dir: &Path) {
    let manifest_path = project_dir.join("package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["scripts"] = serde_json::json!({ "start": "node index.js" });
    fs::write(manifest_path, manifest.to_string()).unwrap();
}

#[test]
fn deployed_project_is_self_contained_when_pnpmfile_is_packed_and_source_uses_hooks() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    fs::write(
        workspace.join(".pnpmfile.mjs"),
        r"export const hooks = {
  readPackage (pkg) {
    if (pkg.name === 'lib') {
      pkg.dependencies = { ...pkg.dependencies, '@pnpm.e2e/foo': '100.0.0' }
    }
    return pkg
  },
}
",
    )
    .unwrap();
    let project_dir = workspace.join("packages/app");
    enable_start_script(&project_dir);
    write_recording_pnpmfile(&project_dir);
    pack_pnpmfile_with_project(&project_dir);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let deploy_dir = dunce::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", deploy_dir.to_str().unwrap()])
        .assert()
        .success();

    assert!(deploy_dir.join(".pnpmfile.mjs").exists());
    assert_deployed_project_is_self_contained(&deploy_dir, &npmrc_path);
    assert!(
        deploy_dir.join("node_modules/.pnpm").exists()
            && !deploy_dir.join(super::PNPMFILE_SENTINEL).exists(),
        "post-deploy commands must honour ignorePnpmfile: true in the deployed workspace manifest",
    );

    drop((root, mock_instance));
}

#[test]
fn deployed_project_is_self_contained_with_include_resolution_settings() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    write_reachability_workspace(&workspace);
    enable_start_script(&workspace.join("packages/app"));
    let workspace_manifest_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_manifest = fs::read_to_string(&workspace_manifest_path).unwrap();
    workspace_manifest.push_str(concat!(
        "autoDedupe: true\n",
        "dedupeInjectedDeps: true\n",
        "dedupePeerDependents: true\n",
        "linkWorkspacePackages: deep\n",
        "lockfile:\n  includeResolutionSettings: true\n",
    ));
    fs::write(workspace_manifest_path, workspace_manifest).unwrap();

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let deploy_dir = dunce::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", deploy_dir.to_str().unwrap()])
        .assert()
        .success();

    assert_deployed_project_is_self_contained(&deploy_dir, &npmrc_path);

    drop((root, mock_instance));
}

#[test]
fn deployed_project_is_self_contained_when_workspace_deps_use_catalogs_overrides_and_extensions() {
    for inject_workspace_packages in [false, true] {
        let CommandTempCwd {
            pacquet,
            root,
            workspace,
            npmrc_info,
            ..
        } = CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
        write_workspace(&workspace, inject_workspace_packages);
        enable_start_script(&workspace.join("packages/app"));
        write_project(
            &workspace,
            "lib",
            &serde_json::json!({
                "name": "lib",
                "version": "1.0.0",
                "files": ["index.js"],
                "dependencies": {
                    "@pnpm.e2e/foo": "catalog:",
                    "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                },
            }),
        );
        let workspace_manifest_path = workspace.join("pnpm-workspace.yaml");
        let mut workspace_manifest = fs::read_to_string(&workspace_manifest_path).unwrap();
        workspace_manifest.push_str(concat!(
            "catalog:\n  '@pnpm.e2e/foo': 100.0.0\n",
            "overrides:\n  '@pnpm.e2e/dep-of-pkg-with-1-dep': 100.1.0\n",
            "packageExtensions:\n  lib:\n    dependencies:\n      '@pnpm.e2e/bar': 100.0.0\n",
        ));
        fs::write(workspace_manifest_path, workspace_manifest).unwrap();

        pacquet
            .with_arg("install")
            .assert()
            .success();
        let deploy_dir = dunce::canonicalize(root.path()).unwrap().join("deploy");
        pacquet_cmd(&workspace)
            .with_args(["--filter", "app", "deploy", "--prod", deploy_dir.to_str().unwrap()])
            .assert()
            .success();

        assert_deployed_project_is_self_contained(&deploy_dir, &npmrc_path);

        drop((root, mock_instance));
    }
}

#[test]
fn deployed_project_is_self_contained_when_workspace_dep_binds_singleton_peers() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    write_peer_workspace(&workspace);
    enable_start_script(&workspace.join("packages/app"));

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let deploy_dir = dunce::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", deploy_dir.to_str().unwrap()])
        .assert()
        .success();

    assert_deployed_project_is_self_contained(&deploy_dir, &npmrc_path);

    drop((root, mock_instance));
}

#[test]
fn deploy_prunes_unused_patched_dependencies_and_remains_self_contained() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    write_reachability_workspace(&workspace);
    enable_start_script(&workspace.join("packages/app"));
    let patches_dir = workspace.join("patches");
    fs::create_dir_all(&patches_dir).unwrap();
    let foo_patch = concat!(
        "diff --git a/PATCH.txt b/PATCH.txt\n",
        "new file mode 100644\n",
        "index 0000000..1111111\n",
        "--- /dev/null\n",
        "+++ b/PATCH.txt\n",
        "@@ -0,0 +1 @@\n",
        "+patched\n",
    );
    fs::write(patches_dir.join("pkg-with-1-dep.patch"), foo_patch).unwrap();
    fs::write(patches_dir.join("bar.patch"), foo_patch).unwrap();
    let workspace_manifest_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_manifest = fs::read_to_string(&workspace_manifest_path).unwrap();
    workspace_manifest.push_str(concat!(
        "patchedDependencies:\n",
        "  '@pnpm.e2e/pkg-with-1-dep@100.0.0': patches/pkg-with-1-dep.patch\n",
        "  '@pnpm.e2e/bar@100.0.0': patches/bar.patch\n",
    ));
    fs::write(workspace_manifest_path, workspace_manifest).unwrap();

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let deploy_dir = dunce::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", deploy_dir.to_str().unwrap()])
        .assert()
        .success();

    let deploy_lockfile = Lockfile::load_wanted_from_dir(&deploy_dir).unwrap().unwrap();
    let patched = deploy_lockfile.patched_dependencies.expect(
        "applied patch should remain in the deploy lockfile",
    );
    assert_eq!(
        patched
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["@pnpm.e2e/pkg-with-1-dep@100.0.0"],
    );
    let deploy_workspace_manifest: serde_json::Value = serde_saphyr::from_str(
        &fs::read_to_string(deploy_dir.join("pnpm-workspace.yaml")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        deploy_workspace_manifest["patchedDependencies"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["@pnpm.e2e/pkg-with-1-dep@100.0.0"],
    );

    assert_deployed_project_is_self_contained(&deploy_dir, &npmrc_path);

    drop((root, mock_instance));
}
