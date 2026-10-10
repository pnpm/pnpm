use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn nerf(registry: &str) -> String {
    let without_scheme = registry
        .strip_prefix("http://")
        .or_else(|| registry.strip_prefix("https://"))
        .unwrap_or(registry);
    format!("//{}/", without_scheme.trim_end_matches('/'))
}

fn configure(root: &Path, workspace: &Path, registry: &str, auth_token: Option<&str>) -> PathBuf {
    fs::write(workspace.join(".npmrc"), format!("registry={registry}\n"))
        .expect("write project .npmrc");
    fs::write(workspace.join("package.json"), "{}").expect("write project package.json");
    let auth_file = root.join("auth-npmrc");
    let contents = match auth_token {
        Some(token) => format!("{}:_authToken={token}\n", nerf(registry)),
        None => String::new(),
    };
    fs::write(&auth_file, contents).expect("write auth .npmrc");
    auth_file
}

#[test]
fn token_list_returns_tokens() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/npm/v1/tokens")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "objects": [
                    {
                        "key": "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
                        "token": "abcdef",
                        "user": "alice",
                        "readonly": false,
                        "created": "2026-01-01T00:00:00.000Z",
                    }
                ],
                "urls": {},
            })
            .to_string(),
        )
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("token")
        .with_arg("list")
        .output()
        .expect("spawn pacquet token list");

    mock.assert();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Token abcdef… with id abcdef created 2026-01-01"));
}

#[test]
fn token_list_json_format() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/npm/v1/tokens")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "objects": [
                    {
                        "key": "1234567890123456789012345678901234567890123456789012345678901234",
                        "token": "123456",
                        "user": "alice",
                        "readonly": true,
                        "created": "2026-01-01T00:00:00.000Z",
                    },
                ],
            })
            .to_string(),
        )
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("token")
        .with_arg("list")
        .with_arg("--json")
        .output()
        .expect("spawn pacquet token list --json");

    mock.assert();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("parse json");
    assert!(parsed.is_array());
    assert_eq!(parsed[0]["token"], "123456");
}

#[test]
fn token_revoke_deletes_token() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let key = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
    let list_mock = server
        .mock("GET", "/-/npm/v1/tokens")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "objects": [
                    {
                        "key": key,
                        "token": "abcdef",
                        "user": "alice",
                        "readonly": false,
                        "created": "2026-01-01T00:00:00.000Z",
                    },
                ],
            })
            .to_string(),
        )
        .create();
    let delete_mock = server
        .mock("DELETE", format!("/-/npm/v1/tokens/token/{key}").as_str())
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(r#"{"ok":true}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("token")
        .with_arg("revoke")
        .with_arg("abcdef")
        .output()
        .expect("spawn pacquet token revoke");

    list_mock.assert();
    delete_mock.assert();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Removed 1 token"));
}

#[test]
fn token_create_posts_token() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("POST", "/-/npm/v1/tokens")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "token": "npm_secretcreatedtoken123",
                "readonly": true,
            })
            .to_string(),
        )
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("token")
        .with_arg("create")
        .with_arg("--read-only")
        .output()
        .expect("spawn pacquet token create");

    mock.assert();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Created token npm_secretcreatedtoken123"));
}

#[test]
fn token_fails_when_unauthorized() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let registry = "https://unauthorized.registry/";
    let auth_file = configure(root.path(), &workspace, registry, None);

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("token")
        .with_arg("list")
        .output()
        .expect("spawn pacquet token list");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_TOKEN_UNAUTHORIZED"));
}

#[test]
fn token_revoke_requires_key_argument() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let registry = "https://example.test/";
    let auth_file = configure(root.path(), &workspace, registry, Some("test-token"));

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("token")
        .with_arg("revoke")
        .output()
        .expect("spawn pacquet token revoke");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_TOKEN_KEY_REQUIRED"));
}
