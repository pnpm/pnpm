use super::{super::workspace_yaml::append_workspace_yaml_key, append_order_script};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

const DEP: &str = "@pnpm.e2e/hello-world-js-bin";

fn record_stage_script(stage: &str) -> String {
    format!(
        r#"node -e "const fs=require('fs');fs.appendFileSync('order.txt','{stage} dep='+fs.existsSync('node_modules/{DEP}')+'\n')""#,
    )
}

fn project_with_uninstall_scripts() -> serde_json::Value {
    serde_json::json!({
        "name": "project-with-uninstall-scripts",
        "version": "1.0.0",
        "dependencies": { DEP: "1.0.0" },
        "scripts": {
            "preinstall": append_order_script("preinstall"),
            "install": append_order_script("install"),
            "postinstall": append_order_script("postinstall"),
            "prepare": append_order_script("prepare"),
            "preuninstall": record_stage_script("preuninstall"),
            "uninstall": record_stage_script("uninstall"),
            "postuninstall": record_stage_script("postuninstall"),
        },
    })
}

fn install_project(workspace: &Path, manifest: &serde_json::Value) {
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_arg("install")
        .assert()
        .success();
    fs::remove_file(workspace.join("order.txt")).expect("install stages ran");
    assert!(
        workspace
            .join("node_modules")
            .join(DEP)
            .exists(),
    );
}

fn manifest_lists_dep(workspace: &Path) -> bool {
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    manifest["dependencies"].get(DEP).is_some()
}

fn recorded_stages(workspace: &Path) -> Vec<String> {
    fs::read_to_string(workspace.join("order.txt"))
        .expect("read order.txt")
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn runs_the_uninstall_stages_around_unlinking() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    install_project(&workspace, &project_with_uninstall_scripts());

    pacquet
        .with_args(["remove", DEP])
        .assert()
        .success();

    assert_eq!(
        recorded_stages(&workspace),
        ["preuninstall dep=true", "uninstall dep=true", "postuninstall dep=false"],
    );
    assert!(!manifest_lists_dep(&workspace));

    drop((root, mock_instance));
}

#[test]
fn failing_uninstall_stage_aborts_the_removal() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let mut manifest = project_with_uninstall_scripts();
    manifest["bin"] = serde_json::json!({ "test-bin": "./bin.js" });
    manifest["scripts"]["uninstall"] = r#"node -e "process.exit(1)""#.into();
    fs::write(workspace.join("bin.js"), "console.log('bin');").expect("write bin.js");
    install_project(&workspace, &manifest);
    let bin_dir = workspace.join("node_modules").join(".bin");
    if bin_dir.exists() {
        fs::remove_dir_all(&bin_dir).expect("remove node_modules/.bin");
    }
    let lockfile_before =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");

    pacquet
        .with_args(["remove", DEP])
        .assert()
        .failure();

    assert_eq!(recorded_stages(&workspace), ["preuninstall dep=true"]);
    assert!(!bin_dir.exists());
    assert!(manifest_lists_dep(&workspace));
    assert_eq!(
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml"),
        lockfile_before,
    );
    assert!(
        workspace
            .join("node_modules")
            .join(DEP)
            .exists(),
    );

    drop((root, mock_instance));
}

#[test]
fn ignore_scripts_skips_the_uninstall_stages() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    install_project(&workspace, &project_with_uninstall_scripts());
    append_workspace_yaml_key(&workspace, "ignoreScripts", true);

    pacquet
        .with_args(["remove", DEP])
        .assert()
        .success();

    assert!(!workspace.join("order.txt").exists(), "no uninstall stage may run");
    assert!(!manifest_lists_dep(&workspace));
    assert!(
        !workspace
            .join("node_modules")
            .join(DEP)
            .exists(),
    );

    drop((root, mock_instance));
}

#[test]
fn lockfile_only_skips_the_uninstall_stages() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    install_project(&workspace, &project_with_uninstall_scripts());

    pacquet
        .with_args(["remove", "--lockfile-only", DEP])
        .assert()
        .success();

    assert!(!workspace.join("order.txt").exists(), "no uninstall stage may run");
    assert!(!manifest_lists_dep(&workspace));
    assert!(
        workspace
            .join("node_modules")
            .join(DEP)
            .exists(),
    );

    drop((root, mock_instance));
}

#[test]
fn virtual_store_only_skips_the_uninstall_stages() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "virtualStoreOnly", true);
    fs::write(workspace.join("package.json"), project_with_uninstall_scripts().to_string())
        .expect("write package.json");
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_arg("install")
        .assert()
        .success();

    pacquet
        .with_args(["remove", DEP])
        .assert()
        .success();

    assert!(!workspace.join("order.txt").exists(), "no uninstall stage may run");
    assert!(!manifest_lists_dep(&workspace));

    drop((root, mock_instance));
}

