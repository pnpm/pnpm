use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils_registry::{
    bin::CommandTempCwd,
    command_env::CommandTestExt,
};
use pretty_assertions::assert_eq;
use std::{
    fs,
    process::Command,
};

#[test]
fn pnpm_config_home_routes_global_reads_writes_and_deletes() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let config_home = root.path().join("custom-config");
    let xdg_home = root.path().join("xdg-config");
    fs::create_dir_all(xdg_home.join("pnpm")).unwrap();
    fs::write(xdg_home.join("pnpm/config.yaml"), "fetchRetries: 3\n").unwrap();
    let command = || {
        Command::cargo_bin("pnpm")
            .unwrap()
            .with_current_dir(&workspace)
            .without_ambient_pnpm_config()
            .with_env("PNPM_CONFIG_HOME", &config_home)
            .with_env("XDG_CONFIG_HOME", &xdg_home)
    };
    command()
        .with_args(["config", "set", "--global", "fetch-retries", "7"])
        .assert()
        .success();
    command()
        .with_args(["config", "get", "fetch-retries"])
        .assert()
        .success()
        .stdout("7\n");
    command()
        .with_args(["config", "get", "globalconfig"])
        .assert()
        .success()
        .stdout(format!("{}\n", config_home.join("config.yaml").display()));
    command()
        .with_args([
            "config",
            "set",
            "--global",
            "//registry.example.test/:_authToken",
            "test-token",
        ])
        .assert()
        .success();
    let output = command()
        .with_args(["config", "get", "//registry.example.test/:_authToken"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_CONFIG_GET_PROTECTED"), "{stderr}");
    assert!(!stderr.contains("test-token"));
    assert!(fs::read_to_string(config_home.join("auth.ini")).unwrap().contains("test-token"));
    fs::write(workspace.join("pnpm-workspace.yaml"), "fetchRetries: 8\n").unwrap();
    command()
        .with_args(["config", "get", "fetch-retries"])
        .assert()
        .success()
        .stdout("8\n");
    command()
        .with_env("PNPM_CONFIG_FETCH_RETRIES", "9")
        .with_args(["config", "get", "fetch-retries"])
        .assert()
        .success()
        .stdout("9\n");
    fs::remove_file(workspace.join("pnpm-workspace.yaml")).unwrap();
    command()
        .with_args(["config", "delete", "--global", "fetch-retries"])
        .assert()
        .success();
    command()
        .with_args(["config", "get", "fetch-retries"])
        .assert()
        .success()
        .stdout("undefined\n");
    assert_eq!(fs::read_to_string(xdg_home.join("pnpm/config.yaml")).unwrap(), "fetchRetries: 3\n");
}
