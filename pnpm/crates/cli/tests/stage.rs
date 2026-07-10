//! `pacquet stage` integration tests: drive the real binary end-to-end
//! against a hosted in-process pnpr for the staging lifecycle, and against a
//! `mockito` registry only for the faults a well-behaved registry cannot
//! produce (OTP challenges, hostile tarball manifests, a misbehaving
//! pagination `total`, exact header/no-upload assertions). Mirrors pnpm's
//! `releasing/commands/test/stage.test.ts`.
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

// --------------------------------------------------------------------
// End-to-end lifecycle against a hosted in-process pnpr. The shared
// `pacquet_testing_utils` registry runs in proxy mode and rejects
// publishes, so these tests serve their own static-mode instance.
// --------------------------------------------------------------------

/// A hosted pnpr on an ephemeral localhost port, backed by a fresh
/// storage tempdir (returned so it outlives the test).
fn spawn_hosted_registry() -> (String, tempfile::TempDir) {
    let storage = tempfile::tempdir().expect("registry storage");
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("bind the stage e2e registry to an unused localhost port");
    listener.set_nonblocking(true).expect("set the registry listener to nonblocking");
    let listen = listener.local_addr().expect("read the registry listener address");
    let url = format!("http://{listen}/");
    let mut config = pnpr::Config::static_serve(listen, storage.path().to_path_buf());
    config.public_url = url.trim_end_matches('/').to_string();
    config.auth.htpasswd.max_users = pnpr::MaxUsers::Unlimited;
    std::thread::Builder::new()
        .name("stage-e2e-registry".to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("create the registry runtime");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener)
                    .expect("create the registry tokio listener");
                pnpr::serve_listener(config, listener).await.expect("serve the stage e2e registry");
            });
        })
        .expect("spawn the registry thread");
    (url, storage)
}

/// Run one async request against the e2e registry from this synchronous test.
fn block_on<Output>(future: impl std::future::Future<Output = Output>) -> Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create the request runtime")
        .block_on(future)
}

/// An HTTP client whose requests time out instead of hanging the test run
/// if the hosted registry stops responding.
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_mins(1))
        .build()
        .expect("build the test HTTP client")
}

/// Register a user against the e2e registry and return their bearer token.
fn add_user(registry: &str) -> String {
    let url = format!("{registry}-/user/org.couchdb.user:alice");
    let body = json!({
        "_id": "org.couchdb.user:alice",
        "name": "alice",
        "password": "secret",
        "email": "alice@example.com",
        "type": "user",
        "roles": [],
    });
    block_on(async {
        let response =
            http_client().put(&url).json(&body).send().await.expect("send the adduser request");
        assert_eq!(response.status().as_u16(), 201, "adduser must succeed");
        let payload: Value = response.json().await.expect("parse the adduser response");
        payload["token"].as_str().expect("token in the adduser response").to_owned()
    })
}

/// The HTTP status of a plain packument read, bypassing the CLI.
fn packument_status(registry: &str, token: &str, name: &str) -> u16 {
    let url = format!("{registry}{name}");
    block_on(async {
        http_client()
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .expect("send the packument request")
            .status()
            .as_u16()
    })
}

/// A workspace whose `.npmrc` points at the e2e registry, plus an auth file
/// carrying the registered user's token.
fn e2e_workspace(dir: &Path, registry: &str, token: &str, manifest: &Value) -> PathBuf {
    write_project(dir, registry, manifest);
    let host = registry.strip_prefix("http://").unwrap_or(registry);
    let auth_file = dir.join("auth-npmrc");
    fs::write(&auth_file, format!("//{host}:_authToken={token}\n")).expect("write auth .npmrc");
    auth_file
}

