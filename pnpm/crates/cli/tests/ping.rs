//! `pacquet ping` resolves the registry (the `--registry` override or the
//! configured default) and prints a `PING`/`PONG` report for
//! `GET <registry>-/ping?write=true`.
//!
//! Covers the reachable-registry report, the JSON-details branch, the
//! configured default registry, rejection on a non-success status,
//! preservation of a registry path prefix, and a transport failure, plus a
//! guard that inline registry credentials are redacted from the echoed
//! `PING` line.
//!
//! The registry is a `mockito` server the spawned `pacquet` connects to
//! over loopback. An empty `--npmrc-auth-file` replaces the developer's
//! real `~/.npmrc` so a token or `registry=` already on the machine can't
//! influence the test.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// An empty user-level `.npmrc`, returned for `--npmrc-auth-file`, so the
/// developer's real `~/.npmrc` cannot leak a registry or token into the test.
fn empty_auth_file(root: &Path) -> PathBuf {
    let auth_file = root.join("auth-npmrc");
    fs::write(&auth_file, "").expect("write empty auth .npmrc");
    auth_file
}

fn run_ping(workspace: &Path, auth_file: &Path, registry: Option<&str>) -> std::process::Output {
    let mut command =
        pacquet_at(workspace).with_arg("--npmrc-auth-file").with_arg(auth_file).with_arg("ping");
    if let Some(registry) = registry {
        command = command.with_arg("--registry").with_arg(registry);
    }
    command.output().expect("spawn pacquet ping")
}

/// Match `GET /<path>-/ping?write=true`, the request `pacquet ping` issues.
fn ping_mock(server: &mut mockito::Server, path_prefix: &str) -> mockito::Mock {
    server
        .mock("GET", format!("{path_prefix}/-/ping").as_str())
        .match_query(Matcher::UrlEncoded("write".into(), "true".into()))
}

/// A loopback registry URL with nothing listening: bind an ephemeral port,
/// then drop the listener so a connection is refused immediately. Avoids
/// assuming a fixed port is free and keeps the network-failure test fast — a
/// closed loopback port returns `ECONNREFUSED` rather than stalling on connect.
fn unreachable_registry() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a probe socket");
    let port = listener.local_addr().expect("read the probe socket address").port();
    drop(listener);
    format!("http://127.0.0.1:{port}/")
}

#[test]
fn reports_ping_and_pong_for_a_reachable_registry() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = ping_mock(&mut server, "").with_status(200).with_body("{}").create();
    let auth_file = empty_auth_file(root.path());

    let output = run_ping(&workspace, &auth_file, Some(&registry));

    mock.assert();
    assert!(
        output.status.success(),
        "ping must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "an empty JSON body produces no details: {stdout:?}");
    assert_eq!(lines[0], format!("PING {registry}"));
    let pong = lines[1].strip_prefix("PONG ").expect("PONG line").strip_suffix("ms").expect("ms");
    pong.parse::<u128>().expect("the elapsed time must be a number of milliseconds");
    drop((root, server));
}

#[test]
fn includes_details_when_the_body_is_non_empty_json() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let body = r#"{"host":"npm","user":"anonymous"}"#;
    let mock = ping_mock(&mut server, "").with_status(200).with_body(body).create();
    let auth_file = empty_auth_file(root.path());

    let output = run_ping(&workspace, &auth_file, Some(&registry));

    mock.assert();
    assert!(
        output.status.success(),
        "ping must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("PING {registry}")), "missing PING line: {stdout:?}");
    assert!(stdout.contains("PONG "), "missing PONG line: {stdout:?}");
    assert!(stdout.contains(r#""host": "npm""#), "missing pretty-printed details: {stdout:?}");
    drop((root, server));
}

#[test]
fn uses_the_configured_registry_when_no_flag_is_given() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    fs::write(workspace.join(".npmrc"), format!("registry={}\n", server.url()))
        .expect("write project .npmrc");
    let mock = ping_mock(&mut server, "").with_status(200).with_body("{}").create();
    let auth_file = empty_auth_file(root.path());

    let output = run_ping(&workspace, &auth_file, None);

    mock.assert();
    assert!(
        output.status.success(),
        "ping must succeed against the configured registry (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The configured registry is normalized to carry a trailing slash.
    assert!(
        stdout.contains(&format!("PING {}/", server.url())),
        "PING must echo the configured registry: {stdout:?}",
    );
    drop((root, server));
}

#[test]
fn fails_when_the_registry_responds_with_an_error_status() {
    for status in [401_usize, 403, 404, 500] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let mut server = mockito::Server::new();
        let registry = format!("{}/", server.url());
        let mock = ping_mock(&mut server, "")
            .with_status(status)
            .with_body(r#"{"error":"nope"}"#)
            .create();
        let auth_file = empty_auth_file(root.path());

        let output = run_ping(&workspace, &auth_file, Some(&registry));

        mock.assert();
        assert!(
            !output.status.success(),
            "ping must fail on status {status} (stderr: {})",
            String::from_utf8_lossy(&output.stderr),
        );
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(
            stderr.contains("ERR_PNPM_PING_ERROR") && stderr.contains("Failed to reach registry"),
            "stderr must name the ping diagnostic for status {status}; got:\n{stderr}",
        );
        drop((root, server));
    }
}

#[test]
fn preserves_a_registry_path_prefix() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    // No trailing slash: ping must still target `<prefix>/-/ping`.
    let registry = format!("{}/custom-prefix", server.url());
    let mock = ping_mock(&mut server, "/custom-prefix").with_status(200).with_body("{}").create();
    let auth_file = empty_auth_file(root.path());

    let output = run_ping(&workspace, &auth_file, Some(&registry));

    mock.assert();
    assert!(
        output.status.success(),
        "ping must succeed against a prefixed registry (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("PING {registry}")), "PING must echo the raw URL: {stdout:?}");
    drop((root, server));
}

#[test]
fn redacts_inline_credentials_in_the_ping_line() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let mock = ping_mock(&mut server, "").with_status(200).with_body("{}").create();
    // A registry URL carrying inline basic-auth credentials, which must not
    // leak into the echoed `PING` line.
    let host = server.url();
    let with_credentials = format!("{}/", host.replacen("http://", "http://hunter2:s3cr3t@", 1));
    let auth_file = empty_auth_file(root.path());

    let output = run_ping(&workspace, &auth_file, Some(&with_credentials));

    mock.assert();
    assert!(
        output.status.success(),
        "ping must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("hunter2") && !stdout.contains("s3cr3t"),
        "inline credentials must be redacted from the PING line: {stdout:?}",
    );
    assert!(
        stdout.contains(&format!("PING {host}/")),
        "PING must echo the credential-free registry URL: {stdout:?}",
    );
    drop((root, server));
}

#[test]
fn fails_on_a_network_failure() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());
    let registry = unreachable_registry();

    let output = run_ping(&workspace, &auth_file, Some(&registry));

    assert!(
        !output.status.success(),
        "ping must fail when the registry is unreachable (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_PING_ERROR") && stderr.contains("Failed to reach registry"),
        "stderr must name the ping diagnostic; got:\n{stderr}",
    );
    drop(root);
}
