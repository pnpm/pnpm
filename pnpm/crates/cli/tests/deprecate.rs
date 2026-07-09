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

fn run_deprecate(
    workspace: &Path,
    auth_file: &Path,
    registry: Option<&str>,
    params: &[&str],
) -> std::process::Output {
    let mut command = pacquet_at(workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("deprecate");
    if let Some(registry) = registry {
        command = command.with_arg("--registry").with_arg(registry);
    }
    for p in params {
        command = command.with_arg(p);
    }
    command.output().expect("spawn pacquet deprecate")
}

#[test]
fn deprecates_a_package_version_successfully() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let get_mock = server
        .mock("GET", "/@scope%2ftest")
        .with_status(200)
        .with_body(r#"{"versions":{"1.0.0":{}}}"#)
        .create();

    let put_mock = server
        .mock("PUT", "/@scope%2ftest")
        .match_header("content-type", "application/json")
        .match_body(Matcher::Json(serde_json::json!({
            "versions": {
                "1.0.0": {
                    "deprecated": "no longer maintained"
                }
            }
        })))
        .with_status(200)
        .with_body("{}")
        .create();

    let auth_file = empty_auth_file(root.path());
    let output = run_deprecate(
        &workspace,
        &auth_file,
        Some(&registry),
        &["@scope/test@1.0.0", "no longer maintained"],
    );

    get_mock.assert();
    put_mock.assert();
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Successfully deprecated 1 version(s) of @scope/test"),
        "stdout: {stdout}"
    );
}

#[test]
fn fails_when_package_is_not_provided() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());
    let output = run_deprecate(&workspace, &auth_file, None, &[]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_DEPRECATE_PACKAGE_REQUIRED"),
        "stderr: {stderr}"
    );
}

#[test]
fn fails_when_message_is_not_provided() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());
    let output = run_deprecate(&workspace, &auth_file, None, &["foo"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_DEPRECATE_MESSAGE_REQUIRED"),
        "stderr: {stderr}"
    );
}

#[test]
fn fails_on_unauthorized() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let get_mock = server
        .mock("GET", "/test")
        .with_status(200)
        .with_body(r#"{"versions":{"1.0.0":{}}}"#)
        .create();

    let put_mock = server
        .mock("PUT", "/test")
        .with_status(401)
        .with_body(r#"{"error":"unauthorized"}"#)
        .create();

    let auth_file = empty_auth_file(root.path());
    let output = run_deprecate(&workspace, &auth_file, Some(&registry), &["test", "msg"]);

    get_mock.assert();
    put_mock.assert();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_UNAUTHORIZED"),
        "stderr: {stderr}"
    );
}
