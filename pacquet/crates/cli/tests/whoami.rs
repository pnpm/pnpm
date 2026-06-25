//! `pacquet whoami` resolves the default registry's auth token from config
//! and prints the username returned by `GET <registry>-/whoami`.
//!
//! Ports the upstream whoami tests
//! (<https://github.com/pnpm/pnpm/blob/fc2f33912e/pnpm11/registry-access/commands/test/whoami.ts>):
//! the success path, the unauthenticated path, a registry that rejects the
//! request, and preservation of a registry path prefix. Adds a pacquet-only
//! guard that control characters in a registry-provided username are stripped.
//!
//! The registry is a `mockito` server the spawned `pacquet` connects to
//! over loopback. Credentials are supplied through `--npmrc-auth-file`,
//! which replaces the developer's real `~/.npmrc`, so the unauthenticated
//! test can't be fooled by a token already on the machine.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

/// The nerf-darted `.npmrc` auth-key prefix (`//host:port/path/`) for a
/// registry URL — its scheme dropped and a single trailing slash kept.
fn nerf(registry: &str) -> String {
    let without_scheme = registry
        .strip_prefix("http://")
        .or_else(|| registry.strip_prefix("https://"))
        .unwrap_or(registry);
    format!("//{}/", without_scheme.trim_end_matches('/'))
}

/// Point `pacquet whoami` at `registry`: the project `.npmrc` carries the
/// registry URL and a separate auth file (returned for `--npmrc-auth-file`)
/// carries the token. Passing `None` leaves the user unauthenticated.
fn configure(root: &Path, workspace: &Path, registry: &str, auth_token: Option<&str>) -> PathBuf {
    fs::write(workspace.join(".npmrc"), format!("registry={registry}\n"))
        .expect("write project .npmrc");
    let auth_file = root.join("auth-npmrc");
    let contents = match auth_token {
        Some(token) => format!("{}:_authToken={token}\n", nerf(registry)),
        None => String::new(),
    };
    fs::write(&auth_file, contents).expect("write auth .npmrc");
    auth_file
}

fn run_whoami(workspace: &Path, auth_file: &Path) -> std::process::Output {
    pacquet_at(workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("whoami")
        .output()
        .expect("spawn pacquet whoami")
}

#[test]
fn returns_the_current_username() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/whoami")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(r#"{"username":"alice"}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));

    let output = run_whoami(&workspace, &auth_file);

    mock.assert();
    assert!(
        output.status.success(),
        "whoami must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "alice");
    drop((root, server));
}

#[test]
fn strips_control_characters_from_the_username() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    // A malicious/compromised registry returns a username carrying an ESC
    // (0x1b) and a BEL (0x07); pacquet must not emit them raw to the terminal.
    let esc = char::from(0x1b);
    let bel = char::from(0x07);
    let username = format!("al{esc}[31mice{bel}");
    let body = serde_json::json!({ "username": username }).to_string();
    let mock = server.mock("GET", "/-/whoami").with_status(200).with_body(body).create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));

    let output = run_whoami(&workspace, &auth_file);

    mock.assert();
    assert!(
        output.status.success(),
        "whoami must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "al[31mice", "control characters must be stripped");
    assert!(!stdout.contains(esc), "ESC must be stripped: {stdout:?}");
    assert!(!stdout.contains(bel), "BEL must be stripped: {stdout:?}");
    drop((root, server));
}

#[test]
fn fails_when_not_logged_in() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    // No request is made, so no server is needed; the registry just has to
    // resolve to no credentials.
    let auth_file = configure(root.path(), &workspace, "http://127.0.0.1:1/", None);

    let output = run_whoami(&workspace, &auth_file);

    assert!(
        !output.status.success(),
        "an unauthenticated whoami must fail (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_WHOAMI_UNAUTHORIZED") && stderr.contains("You must be logged in"),
        "stderr must name the unauthorized diagnostic; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn fails_when_the_registry_rejects_the_request() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server.mock("GET", "/-/whoami").with_status(401).with_body("{}").create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));

    let output = run_whoami(&workspace, &auth_file);

    mock.assert();
    assert!(
        !output.status.success(),
        "a rejected whoami must fail (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_WHOAMI_FAILED")
            && stderr.contains("Failed to find the current user"),
        "stderr must name the failed diagnostic; got:\n{stderr}",
    );
    drop((root, server));
}

#[test]
fn preserves_a_registry_path_prefix() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/custom-prefix/", server.url());
    let mock = server
        .mock("GET", "/custom-prefix/-/whoami")
        .with_status(200)
        .with_body(r#"{"username":"alice"}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, Some("test-token"));

    let output = run_whoami(&workspace, &auth_file);

    mock.assert();
    assert!(
        output.status.success(),
        "whoami must succeed against a prefixed registry (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "alice");
    drop((root, server));
}
