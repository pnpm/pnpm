use crate::_utils::{append_workspace_yaml_key, enable_gvs_in_workspace_yaml};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

fn canonicalize(path: &Path) -> PathBuf {
    dunce::canonicalize(path).expect("canonicalize path")
}

/// <https://github.com/pnpm/pnpm/issues/6929>
#[test]
fn install_registers_the_project_in_the_store() {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    pacquet_at(&workspace)
        .with_args(["add", "is-positive@1.0.0"])
        .assert()
        .success();

    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir);
    let projects: Vec<_> = pnpm_store_dir::get_registered_projects(&store_dir)
        .expect("list registered projects")
        .iter()
        .map(|project| canonicalize(project))
        .collect();
    assert_eq!(projects, [canonicalize(&workspace)]);
}

#[test]
fn frozen_store_install_does_not_register_the_project() {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    pacquet_at(&workspace)
        .with_args(["add", "is-positive@1.0.0"])
        .assert()
        .success();
    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir);
    fs::remove_dir_all(store_dir.projects()).expect("clear the project registry");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile", "--frozen-store", "--offline"])
        .assert()
        .success();

    assert!(workspace.join("node_modules/is-positive").is_dir());
    assert!(!store_dir.projects().exists());
}

#[test]
fn up_to_date_frozen_store_install_does_not_register_the_project() {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    pacquet_at(&workspace)
        .with_args(["add", "is-positive@1.0.0"])
        .assert()
        .success();
    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir);
    fs::remove_dir_all(store_dir.projects()).expect("clear the project registry");

    pacquet_at(&workspace)
        .with_args(["install", "--frozen-store"])
        .assert()
        .success();

    assert!(!store_dir.projects().exists());
}

#[test]
fn up_to_date_no_frozen_store_install_registers_the_project() {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    pacquet_at(&workspace)
        .with_args(["add", "is-positive@1.0.0"])
        .assert()
        .success();
    append_workspace_yaml_key(&workspace, "frozenStore", true);
    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir);
    fs::remove_dir_all(store_dir.projects()).expect("clear the project registry");

    pacquet_at(&workspace)
        .with_args(["install", "--no-frozen-store"])
        .assert()
        .success();

    let projects: Vec<_> = pnpm_store_dir::get_registered_projects(&store_dir)
        .expect("list registered projects")
        .iter()
        .map(|project| canonicalize(project))
        .collect();
    assert_eq!(projects, [canonicalize(&workspace)]);
}

#[test]
fn repeat_install_registers_an_unregistered_project() {
    for args in [&["install"][..], &["install", "--filter=."]] {
        assert_repeat_install_registers_the_project(args);
    }
}

fn assert_repeat_install_registers_the_project(args: &[&str]) {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    pacquet_at(&workspace)
        .with_args(["add", "is-positive@1.0.0"])
        .assert()
        .success();
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir);
    fs::remove_dir_all(store_dir.projects()).expect("clear the project registry");

    let output = pacquet_at(&workspace)
        .with_args(args)
        .with_arg("--reporter=append-only")
        .output()
        .expect("run install");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{args:?}: {output:?}");
    assert!(stdout.contains("Already up to date"), "{args:?}: {stdout}");

    let projects: Vec<_> = pnpm_store_dir::get_registered_projects(&store_dir)
        .expect("list registered projects")
        .iter()
        .map(|project| canonicalize(project))
        .collect();
    assert_eq!(projects, [canonicalize(&workspace)], "{args:?}");
}

#[test]
fn resolve_only_install_does_not_register_the_project() {
    for args in [&["install", "--lockfile-only"][..], &["install", "--dry-run"]] {
        let CommandTempCwd {
            root: _root, workspace, npmrc_info, ..
        } = CommandTempCwd::init().add_mocked_registry();
        fs::write(workspace.join("package.json"), r#"{"dependencies":{"is-positive":"1.0.0"}}"#)
            .expect("write package.json");

        pacquet_at(&workspace)
            .with_args(args)
            .assert()
            .success();

        let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir);
        assert!(!store_dir.projects().exists(), "{args:?}");
    }
}

#[test]
fn repeat_install_registers_the_configured_lockfile_dir() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let lockfile_dir = root.path().join("lockfile-dir");
    fs::create_dir_all(&lockfile_dir).expect("create the lockfile dir");
    // The macOS temp dir sits behind the `/var` -> `/private/var` symlink,
    // and the workspace path the CLI sees is the resolved one.
    let lockfile_dir = canonicalize(&lockfile_dir);
    append_workspace_yaml_key(&workspace, "lockfileDir", lockfile_dir.display());
    pacquet_at(&workspace)
        .with_args(["add", "is-positive@1.0.0"])
        .assert()
        .success();
    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir);
    let registered_projects = || -> Vec<_> {
        pnpm_store_dir::get_registered_projects(&store_dir)
            .expect("list registered projects")
            .iter()
            .map(|project| canonicalize(project))
            .collect()
    };
    let installed = registered_projects();
    fs::remove_dir_all(store_dir.projects()).expect("clear the project registry");

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert_eq!(installed, [canonicalize(&lockfile_dir)]);
    assert_eq!(registered_projects(), installed);
}

#[test]
fn install_without_a_modules_dir_does_not_register_the_project() {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "enableModulesDir", false);
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"is-positive":"1.0.0"}}"#)
        .expect("write package.json");

    for _ in 0..2 {
        pacquet_at(&workspace)
            .with_arg("install")
            .assert()
            .success();
    }

    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir);
    assert!(!store_dir.projects().exists());
}

#[test]
fn up_to_date_dry_run_does_not_register_the_project() {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    pacquet_at(&workspace)
        .with_args(["add", "is-positive@1.0.0"])
        .assert()
        .success();
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir);
    fs::remove_dir_all(store_dir.projects()).expect("clear the project registry");

    pacquet_at(&workspace)
        .with_args(["install", "--dry-run"])
        .assert()
        .success();

    assert!(!store_dir.projects().exists());
}

#[test]
fn frozen_store_install_with_global_virtual_store_registers_the_project() {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    enable_gvs_in_workspace_yaml(&workspace, "");
    pacquet_at(&workspace)
        .with_args(["add", "is-positive@1.0.0"])
        .assert()
        .success();
    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir);
    fs::remove_dir_all(store_dir.projects()).expect("clear the project registry");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile", "--frozen-store", "--offline"])
        .assert()
        .success();

    let projects: Vec<_> = pnpm_store_dir::get_registered_projects(&store_dir)
        .expect("list registered projects")
        .iter()
        .map(|project| canonicalize(project))
        .collect();
    assert_eq!(projects, [canonicalize(&workspace)]);
}
