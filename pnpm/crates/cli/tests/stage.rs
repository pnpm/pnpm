//! `pacquet stage` integration tests: drive the real binary against a
//! `mockito` registry, mirroring pnpm's `releasing/commands/test/stage.test.ts`
//! scenarios that don't need an interactive terminal.
//!
//! CI env is cleared on every spawn so the binary's OIDC id-token probe stays
//! offline and deterministic (outside a supported CI it resolves to "no token"
//! without a network request).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

const STAGE_ID: &str = "1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f";

fn pacquet(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(workspace)
        .without_env("GITHUB_ACTIONS")
        .without_env("GITLAB_CI")
        .without_env("NPM_ID_TOKEN")
        .without_env("NPM_CONFIG_OTP")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_URL")
}

fn stage(workspace: &Path, args: &[&str]) -> std::process::Output {
    pacquet(workspace).with_arg("stage").with_args(args).output().expect("spawn pacquet stage")
}

fn write_project(dir: &Path, registry: &str, manifest: &Value) {
    fs::write(dir.join(".npmrc"), format!("registry={registry}\n")).expect("write .npmrc");
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

fn write_registry_config(dir: &Path, registry: &str) {
    fs::write(dir.join(".npmrc"), format!("registry={registry}\n")).expect("write .npmrc");
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stage must succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

fn assert_failure_with_code(output: &std::process::Output, code: &str) {
    assert!(!output.status.success(), "stage must fail with {code}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(code), "stderr must carry {code}; stderr: {stderr}");
}

#[test]
fn publish_posts_to_the_staging_endpoint_and_returns_keyed_json_with_stage_id() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "@scope/stage-publish-json", "version": "1.0.0" }),
    );
    let mock = server
        .mock("POST", "/-/stage/package/@scope%2fstage-publish-json")
        .match_header("npm-command", "stage")
        .with_status(201)
        .with_body(format!(r#"{{"stageId":"{STAGE_ID}"}}"#))
        .expect(1)
        .create();

    let output = stage(dir.path(), &["publish", "--json", "--no-git-checks", "--reporter=silent"]);

    mock.assert();
    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let keyed: Value = serde_json::from_str(&stdout).expect("keyed JSON output");
    let summary = &keyed["@scope/stage-publish-json"];
    assert_eq!(summary["name"], "@scope/stage-publish-json");
    assert_eq!(summary["version"], "1.0.0");
    assert_eq!(summary["stageId"], STAGE_ID);
}

#[test]
fn publish_prints_the_staged_line_with_the_stage_id() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "stage-publish-line", "version": "1.0.0" }),
    );
    server
        .mock("POST", "/-/stage/package/stage-publish-line")
        .with_status(201)
        .with_body(format!(r#"{{"stageId":"{STAGE_ID}"}}"#))
        .create();

    let output = stage(dir.path(), &["publish", "--no-git-checks", "--reporter=silent"]);

    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("+ stage-publish-line@1.0.0 (staged with id {STAGE_ID})\n"),
    );
}

#[test]
fn publish_dry_run_reports_that_the_package_would_be_staged() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "@scope/stage-publish-dry-run", "version": "1.0.0" }),
    );
    let mock = server.mock("POST", Matcher::Any).expect(0).create();

    let output =
        stage(dir.path(), &["publish", "--dry-run", "--no-git-checks", "--reporter=silent"]);

    mock.assert();
    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "+ @scope/stage-publish-dry-run@1.0.0 (would stage)\n",
    );
}

fn staged_item() -> Value {
    json!({
        "id": STAGE_ID,
        "packageName": "@scope/example-package",
        "version": "1.2.3",
        "tag": "latest",
        "createdAt": "2026-03-16T09:00:00.000Z",
        "actor": "user",
        "actorType": "user",
        "shasum": "4f7f5f1d5bcf2f72f6e4d6c4f3b2812d8a2f6c19",
    })
}