#[test]
fn recursive_remove_runs_the_stages_only_where_the_dependency_was_listed() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "packages", r#"["project-*"]"#);
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0" }).to_string(),
    )
    .expect("write root package.json");
    for (name, lists_dep) in [("project-a", true), ("project-b", false)] {
        let mut manifest = project_with_uninstall_scripts();
        manifest["name"] = name.into();
        if !lists_dep {
            manifest["dependencies"] = serde_json::json!({});
        }
        let project_dir = workspace.join(name);
        fs::create_dir(&project_dir).expect("create project directory");
        fs::write(project_dir.join("package.json"), manifest.to_string())
            .expect("write project package.json");
    }
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_arg("install")
        .assert()
        .success();
    for name in ["project-a", "project-b"] {
        fs::remove_file(workspace.join(name).join("order.txt")).expect("install stages ran");
    }

    pacquet
        .with_args(["remove", "-r", DEP])
        .assert()
        .success();

    assert_eq!(
        recorded_stages(&workspace.join("project-a")),
        ["preuninstall dep=true", "uninstall dep=true", "postuninstall dep=false"],
    );
    assert!(
        !workspace
            .join("project-b")
            .join("order.txt")
            .exists(),
        "a project that never listed the dependency runs no uninstall stage",
    );
    assert!(!manifest_lists_dep(&workspace.join("project-a")));

    drop((root, mock_instance));
}

#[test]
fn recursive_remove_of_a_peer_only_dependency_runs_the_stages() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "packages", r#"["project-*"]"#);
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0" }).to_string(),
    )
    .expect("write root package.json");
    let mut manifest = project_with_uninstall_scripts();
    manifest["name"] = "project-a".into();
    manifest["dependencies"] = serde_json::json!({});
    manifest["peerDependencies"] = serde_json::json!({ DEP: "*" });
    let project_dir = workspace.join("project-a");
    fs::create_dir(&project_dir).expect("create project directory");
    fs::write(project_dir.join("package.json"), manifest.to_string())
        .expect("write project package.json");
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_arg("install")
        .assert()
        .success();
    fs::remove_file(project_dir.join("order.txt")).expect("install stages ran");

    pacquet
        .with_args(["remove", "-r", DEP])
        .assert()
        .success();

    let stages: Vec<String> = recorded_stages(&project_dir)
        .iter()
        .map(|line| {
            line.split(' ')
                .next()
                .expect("stage name")
                .to_string()
        })
        .collect();
    assert_eq!(stages, ["preuninstall", "uninstall", "postuninstall"]);

    drop((root, mock_instance));
}
#[test]
fn recursive_save_dev_removal_runs_stages_for_removed_peer_entries() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "packages", r#"["project-*"]"#);
    fs::write(workspace.join("package.json"), r#"{"name":"root"}"#).expect("write root manifest");
    for (name, is_peer) in [("project-a", false), ("project-b", true)] {
        let mut manifest = project_with_uninstall_scripts();
        manifest["name"] = name.into();
        manifest["dependencies"] = serde_json::json!({});
        manifest["devDependencies"] =
            if is_peer { serde_json::json!({}) } else { serde_json::json!({ DEP: "1.0.0" }) };
        if is_peer {
            manifest["peerDependencies"] = serde_json::json!({ DEP: "*" });
        }
        let project_dir = workspace.join(name);
        fs::create_dir(&project_dir).expect("create project directory");
        fs::write(project_dir.join("package.json"), manifest.to_string())
            .expect("write project manifest");
    }
    Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&workspace)
        .with_arg("install")
        .assert()
        .success();
    for name in ["project-a", "project-b"] {
        fs::remove_file(workspace.join(name).join("order.txt")).expect("install stages ran");
    }
    pacquet
        .with_args(["remove", "-r", "--save-dev", DEP])
        .assert()
        .success();
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("project-b/package.json")).expect("read manifest"),
    )
    .expect("parse manifest");
    assert!(manifest.get("peerDependencies").is_none());
    for name in ["project-a", "project-b"] {
        let stages: Vec<String> = recorded_stages(&workspace.join(name))
            .iter()
            .map(|line| {
                line.split(' ')
                    .next()
                    .expect("stage name")
                    .to_string()
            })
            .collect();
        assert_eq!(stages, ["preuninstall", "uninstall", "postuninstall"]);
    }
    drop((root, mock_instance));
}
