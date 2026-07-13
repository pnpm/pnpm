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
    cmd = cmd.with_current_dir(workspace);
    cmd.env("PNPM_CONFIG_FETCH_RETRIES", "0");
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
fn stars_unauthorized_without_username() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = configure(root.path(), &workspace, "http://127.0.0.1:9/", None);
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("stars")
        .output()
        .expect("spawn pacquet stars");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_STARS_UNAUTHORIZED") && stderr.contains("You must be logged in"),
        "stderr must name the unauthorized diagnostic; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn stars_returns_self_starred_packages_as_array() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let whoami_mock = server
        .mock("GET", "/-/whoami")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(r#"{"username":"alice"}"#)
        .create();
    let stars_mock = server
        .mock("GET", "/-/user/v1/star")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(r#"["foo","bar","baz"]"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("stars")
        .output()
        .expect("spawn pacquet stars");
    whoami_mock.assert();
    stars_mock.assert();
    assert!(
        output.status.success(),
        "stars must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let packages: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(packages, ["foo", "bar", "baz"]);
    drop((root, server));
}

#[test]
fn stars_returns_self_starred_packages_as_object() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let whoami_mock = server
        .mock("GET", "/-/whoami")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(r#"{"username":"alice"}"#)
        .create();
    let stars_mock = server
        .mock("GET", "/-/user/v1/star")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(r#"{"foo":"1.0.0","bar":"2.0.0"}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("stars")
        .output()
        .expect("spawn pacquet stars");
    whoami_mock.assert();
    stars_mock.assert();
    assert!(
        output.status.success(),
        "stars must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let packages: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(packages, ["foo", "bar"]);
    drop((root, server));
}

#[test]
fn stars_self_empty_when_no_packages_are_starred() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let whoami_mock = server
        .mock("GET", "/-/whoami")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(r#"{"username":"alice"}"#)
        .create();
    let stars_mock = server
        .mock("GET", "/-/user/v1/star")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(r"[]")
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("stars")
        .output()
        .expect("spawn pacquet stars");
    whoami_mock.assert();
    stars_mock.assert();
    assert!(
        output.status.success(),
        "stars must succeed for empty list (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");
    drop((root, server));
}

#[test]
fn stars_lists_another_users_starred_packages() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/user/alice/stars")
        .with_status(200)
        .with_body(r#"["foo","bar"]"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("stars")
        .with_arg("alice")
        .output()
        .expect("spawn pacquet stars");
    mock.assert();
    assert!(
        output.status.success(),
        "stars must succeed for other user (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let packages: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(packages, ["foo", "bar"]);
    drop((root, server));
}

#[test]
fn stars_other_user_falls_back_to_util_endpoint() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let primary_mock =
        server.mock("GET", "/-/user/alice/stars").with_status(404).expect_at_least(1).create();
    let util_mock = server
        .mock("GET", "/-/util/user/alice/stars")
        .with_status(200)
        .with_body(r#"["foo"]"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("stars")
        .with_arg("alice")
        .output()
        .expect("spawn pacquet stars");
    primary_mock.assert();
    util_mock.assert();
    assert!(
        output.status.success(),
        "stars must succeed via util endpoint (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "foo");
    drop((root, server));
}

#[test]
fn stars_user_not_found() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let primary_mock =
        server.mock("GET", "/-/user/missing/stars").with_status(404).expect_at_least(1).create();
    let util_mock = server
        .mock("GET", "/-/util/user/missing/stars")
        .with_status(404)
        .expect_at_least(1)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("stars")
        .with_arg("missing")
        .output()
        .expect("spawn pacquet stars");
    primary_mock.assert();
    util_mock.assert();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_USER_NOT_FOUND")
            && stderr.contains(r#"User "missing" not found"#),
        "stderr must name the user-not-found diagnostic; got:\n{stderr}",
    );
    drop((root, server));
}

#[test]
fn stars_other_user_401_falls_through_to_util_endpoint() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let primary_mock =
        server.mock("GET", "/-/user/alice/stars").with_status(401).expect_at_least(1).create();
    let util_mock = server
        .mock("GET", "/-/util/user/alice/stars")
        .with_status(200)
        .with_body(r#"["foo"]"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("stars")
        .with_arg("alice")
        .output()
        .expect("spawn pacquet stars");
    primary_mock.assert();
    util_mock.assert();
    assert!(
        output.status.success(),
        "stars must fall through to the util endpoint on 401 (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "foo");
    drop((root, server));
}

#[test]
fn stars_other_user_registry_error() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let primary_mock =
        server.mock("GET", "/-/user/alice/stars").with_status(500).expect_at_least(1).create();
    let util_mock =
        server.mock("GET", "/-/util/user/alice/stars").with_status(500).expect_at_least(1).create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));
    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("stars")
        .with_arg("alice")
        .output()
        .expect("spawn pacquet stars");
    primary_mock.assert();
    util_mock.assert();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_REGISTRY_ERROR"),
        "stderr must name the registry error diagnostic; got:\n{stderr}",
    );
    drop((root, server));
}
