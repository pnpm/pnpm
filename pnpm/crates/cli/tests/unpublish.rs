//! `pacquet unpublish` removes a package, or a range of its versions, from
//! the registry.
//!
//! Ported from the upstream suite `registry-access/commands/test/unpublish.ts`
//! plus the error paths its handler defines: the missing-name / not-found /
//! no-versions / no-matching-versions errors, the `--force` protection for a
//! full unpublish (also when a range matches every version), the partial
//! unpublish `PUT` (versions removed, dist-tags re-pointed, `latest`
//! reassigned), tolerated tarball-delete 404s, and the 405/401 registry
//! answers.
//!
//! The registry is a `mockito` server; an empty `--npmrc-auth-file` keeps the
//! developer's real `~/.npmrc` from influencing the test.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn empty_auth_file(root: &Path) -> PathBuf {
    let auth_file = root.join("auth-npmrc");
    fs::write(&auth_file, "").expect("write empty auth .npmrc");
    auth_file
}

fn run_unpublish(
    workspace: &Path,
    auth_file: &Path,
    registry: Option<&str>,
    params: &[&str],
) -> std::process::Output {
    let mut command = pacquet_at(workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("unpublish");
    if let Some(registry) = registry {
        command = command.with_arg("--registry").with_arg(registry);
    }
    for param in params {
        command = command.with_arg(param);
    }
    command.output().expect("spawn pacquet unpublish")
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A two-version packument whose tarballs live on `server_url`, with `latest`
/// and a `beta` tag both pointing at 0.0.1.
fn two_version_packument(server_url: &str) -> String {
    json!({
        "name": "test-pkg",
        "_rev": "3-abc",
        "dist-tags": { "latest": "0.0.1", "beta": "0.0.1" },
        "versions": {
            "0.0.1": { "dist": { "tarball": format!("{server_url}/test-pkg/-/test-pkg-0.0.1.tgz") } },
            "0.0.2": { "dist": { "tarball": format!("{server_url}/test-pkg/-/test-pkg-0.0.2.tgz") } },
        },
        "readme": "hello",
    })
    .to_string()
}

#[test]
fn fails_when_package_is_not_provided() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());

    let output = run_unpublish(&workspace, &auth_file, None, &[]);

    assert!(!output.status.success(), "a bare unpublish must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_UNPUBLISH_REQUIRED"), "{stderr}");
    assert!(stderr.contains("Package name is required"), "{stderr}");
    drop(root);
}

#[test]
fn fails_when_package_is_not_found() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let get_mock = server.mock("GET", "/nonexistent-package-99999").with_status(404).create();
    let auth_file = empty_auth_file(root.path());

    let output =
        run_unpublish(&workspace, &auth_file, Some(&registry), &["nonexistent-package-99999"]);

    get_mock.assert();
    assert!(!output.status.success(), "a 404 must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_PACKAGE_NOT_FOUND"), "{stderr}");
    assert!(
        stderr.contains(r#"Package "nonexistent-package-99999" not found in registry"#),
        "{stderr}",
    );
    drop((root, server));
}

#[test]
fn fails_when_the_package_has_no_versions() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let get_mock = server
        .mock("GET", "/test-pkg")
        .with_status(200)
        .with_body(r#"{"name":"test-pkg","versions":{}}"#)
        .create();
    let auth_file = empty_auth_file(root.path());

    let output = run_unpublish(&workspace, &auth_file, Some(&registry), &["test-pkg"]);

    get_mock.assert();
    assert!(!output.status.success(), "an empty packument must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_NO_VERSIONS"), "{stderr}");
    drop((root, server));
}

#[test]
fn fails_when_no_versions_match_the_range() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let get_mock = server
        .mock("GET", "/test-pkg")
        .with_status(200)
        .with_body(two_version_packument(&server.url()))
        .create();
    let auth_file = empty_auth_file(root.path());

    let output = run_unpublish(&workspace, &auth_file, Some(&registry), &["test-pkg@9.9.9"]);

    get_mock.assert();
    assert!(!output.status.success(), "a non-matching range must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_NO_MATCHING_VERSIONS"), "{stderr}");
    assert!(stderr.contains(r#"No versions match "9.9.9""#), "{stderr}");
    drop((root, server));
}

#[test]
fn refuses_a_full_unpublish_without_force() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let get_mock = server
        .mock("GET", "/test-pkg")
        .with_status(200)
        .with_body(two_version_packument(&server.url()))
        .create();
    let delete_mock = server.mock("DELETE", "/test-pkg/-rev/3-abc").expect(0).create();
    let auth_file = empty_auth_file(root.path());

    let output = run_unpublish(&workspace, &auth_file, Some(&registry), &["test-pkg"]);

    get_mock.assert();
    delete_mock.assert();
    assert!(!output.status.success(), "a full unpublish without --force must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_UNPUBLISH_CONFIRM"), "{stderr}");
    assert!(stderr.contains("pnpm unpublish --force"), "{stderr}");
    assert!(stderr.contains("0.0.1, 0.0.2"), "the versions are listed: {stderr}");
    drop((root, server));
}

#[test]
fn a_range_matching_every_version_is_a_full_unpublish() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let get_mock = server
        .mock("GET", "/test-pkg")
        .with_status(200)
        .with_body(two_version_packument(&server.url()))
        .create();
    let auth_file = empty_auth_file(root.path());

    let output = run_unpublish(&workspace, &auth_file, Some(&registry), &["test-pkg@>=0.0.1"]);

    get_mock.assert();
    assert!(!output.status.success(), "removing every version needs --force too");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_UNPUBLISH_CONFIRM"), "{stderr}");
    drop((root, server));
}

