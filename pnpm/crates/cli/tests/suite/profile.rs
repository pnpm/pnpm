use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn authed_file(root: &Path, registry: &str, token: &str) -> PathBuf {
    let auth_file = root.join("auth-npmrc");
    let host = registry.trim_start_matches("http://").trim_start_matches("https://");
    let host = host.trim_end_matches('/');
    fs::write(&auth_file, format!("//{host}/:_authToken={token}\n")).expect("write auth .npmrc");
    auth_file
}

fn empty_auth_file(root: &Path) -> PathBuf {
    let auth_file = root.join("empty-auth-npmrc");
    fs::write(&auth_file, "").expect("write empty auth .npmrc");
    auth_file
}

#[test]
fn profile_get_unauthorized_when_no_token() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("profile")
        .with_arg("get")
        .output()
        .expect("spawn pnpm profile get");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_PROFILE_UNAUTHORIZED"), "unexpected stderr: {stderr}");
}

#[test]
fn profile_get_renders_table_format() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let auth_file = authed_file(root.path(), &registry, "secret-token");

    let profile_data = json!({
        "name": "testuser",
        "email": "test@example.com",
        "email_verified": true,
        "tfa": {
            "mode": "auth-and-writes",
        },
        "fullname": "Test User",
        "homepage": "https://example.com",
        "freenode": "",
        "twitter": "test_twitter",
        "github": "test_github",
        "created": "2024-01-01T00:00:00.000Z",
        "updated": "2024-01-02T00:00:00.000Z",
    });

    let mock = server
        .mock("GET", "/-/npm/v1/user")
        .match_header("authorization", "Bearer secret-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(profile_data.to_string())
        .create();

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("--registry")
        .with_arg(&registry)
        .with_arg("profile")
        .with_arg("get")
        .output()
        .expect("spawn pnpm profile get");

    mock.assert();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("name: testuser"));
    assert!(stdout.contains("email: test@example.com (verified)"));
    assert!(stdout.contains("two-factor auth: auth-and-writes"));
    assert!(stdout.contains("fullname: Test User"));
    assert!(stdout.contains("github: test_github"));
}

#[test]
fn profile_get_specific_property() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let auth_file = authed_file(root.path(), &registry, "secret-token");

    let profile_data = json!({
        "name": "testuser",
        "email": "test@example.com",
        "email_verified": false,
    });

    let mock = server
        .mock("GET", "/-/npm/v1/user")
        .match_header("authorization", "Bearer secret-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(profile_data.to_string())
        .create();

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("--registry")
        .with_arg(&registry)
        .with_arg("profile")
        .with_arg("get")
        .with_arg("email")
        .output()
        .expect("spawn pnpm profile get email");

    mock.assert();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "test@example.com (unverified)");
}

#[test]
fn profile_get_json() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let auth_file = authed_file(root.path(), &registry, "secret-token");

    let profile_data = json!({
        "name": "testuser",
        "email": "test@example.com",
    });

    let mock = server
        .mock("GET", "/-/npm/v1/user")
        .match_header("authorization", "Bearer secret-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(profile_data.to_string())
        .create();

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("--registry")
        .with_arg(&registry)
        .with_arg("profile")
        .with_arg("get")
        .with_arg("--json")
        .output()
        .expect("spawn pnpm profile get --json");

    mock.assert();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["name"], "testuser");
    assert_eq!(parsed["email"], "test@example.com");
}

#[test]
fn profile_set_property() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let auth_file = authed_file(root.path(), &registry, "secret-token");

    let profile = server
        .mock("GET", "/-/npm/v1/user")
        .match_header("authorization", "Bearer secret-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "name": "testuser", "fullname": "Old Name" }).to_string())
        .create();
    let mock = server
        .mock("POST", "/-/npm/v1/user")
        .match_header("authorization", "Bearer secret-token")
        .match_header("content-type", "application/json")
        .match_body(Matcher::Json(json!({
            "fullname": "New Name",
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "fullname": "New Name" }).to_string())
        .create();

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("--registry")
        .with_arg(&registry)
        .with_arg("profile")
        .with_arg("set")
        .with_arg("fullname")
        .with_arg("New Name")
        .output()
        .expect("spawn pnpm profile set");

    profile.assert();
    mock.assert();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "Set fullname to New Name");
}

#[test]
fn profile_disable_2fa_when_not_enabled() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let auth_file = authed_file(root.path(), &registry, "secret-token");

    let mock = server
        .mock("GET", "/-/npm/v1/user")
        .match_header("authorization", "Bearer secret-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "name": "testuser", "tfa": null }).to_string())
        .create();

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("--registry")
        .with_arg(&registry)
        .with_arg("profile")
        .with_arg("disable-2fa")
        .output()
        .expect("spawn pnpm profile disable-2fa");

    mock.assert();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "Two factor authentication not enabled.");
}
