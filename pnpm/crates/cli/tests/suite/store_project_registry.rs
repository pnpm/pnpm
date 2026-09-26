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