#[test]
fn force_unpublishes_the_entire_package() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let get_mock = server
        .mock("GET", "/test-pkg")
        .with_status(200)
        .with_body(two_version_packument(&server.url()))
        .create();
    let delete_mock =
        server.mock("DELETE", "/test-pkg/-rev/3-abc").with_status(200).with_body("{}").create();
    let auth_file = empty_auth_file(root.path());

    let output = run_unpublish(&workspace, &auth_file, Some(&registry), &["test-pkg", "--force"]);

    get_mock.assert();
    delete_mock.assert();
    assert!(output.status.success(), "{}", stderr_of(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Successfully unpublished all 2 version(s) of test-pkg"), "{stdout}");
    drop((root, server));
}

#[test]
fn unpublishes_a_specific_version_and_repoints_dist_tags() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let server_url = server.url();

    // The refetch before each tarball delete hits GET again.
    let get_mock = server
        .mock("GET", "/test-pkg")
        .with_status(200)
        .with_body(two_version_packument(&server_url))
        .expect_at_least(2)
        .create();
    // 0.0.1 disappears; the `beta` tag pointing at it is dropped and `latest`
    // is re-pointed at the highest remaining version.
    let put_mock = server
        .mock("PUT", "/test-pkg/-rev/3-abc")
        .match_header("content-type", "application/json")
        .match_body(Matcher::Json(json!({
            "name": "test-pkg",
            "_rev": "3-abc",
            "dist-tags": { "latest": "0.0.2" },
            "versions": {
                "0.0.2": { "dist": { "tarball": format!("{server_url}/test-pkg/-/test-pkg-0.0.2.tgz") } },
            },
            "readme": "hello",
        })))
        .with_status(200)
        .with_body("{}")
        .create();
    let tarball_delete_mock = server
        .mock("DELETE", "/test-pkg/-/test-pkg-0.0.1.tgz/-rev/3-abc")
        .with_status(200)
        .with_body("{}")
        .create();
    let auth_file = empty_auth_file(root.path());

    let output = run_unpublish(&workspace, &auth_file, Some(&registry), &["test-pkg@0.0.1"]);

    get_mock.assert();
    put_mock.assert();
    tarball_delete_mock.assert();
    assert!(output.status.success(), "{}", stderr_of(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Successfully unpublished 1 version(s) of test-pkg"), "{stdout}");
    drop((root, server));
}

#[test]
fn a_tarball_delete_404_is_tolerated() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let get_mock = server
        .mock("GET", "/test-pkg")
        .with_status(200)
        .with_body(two_version_packument(&server.url()))
        .expect_at_least(2)
        .create();
    let put_mock =
        server.mock("PUT", "/test-pkg/-rev/3-abc").with_status(200).with_body("{}").create();
    // Some registries clean tarballs up on the packument update themselves.
    let tarball_delete_mock = server
        .mock("DELETE", "/test-pkg/-/test-pkg-0.0.1.tgz/-rev/3-abc")
        .with_status(404)
        .create();
    let auth_file = empty_auth_file(root.path());

    let output = run_unpublish(&workspace, &auth_file, Some(&registry), &["test-pkg@0.0.1"]);

    get_mock.assert();
    put_mock.assert();
    tarball_delete_mock.assert();
    assert!(output.status.success(), "{}", stderr_of(&output));
    drop((root, server));
}

#[test]
fn a_405_on_a_full_unpublish_reports_unpublish_forbidden() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let get_mock = server
        .mock("GET", "/test-pkg")
        .with_status(200)
        .with_body(two_version_packument(&server.url()))
        .create();
    let delete_mock = server.mock("DELETE", "/test-pkg/-rev/3-abc").with_status(405).create();
    let auth_file = empty_auth_file(root.path());

    let output = run_unpublish(&workspace, &auth_file, Some(&registry), &["test-pkg", "--force"]);

    get_mock.assert();
    delete_mock.assert();
    assert!(!output.status.success(), "a 405 must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_UNPUBLISH_FORBIDDEN"), "{stderr}");
    assert!(stderr.contains("cannot be completely unpublished"), "{stderr}");
    drop((root, server));
}

#[test]
fn an_unauthorized_delete_reports_unauthorized() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let get_mock = server
        .mock("GET", "/test-pkg")
        .with_status(200)
        .with_body(two_version_packument(&server.url()))
        .create();
    let delete_mock = server
        .mock("DELETE", "/test-pkg/-rev/3-abc")
        .with_status(401)
        .with_body(r#"{"error":"unauthorized"}"#)
        .create();
    let auth_file = empty_auth_file(root.path());

    let output = run_unpublish(&workspace, &auth_file, Some(&registry), &["test-pkg", "--force"]);

    get_mock.assert();
    delete_mock.assert();
    assert!(!output.status.success(), "a 401 must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_UNAUTHORIZED"), "{stderr}");
    assert!(stderr.contains("You must be logged in to unpublish packages"), "{stderr}");
    drop((root, server));
}

#[test]
fn the_force_hint_lists_versions_in_packument_order() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    // 1.10.0 after 1.9.0 in the packument; lexicographic order would flip
    // them, the TypeScript CLI's Object.keys does not.
    let get_mock = server
        .mock("GET", "/test-pkg")
        .with_status(200)
        .with_body(
            r#"{"name":"test-pkg","_rev":"3-abc","versions":{"1.9.0":{},"1.10.0":{},"1.2.0":{}}}"#,
        )
        .create();
    let auth_file = empty_auth_file(root.path());

    let output = run_unpublish(&workspace, &auth_file, Some(&registry), &["test-pkg"]);

    get_mock.assert();
    assert!(!output.status.success(), "a full unpublish without --force must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("1.9.0, 1.10.0, 1.2.0"), "packument order survives: {stderr}");
    drop((root, server));
}
