use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, assert_frozen_outdated, fs,
    is_symlink_or_junction, pacquet_at, two_project_workspace,
};
use assert_cmd::assert::OutputAssertExt;

#[test]
fn changed_workspace_importer_invalidates_lockfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } = two_project_workspace(
        &serde_json::json!({ "name": "pkg-a", "version": "1.0.0" }),
        &serde_json::json!({ "name": "pkg-b", "version": "1.0.0" }),
    );
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::write(
        workspace.join("pkg-a/package.json"),
        serde_json::json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "dependencies": { "pkg-b": "workspace:*" },
        })
        .to_string(),
    )
    .expect("update pkg-a/package.json");

    assert_frozen_outdated(&workspace);

    pacquet_at(&workspace).with_arg("install").assert().success();
    let linked_pkg = workspace.join("pkg-a/node_modules/pkg-b");
    assert!(
        is_symlink_or_junction(&linked_pkg).expect("query pkg-b link"),
        "normal install did not link the dependency added to pkg-a",
    );
    assert!(linked_pkg.join("package.json").exists(), "pkg-b link is dangling");

    drop((root, mock_instance));
}

#[test]
fn changed_registry_specifier_in_workspace_importer_invalidates_lockfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } = two_project_workspace(
        &serde_json::json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
        }),
        &serde_json::json!({
            "name": "pkg-b",
            "version": "1.0.0",
            "dependencies": { "is-negative": "1.0.0" },
        }),
    );
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet_at(&workspace).with_arg("install").assert().success();
    fs::write(
        workspace.join("pkg-a/package.json"),
        serde_json::json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "dependencies": { "is-positive": "3.1.0" },
        })
        .to_string(),
    )
    .expect("update pkg-a/package.json");

    assert_frozen_outdated(&workspace);
    pacquet_at(&workspace).with_arg("install").assert().success();

    drop((root, mock_instance));
}

#[test]
fn workspace_importer_dependencies_meta_is_checked() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } = two_project_workspace(
        &serde_json::json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "dependencies": { "pkg-b": "workspace:*" },
            "dependenciesMeta": { "pkg-b": { "injected": true } },
        }),
        &serde_json::json!({ "name": "pkg-b", "version": "1.0.0" }),
    );
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet_at(&workspace).with_arg("install").assert().success();
    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    fs::write(
        workspace.join("pkg-a/package.json"),
        serde_json::json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "dependencies": { "pkg-b": "workspace:*" },
        })
        .to_string(),
    )
    .expect("remove pkg-a dependenciesMeta");

    assert_frozen_outdated(&workspace);

    drop((root, mock_instance));
}

#[test]
fn missing_workspace_importer_is_not_accepted_by_frozen_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } = two_project_workspace(
        &serde_json::json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
        }),
        &serde_json::json!({ "name": "pkg-b", "version": "1.0.0" }),
    );
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet_at(&workspace).with_arg("install").assert().success();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let mut lockfile: pnpm_lockfile::Lockfile =
        serde_saphyr::from_str(&fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml"))
            .expect("parse pnpm-lock.yaml");
    lockfile.importers.remove("pkg-a").expect("pkg-a importer exists");
    lockfile.save_to_path(&lockfile_path).expect("save lockfile without pkg-a importer");

    let output = pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .expect("run frozen install");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "frozen install accepted a missing importer");
    assert!(
        stderr.contains("ERR_PNPM_PACKAGE_MANAGER_NO_IMPORTER") && stderr.contains("pkg-a"),
        "missing importer returned the wrong error\nstderr:\n{stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn normal_install_accepts_missing_dependency_free_workspace_importer() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } = two_project_workspace(
        &serde_json::json!({ "name": "pkg-a", "version": "1.0.0" }),
        &serde_json::json!({ "name": "pkg-b", "version": "1.0.0" }),
    );
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet_at(&workspace).with_arg("install").assert().success();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let mut lockfile: pnpm_lockfile::Lockfile =
        serde_saphyr::from_str(&fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml"))
            .expect("parse pnpm-lock.yaml");
    lockfile.importers.remove("pkg-b").expect("pkg-b importer exists");
    lockfile.save_to_path(&lockfile_path).expect("save lockfile without pkg-b importer");

    pacquet_at(&workspace).with_arg("install").assert().success();
    let retained: pnpm_lockfile::Lockfile = serde_saphyr::from_str(
        &fs::read_to_string(&lockfile_path).expect("read retained pnpm-lock.yaml"),
    )
    .expect("parse retained pnpm-lock.yaml");
    assert!(
        !retained.importers.contains_key("pkg-b"),
        "dependency-free pkg-b should not force lockfile regeneration",
    );

    drop((root, mock_instance));
}

#[test]
fn normal_install_accepts_missing_importer_with_only_ignored_optional_dependencies() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } = two_project_workspace(
        &serde_json::json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "optionalDependencies": { "is-positive": "1.0.0" },
        }),
        &serde_json::json!({ "name": "pkg-b", "version": "1.0.0" }),
    );
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    workspace_yaml.push_str("ignoredOptionalDependencies:\n  - is-positive\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write ignored optional config");

    pacquet_at(&workspace).with_arg("install").assert().success();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let mut lockfile: pnpm_lockfile::Lockfile =
        serde_saphyr::from_str(&fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml"))
            .expect("parse pnpm-lock.yaml");
    lockfile.importers.remove("pkg-a").expect("pkg-a importer exists");
    lockfile.save_to_path(&lockfile_path).expect("save lockfile without pkg-a importer");

    pacquet_at(&workspace).with_arg("install").assert().success();
    let retained: pnpm_lockfile::Lockfile = serde_saphyr::from_str(
        &fs::read_to_string(&lockfile_path).expect("read retained pnpm-lock.yaml"),
    )
    .expect("parse retained pnpm-lock.yaml");
    assert!(
        !retained.importers.contains_key("pkg-a"),
        "ignored optional dependency should not force lockfile regeneration",
    );

    drop((root, mock_instance));
}
