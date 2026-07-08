//! Tests for `pacquet search`.

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
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

fn empty_auth_file(root: &Path) -> PathBuf {
    let auth_file = root.join("auth-npmrc");
    fs::write(&auth_file, "").expect("write empty auth .npmrc");
    auth_file
}

fn run_search(
    workspace: &Path,
    auth_file: &Path,
    registry: &str,
    args: &[&str],
) -> std::process::Output {
    // Write fetchRetries=0 and fetchRetryMintimeout=0 to project pnpm-workspace.yaml
    fs::write(workspace.join("pnpm-workspace.yaml"), "fetchRetries: 0\nfetchRetryMintimeout: 0\n")
        .expect("write project pnpm-workspace.yaml");

    pacquet_at(workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("search")
        .with_arg("--registry")
        .with_arg(registry)
        .with_args(args)
        .output()
        .expect("spawn pacquet search")
}

fn unreachable_registry() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a probe socket");
    let port = listener.local_addr().expect("read the probe socket address").port();
    drop(listener);
    format!("http://127.0.0.1:{port}/")
}

#[test]
fn missing_query_throws_error() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());
    let server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let output = run_search(&workspace, &auth_file, &registry, &[]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_MISSING_SEARCH_QUERY"));
    assert!(stderr.contains("Search query is required"));
    drop(root);
}

#[test]
fn returns_formatted_output_with_package_name_and_npmx_dev_url() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let mock_body = r#"{
        "objects": [
            {
                "package": {
                    "name": "create-touch-file-one-bin",
                    "version": "1.0.0",
                    "description": "A test description",
                    "date": "2026-07-08T12:00:00.000Z",
                    "author": { "name": "John Doe" },
                    "keywords": ["test", "touch"]
                }
            }
        ]
    }"#;

    let mock = server
        .mock("GET", "/-/v1/search")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("text".into(), "create-touch-file-one-bin".into()),
            Matcher::UrlEncoded("size".into(), "20".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_body)
        .create();

    let auth_file = empty_auth_file(root.path());

    let output = run_search(&workspace, &auth_file, &registry, &["create-touch-file-one-bin"]);

    mock.assert();
    assert!(
        output.status.success(),
        "search must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("create-touch-file-one-bin"));
    assert!(stdout.contains("A test description"));
    assert!(stdout.contains("Version 1.0.0 published 2026-07-08 by John Doe"));
    assert!(stdout.contains("Keywords: test, touch"));
    assert!(stdout.contains("https://npmx.dev/package/create-touch-file-one-bin"));

    drop(root);
}

#[test]
fn json_flag_returns_parsed_package_array() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let mock_body = r#"{
        "objects": [
            {
                "package": {
                    "name": "create-touch-file-one-bin",
                    "version": "1.0.0",
                    "extra_custom_field": "hello"
                }
            }
        ]
    }"#;

    let mock = server
        .mock("GET", "/-/v1/search")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("text".into(), "create-touch-file-one-bin".into()),
            Matcher::UrlEncoded("size".into(), "5".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_body)
        .create();

    let auth_file = empty_auth_file(root.path());

    let output = run_search(
        &workspace,
        &auth_file,
        &registry,
        &["--json", "--search-limit", "5", "create-touch-file-one-bin"],
    );

    mock.assert();
    assert!(
        output.status.success(),
        "search must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("output must be valid JSON");
    assert!(parsed.is_array());
    let arr = parsed.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "create-touch-file-one-bin");
    assert_eq!(arr[0]["version"], "1.0.0");
    assert_eq!(arr[0]["extra_custom_field"], "hello");

    drop(root);
}

#[test]
fn empty_results_returns_no_packages_found() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let mock_body = r#"{
        "objects": []
    }"#;

    let mock = server
        .mock("GET", "/-/v1/search")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("text".into(), "nonexistent-package".into()),
            Matcher::UrlEncoded("size".into(), "20".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_body)
        .create();

    let auth_file = empty_auth_file(root.path());

    let output = run_search(&workspace, &auth_file, &registry, &["nonexistent-package"]);

    mock.assert();
    assert!(
        output.status.success(),
        "search must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "No packages found");

    drop(root);
}

#[test]
fn non_ok_registry_response_throws_search_failed() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let mock = server
        .mock("GET", "/-/v1/search")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("text".into(), "some-package".into()),
            Matcher::UrlEncoded("size".into(), "20".into()),
        ]))
        .with_status(500)
        .with_body("Internal Server Error Details")
        .create();

    let auth_file = empty_auth_file(root.path());

    let output = run_search(&workspace, &auth_file, &registry, &["some-package"]);

    mock.assert();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_SEARCH_FAILED"));
    assert!(stderr.contains("Search failed with status 500: Internal Server Error"));
    assert!(stderr.contains("Internal Server"));
    assert!(stderr.contains("Error Details"));

    drop(root);
}

#[test]
fn fails_on_a_network_failure() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());
    let registry = unreachable_registry();

    let output = run_search(&workspace, &auth_file, &registry, &["some-package"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_SEARCH_FAILED"));
    assert!(stderr.contains("Network request failed"));

    drop(root);
}
