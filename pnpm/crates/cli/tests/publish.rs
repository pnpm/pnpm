//! Single-package `pacquet publish` integration tests: drive the real binary
//! against a `mockito` registry. The pnpr `TestRegistry` runs in proxy mode and
//! rejects path-less publishes ("routes to an upstream registry"), so a mocked
//! `PUT` is the portable harness here — it exercises the whole
//! pack → build-document → `PUT /:pkg` path, mirroring pnpm's
//! `test/publish/publish.ts` scenarios that don't need OIDC / provenance / OTP.
//!
//! CI env is cleared on every spawn so the binary's OIDC id-token probe stays
//! offline and deterministic (outside a supported CI it resolves to "no token"
//! without a network request).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn pacquet(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_env("GITHUB_ACTIONS")
        .without_env("GITLAB_CI")
        .without_env("NPM_ID_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_URL")
}

/// Run `pacquet publish --no-git-checks` in `workspace` with `args` appended.
fn publish(workspace: &Path, args: &[&str]) -> std::process::Output {
    pacquet(workspace)
        .with_arg("publish")
        .with_arg("--no-git-checks")
        .with_args(args)
        .output()
        .expect("spawn pacquet publish")
}

fn write_project(dir: &Path, registry: &str, manifest: &Value) {
    fs::write(dir.join(".npmrc"), format!("registry={registry}\n")).expect("write .npmrc");
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "publish must succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn publish_puts_the_package_document_with_dist_tag_and_attachment() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-pkg", "version": "1.0.0" }),
    );

    let mock = server
        .mock("PUT", "/test-publish-pkg")
        .match_body(Matcher::AllOf(vec![
            Matcher::PartialJsonString(
                r#"{"name":"test-publish-pkg","dist-tags":{"latest":"1.0.0"}}"#.to_owned(),
            ),
            // The tarball rides along base64-encoded under `_attachments`.
            Matcher::PartialJsonString(
                r#"{"_attachments":{"test-publish-pkg-1.0.0.tgz":{"content_type":"application/octet-stream"}}}"#
                    .to_owned(),
            ),
        ]))
        .with_status(200)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();

    assert_success(&publish(dir.path(), &[]));
    mock.assert();
}

#[test]
fn dry_run_uploads_nothing() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-dry", "version": "1.0.0" }),
    );

    // Any PUT during a dry run is a failure: the mock expects zero hits.
    let mock = server.mock("PUT", Matcher::Any).expect(0).create();

    let output = publish(dir.path(), &["--dry-run"]);
    assert_success(&output);
    // The dry-run notice rides the `pnpm` reporter channel, which shares stdout.
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(combined.contains("dry run"), "a dry run should announce itself; output: {combined}");
    mock.assert();
}

#[test]
fn publish_config_registry_overrides_the_default() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut default_registry = mockito::Server::new();
    let mut publish_registry = mockito::Server::new();
    write_project(
        dir.path(),
        &format!("{}/", default_registry.url()),
        &json!({
            "name": "test-publish-override",
            "version": "1.0.0",
            "publishConfig": { "registry": format!("{}/", publish_registry.url()) },
        }),
    );

    let default_mock = default_registry.mock("PUT", Matcher::Any).expect(0).create();
    let publish_mock = publish_registry
        .mock("PUT", "/test-publish-override")
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    assert_success(&publish(dir.path(), &[]));
    default_mock.assert();
    publish_mock.assert();
}

