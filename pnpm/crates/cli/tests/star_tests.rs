use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn pacquet_at(workspace: &Path) -> Command {
    let mut cmd = Command::cargo_bin("pnpm").expect("find the pnpm binary");
    cmd.with_current_dir(workspace).env("PNPM_CONFIG_FETCH_RETRIES", "0");
    cmd
}

fn nerf(registry: &str) -> String {
    let without_scheme = registry
        .strip_prefix("http://")
        .or_else(|| registry.strip_prefix("https://"))
        .unwrap_or(registry);
    format!("//{}/", without_scheme.trim_end_matches('/'))
}

fn configure(root: &Path, workspace: &Path, registry: &str, auth_token: Option<&str>) -> PathBuf {
    fs::write(workspace.join(".npmrc"), format!("registry={registry}\nfetch-retries=0\n"))
        .expect("write project .npmrc");
    let auth_file = root.join("auth-npmrc");
    let contents = match auth_token {
        Some(token) => format!("{}:_authToken={token}\n", nerf(registry)),
        None => String::new(),
    };
    fs::write(&auth_file, contents).expect("write auth .npmrc");
    auth_file
}

#[test]
fn star_unauthorized() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = configure(root.path(), &workspace, "http://127.0.0.1:9/", None);
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("star")
        .with_arg("foo")
        .output()
        .expect("spawn pacquet star");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_STAR_UNAUTHORIZED") && stderr.contains("You must be logged in"),
        "stderr must name the unauthorized diagnostic; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn star_successfully_stars_a_package() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("PUT", "/-/user/v1/star")
        .match_header("authorization", "Bearer test-token")
        .match_header("content-type", "application/json")
        .match_body(mockito::Matcher::JsonString(r#"{"name":"foo","package":"foo"}"#.to_string()))
        .with_status(200)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("star")
        .with_arg("foo")
        .output()
        .expect("spawn pacquet star");
    mock.assert();
    assert!(
        output.status.success(),
        "star must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");
    drop((root, server));
}

#[test]
fn star_falls_back_to_legacy_endpoint_when_primary_fails() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let primary_mock = server
        .mock("PUT", "/-/user/v1/star")
        .match_header("authorization", "Bearer test-token")
        .with_status(404)
        .create();
    let legacy_mock = server
        .mock("PUT", "/-/user/package/foo/star")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("star")
        .with_arg("foo")
        .output()
        .expect("spawn pacquet star");
    primary_mock.assert();
    legacy_mock.assert();
    assert!(
        output.status.success(),
        "star must succeed via legacy endpoint (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    drop((root, server));
}

#[test]
fn star_registry_error_when_both_endpoints_fail() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let primary_mock = server
        .mock("PUT", "/-/user/v1/star")
        .match_header("authorization", "Bearer test-token")
        .with_status(404)
        .expect_at_least(1)
        .create();
    let legacy_mock = server
        .mock("PUT", "/-/user/package/foo/star")
        .match_header("authorization", "Bearer test-token")
        .with_status(500)
        .expect_at_least(1)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("star")
        .with_arg("foo")
        .output()
        .expect("spawn pacquet star");
    primary_mock.assert();
    legacy_mock.assert();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_REGISTRY_ERROR"),
        "stderr must name the registry error diagnostic; got:\n{stderr}",
    );
    drop((root, server));
}

#[test]
fn star_stars_a_scoped_package() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("PUT", "/-/user/v1/star")
        .match_header("authorization", "Bearer test-token")
        .match_body(mockito::Matcher::JsonString(
            r#"{"name":"@scope/foo","package":"@scope/foo"}"#.to_string(),
        ))
        .with_status(200)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("star")
        .with_arg("@scope/foo")
        .output()
        .expect("spawn pacquet star");
    mock.assert();
    assert!(
        output.status.success(),
        "star must succeed for scoped packages (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    drop((root, server));
}
