use super::{CommandTempCwd, seed_package};
use assert_cmd::cargo::CommandCargoExt;
use command_extra::CommandExtra;
use pnpm_testing_utils::command_env::CommandTestExt;
use std::{fs, io::ErrorKind, path::Path, process::Command};

fn setup_local_dependency_project(workspace: &Path, store_dir: &Path) {
    let dependency = workspace.join("dependency");
    fs::create_dir_all(&dependency).expect("create the local dependency");
    fs::write(dependency.join("package.json"), r#"{"name":"local-dependency","version":"1.0.0"}"#)
        .expect("write the local dependency manifest");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"demo-app","version":"1.0.0","dependencies":{"local-dependency":"file:dependency"}}"#,
    )
    .expect("write the project manifest");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("storeDir: {}\n", serde_json::to_string(store_dir).expect("quote storeDir")),
    )
    .expect("write pnpm-workspace.yaml");
}

#[test]
fn clean_and_purge_do_not_delete_files_in_external_modules_dir() {
    for command in ["clean", "purge"] {
        for use_absolute in [false, true] {
            let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
            let external = root.path().join("external-target");
            fs::create_dir_all(&external).expect("create external target");
            fs::write(external.join("keep.txt"), "keep").expect("write keep.txt");
            seed_package(&external, "lodash");

            let modules_dir_setting = if use_absolute {
                serde_json::to_string(&external).expect("quote external path")
            } else {
                "../external-target".to_string()
            };
            fs::write(
                workspace.join("pnpm-workspace.yaml"),
                format!("modulesDir: {modules_dir_setting}\n"),
            )
            .expect("write pnpm-workspace.yaml");
            fs::write(workspace.join("package.json"), r#"{"name":"demo-app","version":"1.0.0"}"#)
                .expect("write package.json");

            let output = pacquet
                .with_args([command])
                .output()
                .expect("run command");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(output.status.success(), "{command} (abs={use_absolute}) failed: {stderr}");

            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                !stdout.contains("Removing"),
                "{command} (abs={use_absolute}) must not report removals: {stdout}",
            );
            assert!(
                external.join("keep.txt").exists(),
                "keep.txt must remain untouched for {command} (abs={use_absolute})",
            );
            assert!(
                external.join("lodash/package.json").exists(),
                "external package must remain untouched for {command} (abs={use_absolute})",
            );

            drop(root);
        }
    }
}