#[test]
fn tag_flag_registers_the_version_under_that_dist_tag() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-tag", "version": "2.3.4" }),
    );

    let mock = server
        .mock("PUT", "/test-publish-tag")
        .match_body(Matcher::PartialJsonString(r#"{"dist-tags":{"next":"2.3.4"}}"#.to_owned()))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    assert_success(&publish(dir.path(), &["--tag", "next"]));
    mock.assert();
}

#[test]
fn scoped_package_publishes_to_the_slash_escaped_path() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(dir.path(), &registry, &json!({ "name": "@scope/pkg", "version": "1.0.0" }));

    // npm publishes a scoped package to the `%2f`-escaped path.
    let mock = server
        .mock("PUT", "/@scope%2fpkg")
        .match_body(Matcher::PartialJsonString(r#"{"access":"public"}"#.to_owned()))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    assert_success(&publish(dir.path(), &["--access", "public"]));
    mock.assert();
}

#[test]
fn publish_from_a_prebuilt_tarball() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-tgz", "version": "1.0.0" }),
    );

    // Build a tarball with `pacquet pack`, then publish it by path.
    let pack = pacquet(dir.path()).with_arg("pack").output().expect("spawn pacquet pack");
    assert!(pack.status.success(), "pack stderr: {}", String::from_utf8_lossy(&pack.stderr));
    let tarball = "test-publish-tgz-1.0.0.tgz";
    assert!(dir.path().join(tarball).exists(), "pack should write {tarball}");

    let mock =
        server.mock("PUT", "/test-publish-tgz").with_status(200).with_body("{}").expect(1).create();

    assert_success(&publish(dir.path(), &[tarball]));
    mock.assert();
}

#[test]
fn publish_a_directory_argument_publishes_that_package() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    fs::write(dir.path().join(".npmrc"), format!("registry={registry}\n")).expect("write .npmrc");
    let package_dir = dir.path().join("subpkg");
    fs::create_dir(&package_dir).expect("create package dir");
    fs::write(
        package_dir.join("package.json"),
        json!({ "name": "test-publish-subdir", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let mock = server
        .mock("PUT", "/test-publish-subdir")
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    assert_success(&publish(dir.path(), &["subpkg"]));
    mock.assert();
}

/// A registry that rejects the `PUT` with a 5xx makes `pacquet publish` fail
/// with the failed-to-publish error — the response completed, so this is not a
/// transport error.
#[test]
fn errors_when_the_registry_rejects_the_publish() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-rejected", "version": "1.0.0" }),
    );
    server.mock("PUT", "/test-publish-rejected").with_status(500).with_body("boom").create();

    let output = publish(dir.path(), &[]);
    assert!(!output.status.success(), "a 5xx registry response must fail the publish");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("test-publish-rejected"),
        "the failure should name the package; stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn errors_when_publishing_a_nonexistent_tarball() {
    let dir = tempfile::tempdir().expect("workspace");
    let registry = "https://registry.example/";
    fs::write(dir.path().join(".npmrc"), format!("registry={registry}\n")).expect("write .npmrc");

    let output = publish(dir.path(), &["does-not-exist.tgz"]);
    assert!(!output.status.success(), "publishing a missing tarball must fail");
}

#[test]
fn json_flag_prints_the_per_package_summary() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-json", "version": "1.0.0" }),
    );
    server.mock("PUT", "/test-publish-json").with_status(200).with_body("{}").create();

    let output = publish(dir.path(), &["--json"]);
    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(r#""id": "test-publish-json@1.0.0""#),
        "--json must print the per-package summary; stdout: {stdout}",
    );
}

/// `prepublishOnly` runs through `sh -c` before packing, so a script that writes
/// a file leaves it in the package dir; the publish `PUT` still happens.
#[cfg(unix)]
#[test]
fn runs_the_publish_lifecycle_scripts() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({
            "name": "test-publish-scripts",
            "version": "1.0.0",
            "scripts": { "prepublishOnly": "echo ok > prepublish-ran.txt" },
        }),
    );
    let mock = server
        .mock("PUT", "/test-publish-scripts")
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    assert_success(&publish(dir.path(), &[]));
    assert!(
        dir.path().join("prepublish-ran.txt").exists(),
        "prepublishOnly should have run and written its marker",
    );
    mock.assert();
}

/// `--ignore-scripts` suppresses the publish-lifecycle scripts, but the publish
/// `PUT` still happens.
#[cfg(unix)]
#[test]
fn ignore_scripts_skips_the_publish_lifecycle_scripts() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({
            "name": "test-publish-noscripts",
            "version": "1.0.0",
            "scripts": { "prepublishOnly": "echo ok > prepublish-ran.txt" },
        }),
    );
    let mock = server
        .mock("PUT", "/test-publish-noscripts")
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    assert_success(&publish(dir.path(), &["--ignore-scripts"]));
    assert!(
        !dir.path().join("prepublish-ran.txt").exists(),
        "prepublishOnly must not run under --ignore-scripts",
    );
    mock.assert();
}