#[test]
fn list_and_view_fetch_staged_package_metadata() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let item = staged_item();
    let list_mock = server
        .mock("GET", "/-/stage")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "0".into()),
            Matcher::UrlEncoded("perPage".into(), "100".into()),
        ]))
        .with_body(json!({ "items": [item], "page": 0, "perPage": 100, "total": 1 }).to_string())
        .expect(1)
        .create();
    let view_mock = server
        .mock("GET", format!("/-/stage/{STAGE_ID}").as_str())
        .with_body(item.to_string())
        .expect(1)
        .create();

    let list_output = stage(dir.path(), &["list", "--json", "--reporter=silent"]);
    list_mock.assert();
    assert_success(&list_output);
    let listed: Value = serde_json::from_str(&String::from_utf8_lossy(&list_output.stdout))
        .expect("list JSON output");
    assert_eq!(listed, json!([staged_item()]));

    let view_output = stage(dir.path(), &["view", STAGE_ID]);
    view_mock.assert();
    assert_success(&view_output);
    let stdout = String::from_utf8_lossy(&view_output.stdout);
    assert!(stdout.contains("package name: @scope/example-package"), "stdout: {stdout}");
    assert!(stdout.contains("staged by: user (user)"), "stdout: {stdout}");
}

#[test]
fn list_passes_the_package_filter_and_reports_an_empty_result() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let mock = server
        .mock("GET", "/-/stage")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "0".into()),
            Matcher::UrlEncoded("perPage".into(), "100".into()),
            Matcher::UrlEncoded("package".into(), "@scope/example-package".into()),
        ]))
        .with_body(json!({ "items": [], "page": 0, "perPage": 100, "total": 0 }).to_string())
        .expect(1)
        .create();

    let output = stage(dir.path(), &["list", "@scope/example-package"]);

    mock.assert();
    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "No staged versions of package name \"@scope/example-package\".\n",
    );
}

#[test]
fn list_paginates_until_the_total_is_reached() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let full_page: Vec<Value> = (0..100).map(|_| staged_item()).collect();
    let first = server
        .mock("GET", "/-/stage")
        .match_query(Matcher::UrlEncoded("page".into(), "0".into()))
        .with_body(json!({ "items": full_page, "total": 101 }).to_string())
        .expect(1)
        .create();
    let second = server
        .mock("GET", "/-/stage")
        .match_query(Matcher::UrlEncoded("page".into(), "1".into()))
        .with_body(json!({ "items": [staged_item()], "total": 101 }).to_string())
        .expect(1)
        .create();

    let output = stage(dir.path(), &["list", "--json", "--reporter=silent"]);

    first.assert();
    second.assert();
    assert_success(&output);
    let listed: Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("list JSON output");
    assert_eq!(listed.as_array().map(Vec::len), Some(101));
}

/// A registry that keeps answering full pages with an inflated `total` must
/// not drive the pagination loop forever: it stops at the fail-safe cap of
/// 1000 pages.
#[test]
fn list_stops_paginating_at_the_fail_safe_page_cap() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let full_page: Vec<Value> = (0..100).map(|_| staged_item()).collect();
    let mock = server
        .mock("GET", "/-/stage")
        .match_query(Matcher::Any)
        .with_body(json!({ "items": full_page, "total": 10_000_000 }).to_string())
        .expect(1000)
        .create();

    let output = stage(dir.path(), &["list", "--json", "--reporter=silent"]);

    mock.assert();
    assert_success(&output);
    let listed: Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("list JSON output");
    assert_eq!(listed.as_array().map(Vec::len), Some(100_000));
}

#[test]
fn list_uses_package_scoped_auth_for_package_filters() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let host = registry.strip_prefix("http://").unwrap_or(&registry);
    let auth_file = dir.path().join("auth-npmrc");
    fs::write(
        &auth_file,
        format!("//{host}:_authToken=default-token\n//{host}:@scope:_authToken=scoped-token\n"),
    )
    .expect("write auth .npmrc");
    let mock = server
        .mock("GET", "/-/stage")
        .match_query(Matcher::Any)
        .match_header("authorization", "Bearer scoped-token")
        .with_body(json!({ "items": [], "page": 0, "perPage": 100, "total": 0 }).to_string())
        .expect(1)
        .create();

    let output = pacquet(dir.path())
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("stage")
        .with_args(["list", "@scope/example-package"])
        .output()
        .expect("spawn pacquet stage");

    mock.assert();
    assert_success(&output);
}

#[test]
fn list_rejects_version_specifiers() {
    let dir = tempfile::tempdir().expect("workspace");
    write_registry_config(dir.path(), "http://localhost:4873/");

    let output = stage(dir.path(), &["list", "pkg@1.0.0"]);

    assert_failure_with_code(&output, "ERR_PNPM_STAGE_VERSION_SPECIFIER_UNSUPPORTED");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Version specifiers are not supported for listing staged packages"),
        "stderr: {stderr}",
    );
}