#[test]
fn clean_rejects_self_and_symlink_parent_and_sibling_modules_dir_escapes() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let outside = root.path().join("outside");
    let outside_sub = outside.join("sub");
    let outside_nm = outside_sub.join("nm");
    fs::create_dir_all(&outside_nm).expect("create outside/sub/nm");
    fs::write(outside_nm.join("keep.txt"), "keep").expect("write outside keep.txt");
    pnpm_fs::symlink_dir(&outside_sub, &workspace.join("escape"))
        .expect("link escape -> outside/sub");

    let keep_dir = workspace.join("keep_dir");
    fs::create_dir_all(&keep_dir).expect("create keep_dir");
    fs::write(keep_dir.join("keep.txt"), "keep").expect("write keep_dir/keep.txt");
    fs::write(workspace.join("package.json"), r#"{"name":"demo-app","version":"1.0.0"}"#)
        .expect("write package.json");

    let mut escape_cases = vec![".", ". ", ".. ", "escape/nm", "escape/../keep_dir"];
    if cfg!(windows) {
        escape_cases.extend([r"escape\nm", r"escape\..\keep_dir"]);
    }
    for modules_dir in escape_cases {
        let quoted_modules_dir = serde_json::to_string(modules_dir).expect("quote modulesDir");
        fs::write(
            workspace.join("pnpm-workspace.yaml"),
            format!("modulesDir: {quoted_modules_dir}\n"),
        )
        .expect("write pnpm-workspace.yaml");
        let output = Command::cargo_bin("pnpm")
            .expect("find pnpm binary")
            .with_current_dir(&workspace)
            .without_ambient_pnpm_config()
            .with_args(["clean"])
            .output()
            .expect("run pnpm clean");
        assert!(output.status.success(), "clean failed for {modules_dir}: {output:?}");
        assert!(workspace.join("package.json").exists(), "package.json removed for {modules_dir}");
        assert!(outside_nm.join("keep.txt").exists(), "outside file removed for {modules_dir}");
        assert!(keep_dir.join("keep.txt").exists(), "keep_dir file removed for {modules_dir}");
    }

    // In a workspace, `modulesDir: ../pkg2` resolved against `pkg1` stays
    // inside `workspace` but escapes `pkg1` into sibling package `pkg2`.
    let pkg1 = workspace.join("packages/pkg1");
    let pkg2 = workspace.join("packages/pkg2");
    fs::create_dir_all(&pkg1).expect("create pkg1");
    fs::create_dir_all(&pkg2).expect("create pkg2");
    fs::write(pkg1.join("package.json"), r#"{"name":"pkg1"}"#).expect("write pkg1 manifest");
    fs::write(pkg2.join("package.json"), r#"{"name":"pkg2"}"#).expect("write pkg2 manifest");
    fs::write(pkg2.join("keep.txt"), "keep").expect("write pkg2/keep.txt");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nmodulesDir: ../pkg2\n",
    )
    .expect("write workspace manifest");

    let output = Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_args(["clean"])
        .output()
        .expect("run pnpm clean in workspace");
    assert!(output.status.success(), "{output:?}");
    assert!(pkg2.join("package.json").exists(), "sibling pkg2 manifest must survive");
    assert!(pkg2.join("keep.txt").exists(), "sibling pkg2 keep.txt must survive");

    drop(root);
}

#[test]
fn ci_does_not_delete_files_in_external_modules_dir() {
    for use_absolute in [false, true] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        let store_dir = root.path().join("store");
        setup_local_dependency_project(&workspace, &store_dir);

        let install_output = pacquet
            .with_args(["install", "--lockfile-only"])
            .output()
            .expect("generate lockfile");
        assert!(install_output.status.success(), "{install_output:?}");

        let external = root.path().join("external-target");
        fs::create_dir_all(&external).expect("create external target");
        fs::write(external.join("keep.txt"), "keep").expect("write keep.txt");
        seed_package(&external, "existing-pkg");

        let modules_dir_setting = if use_absolute {
            serde_json::to_string(&external).expect("quote external path")
        } else {
            "../external-target".to_string()
        };
        fs::write(
            workspace.join("pnpm-workspace.yaml"),
            format!(
                "storeDir: {}\nmodulesDir: {modules_dir_setting}\n",
                serde_json::to_string(&store_dir).expect("quote storeDir"),
            ),
        )
        .expect("write pnpm-workspace.yaml");

        let ci_output = Command::cargo_bin("pnpm")
            .expect("find pnpm binary")
            .with_current_dir(&workspace)
            .without_ambient_pnpm_config()
            .with_args(["ci"])
            .output()
            .expect("run pnpm ci");
        let stderr = String::from_utf8_lossy(&ci_output.stderr);
        assert!(ci_output.status.success(), "pnpm ci (abs={use_absolute}) failed: {stderr}");

        assert!(
            external.join("keep.txt").exists(),
            "pnpm ci (abs={use_absolute}) must not delete external keep.txt",
        );
        assert!(
            external.join("existing-pkg/package.json").exists(),
            "pnpm ci (abs={use_absolute}) must not delete existing external package",
        );
        assert!(
            external.join("local-dependency/package.json").exists(),
            "pnpm ci (abs={use_absolute}) should complete install into configured modulesDir",
        );

        drop(root);
    }
}

#[test]
fn clean_and_purge_unlink_symlinked_modules_dir_without_touching_target() {
    for command in ["clean", "purge"] {
        for custom_leaf in [None, Some("vendor")] {
            let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
            let external = root.path().join("external-target");
            fs::create_dir_all(external.join(".pnpm")).expect("create external .pnpm");
            fs::write(external.join("keep.txt"), "keep").expect("write keep.txt");
            seed_package(&external, "lodash");

            let leaf_name = custom_leaf.unwrap_or("node_modules");
            if let Some(custom) = custom_leaf {
                fs::write(workspace.join("pnpm-workspace.yaml"), format!("modulesDir: {custom}\n"))
                    .expect("write pnpm-workspace.yaml");
            }
            fs::write(workspace.join("package.json"), r#"{"name":"demo-app","version":"1.0.0"}"#)
                .expect("write package.json");

            let link_path = workspace.join(leaf_name);
            pnpm_fs::symlink_dir(&external, &link_path).expect("link modules dir to external");

            let output = pacquet
                .with_args([command])
                .output()
                .expect("run command");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(output.status.success(), "{command} ({leaf_name}) failed: {stderr}");

            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                stdout.contains(&format!("Removing {leaf_name}")),
                "expected Removing {leaf_name}: {stdout}",
            );

            let link_meta = fs::symlink_metadata(&link_path);
            assert!(
                matches!(&link_meta, Err(err) if err.kind() == ErrorKind::NotFound),
                "symlink {leaf_name} must be unlinked, got {link_meta:?}",
            );
            assert!(external.join("keep.txt").exists(), "external keep.txt must survive");
            assert!(external.join(".pnpm").exists(), "external .pnpm must survive");
            assert!(external.join("lodash/package.json").exists(), "external package must survive");

            drop(root);
        }
    }
}