fn stage_with_auth(workspace: &Path, auth_file: &Path, args: &[&str]) -> std::process::Output {
    pacquet(workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("stage")
        .with_args(args)
        .output()
        .expect("spawn pacquet stage")
}

/// The whole staged-publishing happy path against a real registry: stage a
/// scoped package, observe it held back, inspect and download it, approve
/// it, and see it become installable with the staged record gone.
#[test]
fn stage_lifecycle_against_pnpr_publishes_only_on_approval() {
    let (registry, _storage) = spawn_hosted_registry();
    let token = add_user(&registry);
    let dir = tempfile::tempdir().expect("workspace");
    let auth_file = e2e_workspace(
        dir.path(),
        &registry,
        &token,
        &json!({ "name": "@stage-e2e/lifecycle", "version": "1.0.0" }),
    );

    let publish = stage_with_auth(
        dir.path(),
        &auth_file,
        &["publish", "--json", "--no-git-checks", "--reporter=silent"],
    );
    assert_success(&publish);
    let keyed: Value =
        serde_json::from_str(&String::from_utf8_lossy(&publish.stdout)).expect("keyed JSON output");
    let summary = &keyed["@stage-e2e/lifecycle"];
    assert_eq!(summary["version"], "1.0.0");
    let stage_id = summary["stageId"].as_str().expect("a stage id").to_owned();

    // Held back: not installable until approved.
    assert_eq!(packument_status(&registry, &token, "@stage-e2e/lifecycle"), 404);

    let list = stage_with_auth(dir.path(), &auth_file, &["list", "--json", "--reporter=silent"]);
    assert_success(&list);
    let listed: Value =
        serde_json::from_str(&String::from_utf8_lossy(&list.stdout)).expect("list JSON output");
    assert_eq!(listed[0]["id"], Value::String(stage_id.clone()));
    assert_eq!(listed[0]["packageName"], "@stage-e2e/lifecycle");
    assert_eq!(listed[0]["tag"], "latest");
    assert_eq!(listed[0]["actor"], "alice");

    let view = stage_with_auth(dir.path(), &auth_file, &["view", &stage_id]);
    assert_success(&view);
    let stdout = String::from_utf8_lossy(&view.stdout);
    assert!(stdout.contains("package name: @stage-e2e/lifecycle"), "stdout: {stdout}");
    assert!(stdout.contains("staged by: alice (user)"), "stdout: {stdout}");

    let download = stage_with_auth(
        dir.path(),
        &auth_file,
        &["download", &stage_id, "--json", "--reporter=silent"],
    );
    assert_success(&download);
    let downloaded: Value = serde_json::from_str(&String::from_utf8_lossy(&download.stdout))
        .expect("download JSON output");
    let expected_filename = format!("stage-e2e-lifecycle-1.0.0-{stage_id}.tgz");
    assert_eq!(
        downloaded["@stage-e2e/lifecycle"]["filename"],
        Value::String(expected_filename.clone()),
    );
    assert!(dir.path().join(&expected_filename).exists(), "the tarball must be written");

    let approve = stage_with_auth(dir.path(), &auth_file, &["approve", &stage_id]);
    assert_success(&approve);
    assert_eq!(
        String::from_utf8_lossy(&approve.stdout),
        format!("Staged package {stage_id} approved and published successfully.\n"),
    );

    assert_eq!(packument_status(&registry, &token, "@stage-e2e/lifecycle"), 200);
    let list = stage_with_auth(dir.path(), &auth_file, &["list"]);
    assert_success(&list);
    assert_eq!(
        String::from_utf8_lossy(&list.stdout),
        "No staged packages found.\n",
        "an approved stage leaves no record behind",
    );
}

/// Rejecting a staged publish deletes it without ever publishing, and the
/// non-JSON `stage publish` output carries the stage id to act on.
#[test]
fn stage_reject_against_pnpr_deletes_the_staged_publish() {
    let (registry, _storage) = spawn_hosted_registry();
    let token = add_user(&registry);
    let dir = tempfile::tempdir().expect("workspace");
    let auth_file = e2e_workspace(
        dir.path(),
        &registry,
        &token,
        &json!({ "name": "stage-e2e-rejected", "version": "1.0.0" }),
    );

    let publish = stage_with_auth(
        dir.path(),
        &auth_file,
        &["publish", "--no-git-checks", "--reporter=silent"],
    );
    assert_success(&publish);
    let stdout = String::from_utf8_lossy(&publish.stdout);
    let stage_id = stdout
        .trim()
        .strip_prefix("+ stage-e2e-rejected@1.0.0 (staged with id ")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or_else(|| panic!("staged line must carry the id; stdout: {stdout}"))
        .to_owned();

    let reject = stage_with_auth(dir.path(), &auth_file, &["reject", &stage_id]);
    assert_success(&reject);
    let stdout = String::from_utf8_lossy(&reject.stdout);
    assert!(
        stdout.contains(&format!("Staged package {stage_id} has been rejected.")),
        "stdout: {stdout}",
    );

    assert_eq!(packument_status(&registry, &token, "stage-e2e-rejected"), 404);
    let view = stage_with_auth(dir.path(), &auth_file, &["view", &stage_id]);
    assert_failure_with_code(&view, "ERR_PNPM_STAGE_REGISTRY_ERROR");
    let list = stage_with_auth(dir.path(), &auth_file, &["list", "stage-e2e-rejected"]);
    assert_success(&list);
    assert_eq!(
        String::from_utf8_lossy(&list.stdout),
        "No staged versions of package name \"stage-e2e-rejected\".\n",
    );
}

/// The client's pagination loop crosses page boundaries against the real
/// registry: 101 staged records arrive in one 100-item page plus one more.
#[test]
fn stage_list_paginates_against_pnpr() {
    let (registry, _storage) = spawn_hosted_registry();
    let token = add_user(&registry);
    let dir = tempfile::tempdir().expect("workspace");
    let auth_file = e2e_workspace(
        dir.path(),
        &registry,
        &token,
        &json!({ "name": "stage-e2e-paginated", "version": "1.0.0" }),
    );

    // Seed 101 staged records directly over HTTP; the client under test is
    // the list loop, not the publisher.
    block_on(async {
        let client = http_client();
        for index in 0..101 {
            let name = format!("stage-e2e-paginated-{index:03}");
            let doc = json!({
                "_id": name,
                "name": name,
                "dist-tags": { "latest": "1.0.0" },
                "versions": { "1.0.0": { "name": name, "version": "1.0.0" } },
                "_attachments": {
                    format!("{name}-1.0.0.tgz"): { "data": "dGFyYmFsbA==", "length": 7 }
                },
            });
            let response = client
                .post(format!("{registry}-/stage/package/{name}"))
                .bearer_auth(&token)
                .json(&doc)
                .send()
                .await
                .expect("send the stage request");
            assert_eq!(response.status().as_u16(), 201, "staging {name} must succeed");
        }
    });

    let list = stage_with_auth(dir.path(), &auth_file, &["list", "--json", "--reporter=silent"]);
    assert_success(&list);
    let listed: Value =
        serde_json::from_str(&String::from_utf8_lossy(&list.stdout)).expect("list JSON output");
    assert_eq!(listed.as_array().map(Vec::len), Some(101));
}