#[test]
fn approve_and_reject_send_the_configured_otp_and_stage_headers() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let approve_mock = server
        .mock("POST", format!("/-/stage/{STAGE_ID}/approve").as_str())
        .match_header("npm-auth-type", "web")
        .match_header("npm-command", "stage")
        .match_header("npm-otp", "123456")
        .with_status(201)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();
    let reject_mock = server
        .mock("DELETE", format!("/-/stage/{STAGE_ID}").as_str())
        .match_header("npm-auth-type", "web")
        .match_header("npm-command", "stage")
        .match_header("npm-otp", "123456")
        .with_status(204)
        .expect(1)
        .create();

    let approve = stage(dir.path(), &["approve", STAGE_ID, "--otp", "123456"]);
    approve_mock.assert();
    assert_success(&approve);
    assert_eq!(
        String::from_utf8_lossy(&approve.stdout),
        format!("Staged package {STAGE_ID} approved and published successfully.\n"),
    );

    let reject = stage(dir.path(), &["reject", STAGE_ID, "--otp", "123456"]);
    reject_mock.assert();
    assert_success(&reject);
    let stdout = String::from_utf8_lossy(&reject.stdout);
    assert!(
        stdout.contains(&format!("Staged package {STAGE_ID} has been rejected.")),
        "stdout: {stdout}",
    );
}

#[test]
fn approve_maps_a_web_auth_challenge_to_the_non_interactive_error() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    server
        .mock("POST", format!("/-/stage/{STAGE_ID}/approve").as_str())
        .with_status(401)
        .with_body(
            json!({
                "authUrl": "https://www.npmjs.com/auth/cli/test-auth-id",
                "doneUrl": "https://registry.example.com/-/v1/done?authId=test-auth-id",
            })
            .to_string(),
        )
        .create();

    // The spawned binary has no TTY, so the web-auth challenge cannot be
    // driven interactively.
    let output = stage(dir.path(), &["approve", STAGE_ID]);

    assert_failure_with_code(&output, "ERR_PNPM_OTP_NON_INTERACTIVE");
}

#[test]
fn approve_surfaces_a_plain_401_as_a_stage_registry_error() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    server
        .mock("POST", format!("/-/stage/{STAGE_ID}/approve").as_str())
        .with_status(401)
        .with_header("www-authenticate", r#"Basic realm="example""#)
        .with_body(r#"{"error":"unauthorized"}"#)
        .create();

    let output = stage(dir.path(), &["approve", STAGE_ID]);

    assert_failure_with_code(&output, "ERR_PNPM_STAGE_REGISTRY_ERROR");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("Failed to approve staged package {STAGE_ID}")),
        "stderr: {stderr}",
    );
    // miette wraps long lines, so the status clause is asserted separately.
    assert!(stderr.contains("(status 401 Unauthorized)"), "stderr: {stderr}");
}

#[test]
fn download_writes_the_staged_tarball_and_prints_keyed_json() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let tarball = gzipped_tarball(&[(
        "package/package.json",
        r#"{"name":"@scope/stage-download-json","version":"1.0.0"}"#,
    )]);
    server
        .mock("GET", format!("/-/stage/{STAGE_ID}/tarball").as_str())
        .with_header("content-type", "application/octet-stream")
        .with_body(&tarball)
        .create();

    let output = stage(dir.path(), &["download", STAGE_ID, "--json", "--reporter=silent"]);

    assert_success(&output);
    let keyed: Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("keyed JSON output");
    let expected_filename = format!("scope-stage-download-json-1.0.0-{STAGE_ID}.tgz");
    let summary = &keyed["@scope/stage-download-json"];
    assert_eq!(summary["name"], "@scope/stage-download-json");
    assert_eq!(summary["version"], "1.0.0");
    assert_eq!(summary["filename"], Value::String(expected_filename.clone()));
    let written = fs::read(dir.path().join(&expected_filename)).expect("the tarball is written");
    assert_eq!(written, tarball);
}