#[test]
fn clean_unlinks_dangling_node_modules_symlink() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"demo-app","version":"1.0.0"}"#)
        .expect("write package.json");
    let missing_target = root.path().join("missing-target");
    fs::create_dir_all(&missing_target).expect("create target before unlinking it");
    let link_path = workspace.join("node_modules");
    pnpm_fs::symlink_dir(&missing_target, &link_path).expect("create link");
    fs::remove_dir_all(&missing_target).expect("make link dangle");

    let output = pacquet
        .with_args(["clean"])
        .output()
        .expect("run clean");
    assert!(output.status.success(), "{output:?}");
    assert!(
        matches!(fs::symlink_metadata(&link_path), Err(err) if err.kind() == ErrorKind::NotFound),
        "dangling node_modules link should be removed",
    );

    drop(root);
}

#[test]
fn ci_unlinks_symlinked_node_modules_without_deleting_target_and_reinstalls() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    setup_local_dependency_project(&workspace, &store_dir);

    let install_output = pacquet
        .with_args(["install", "--lockfile-only"])
        .output()
        .expect("generate lockfile");
    assert!(install_output.status.success(), "{install_output:?}");

    let external = root.path().join("external-target");
    fs::create_dir_all(external.join(".pnpm")).expect("create external .pnpm");
    fs::write(external.join("keep.txt"), "keep").expect("write keep.txt");
    seed_package(&external, "lodash");

    let node_modules = workspace.join("node_modules");
    pnpm_fs::symlink_dir(&external, &node_modules).expect("link node_modules -> external");

    let ci_output = Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_args(["ci"])
        .output()
        .expect("run pnpm ci");
    assert!(ci_output.status.success(), "pnpm ci should succeed: {ci_output:?}");

    assert!(external.join("keep.txt").exists(), "external keep.txt must survive pnpm ci");
    assert!(external.join(".pnpm").exists(), "external .pnpm must survive pnpm ci");
    assert!(external.join("lodash/package.json").exists(), "external package must survive pnpm ci");

    let nm_meta = fs::symlink_metadata(&node_modules).expect("node_modules recreated");
    assert!(
        !pnpm_fs::is_symlink_metadata(&nm_meta) && nm_meta.is_dir(),
        "node_modules should be a real directory after pnpm ci",
    );
    assert!(
        node_modules.join("local-dependency/package.json").exists(),
        "dependency should be installed in fresh node_modules",
    );

    drop(root);
}

#[test]
fn clean_keeps_project_root_and_rejects_symlinked_parent_virtual_store_and_dir_lockfile() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let outside_store = root.path().join("outside/store");
    fs::create_dir_all(&outside_store).expect("create outside/store");
    fs::write(outside_store.join("keep.txt"), "keep").expect("write outside store keep.txt");
    pnpm_fs::symlink_dir(
        outside_store.parent().expect("outside parent"),
        &workspace.join("escape"),
    )
    .expect("link escape -> outside");
    fs::write(workspace.join("package.json"), r#"{"name":"demo-app","version":"1.0.0"}"#)
        .expect("write package.json");

    let lockfile_dir = workspace.join("pnpm-lock.yaml");
    fs::create_dir_all(&lockfile_dir).expect("create directory named pnpm-lock.yaml");
    fs::write(lockfile_dir.join("keep.txt"), "keep").expect("write keep.txt inside lockfile dir");

    for virtual_store_dir in [".", ". ", ".. ", "escape/store"] {
        let quoted_store_dir =
            serde_json::to_string(virtual_store_dir).expect("quote virtualStoreDir");
        fs::write(
            workspace.join("pnpm-workspace.yaml"),
            format!("virtualStoreDir: {quoted_store_dir}\n"),
        )
        .expect("write pnpm-workspace.yaml");
        let output = Command::cargo_bin("pnpm")
            .expect("find pnpm binary")
            .with_current_dir(&workspace)
            .without_ambient_pnpm_config()
            .with_args(["clean", "--lockfile"])
            .output()
            .expect("run pnpm clean --lockfile");
        assert!(output.status.success(), "clean failed for {virtual_store_dir}: {output:?}");
        assert!(workspace.join("package.json").exists(), "package.json removed");
        assert!(outside_store.join("keep.txt").exists(), "outside store file removed");
        assert!(
            lockfile_dir.join("keep.txt").exists(),
            "real directory named pnpm-lock.yaml must not be deleted",
        );
    }

    drop(root);
}
