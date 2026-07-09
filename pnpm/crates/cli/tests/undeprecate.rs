use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

fn empty_auth_file(root: &Path) -> PathBuf {
    let auth_file = root.join("auth-npmrc");
    fs::write(&auth_file, "").expect("write empty auth .npmrc");
    auth_file
}

fn run_undeprecate(
    workspace: &Path,
    auth_file: &Path,
    registry: Option<&str>,
    params: &[&str],
) -> std::process::Output {
    let mut command = pacquet_at(workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("undeprecate");
    if let Some(registry) = registry {
        command = command.with_arg("--registry").with_arg(registry);
    }
    for p in params {
        command = command.with_arg(p);
    }
    command.output().expect("spawn pacquet undeprecate")
}

#[test]
fn undeprecates_a_package_version_successfully() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let get_mock = server
        .mock("GET", "/test")
        .with_status(200)
        .with_body(r#"{"versions":{"1.0.0":{"deprecated":"old"}}}"#)
        .create();

    let put_mock = server
        .mock("PUT", "/test")
        .match_header("content-type", "application/json")
        .match_body(Matcher::Json(serde_json::json!({
            "versions": {
                "1.0.0": {}
            }
        })))
        .with_status(200)
        .with_body("{}")
        .create();

    let auth_file = empty_auth_file(root.path());
    let output = run_undeprecate(&workspace, &auth_file, Some(&registry), &["test@1.0.0"]);

    get_mock.assert();
    put_mock.assert();
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Successfully un-deprecated 1 version(s) of test"));
}

#[test]
fn fails_when_package_is_not_provided() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());
    let output = run_undeprecate(&workspace, &auth_file, None, &[]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_DEPRECATE_PACKAGE_REQUIRED"));
}

#[test]
fn fails_when_not_deprecated() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let get_mock = server
        .mock("GET", "/test")
        .with_status(200)
        .with_body(r#"{"versions":{"1.0.0":{}}}"#)
        .create();

    let auth_file = empty_auth_file(root.path());
    let output = run_undeprecate(&workspace, &auth_file, Some(&registry), &["test@1.0.0"]);

    get_mock.assert();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_NOT_DEPRECATED"));
}