#[test]
fn download_rejects_traversal_through_the_tarball_manifest_version() {
    let dir = tempfile::tempdir().expect("workspace");
    let outside_base = "stage-download-outside-version";
    let tarball = gzipped_tarball(&[(
        "package/package.json",
        &json!({
            "name": "@scope/stage-download-version",
            "version": format!("1.0.0/../../{outside_base}"),
        })
        .to_string(),
    )]);
    let (registry, _server, download_dir) = download_registry(dir.path(), &tarball);
    let outside_path = outside_tarball_path(&download_dir, &format!("{outside_base}-{STAGE_ID}"));

    let output = stage(&download_dir, &["download", STAGE_ID]);

    assert_failure_with_code(&output, "ERR_PNPM_INVALID_PACKAGE_VERSION");
    assert!(!outside_path.exists(), "nothing may be written outside the download dir");
    assert_download_dir_untouched(&download_dir);
    drop(registry);
}

#[test]
fn download_rejects_traversal_through_the_tarball_manifest_name() {
    let dir = tempfile::tempdir().expect("workspace");
    let outside_base = "stage-download-outside-name";
    let tarball = gzipped_tarball(&[(
        "package/package.json",
        &json!({
            "name": format!("@scope/../../{outside_base}"),
            "version": "1.0.0",
        })
        .to_string(),
    )]);
    let (registry, _server, download_dir) = download_registry(dir.path(), &tarball);
    let outside_path =
        outside_tarball_path(&download_dir, &format!("{outside_base}-1.0.0-{STAGE_ID}"));

    let output = stage(&download_dir, &["download", STAGE_ID]);

    assert_failure_with_code(&output, "ERR_PNPM_INVALID_PACKAGE_NAME");
    assert!(!outside_path.exists(), "nothing may be written outside the download dir");
    assert_download_dir_untouched(&download_dir);
    drop(registry);
}

#[test]
fn a_missing_subcommand_is_rejected_with_the_stage_error_code() {
    let dir = tempfile::tempdir().expect("workspace");
    write_registry_config(dir.path(), "http://localhost:4873/");

    let output = stage(dir.path(), &[]);

    assert_failure_with_code(&output, "ERR_PNPM_STAGE_SUBCOMMAND_REQUIRED");
}

#[test]
fn an_unknown_subcommand_is_rejected_with_the_stage_error_code() {
    let dir = tempfile::tempdir().expect("workspace");
    write_registry_config(dir.path(), "http://localhost:4873/");

    let output = stage(dir.path(), &["frobnicate"]);

    assert_failure_with_code(&output, "ERR_PNPM_STAGE_UNKNOWN_SUBCOMMAND");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(r#"Unknown stage subcommand "frobnicate""#), "stderr: {stderr}");
}

#[test]
fn view_requires_a_uuid_stage_id() {
    let dir = tempfile::tempdir().expect("workspace");
    write_registry_config(dir.path(), "http://localhost:4873/");

    let missing = stage(dir.path(), &["view"]);
    assert_failure_with_code(&missing, "ERR_PNPM_STAGE_ID_REQUIRED");

    let invalid = stage(dir.path(), &["view", "not-a-uuid"]);
    assert_failure_with_code(&invalid, "ERR_PNPM_INVALID_STAGE_ID");
}

/// A registry that serves `tarball` for the staged download, plus a fresh
/// download workspace pointing at it.
fn download_registry(
    root: &Path,
    tarball: &[u8],
) -> (mockito::Mock, mockito::ServerGuard, PathBuf) {
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", format!("/-/stage/{STAGE_ID}/tarball").as_str())
        .with_header("content-type", "application/octet-stream")
        .with_body(tarball)
        .create();
    let download_dir = root.join("download");
    fs::create_dir_all(&download_dir).expect("create the download dir");
    write_registry_config(&download_dir, &registry);
    (mock, server, download_dir)
}

fn outside_tarball_path(download_dir: &Path, basename: &str) -> PathBuf {
    download_dir.parent().expect("the download dir has a parent").join(format!("{basename}.tgz"))
}

/// After a rejected download the workspace must hold only its `.npmrc`.
fn assert_download_dir_untouched(download_dir: &Path) {
    let entries: Vec<String> = fs::read_dir(download_dir)
        .expect("read the download dir")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, [".npmrc"], "no tarball may be written on a rejected download");
}

fn gzipped_tarball(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, contents) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, contents.as_bytes()).expect("append tar entry");
    }
    let tar = builder.into_inner().expect("finish the tar archive");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar).expect("gzip the tarball");
    encoder.finish().expect("finish the gzip stream")
}
