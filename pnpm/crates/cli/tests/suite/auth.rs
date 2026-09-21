use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::CommandTempCwd,
    fixtures::{minimal_tarball, sha512_integrity},
};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn write_project_config(root: &Path, workspace: &Path, registry: &str, credentials: &str) {
    fs::write(workspace.join(".npmrc"), format!("registry={registry}/\n{credentials}"))
        .expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "storeDir: ../store\ncacheDir: ../cache\nenableGlobalVirtualStore: false\nfetchRetries: 0\n",
    )
    .expect("write workspace config");
    fs::create_dir_all(root.join("xdg")).expect("create isolated config home");
}

fn install_command(workspace: &Path, root: &Path) -> Command {
    pacquet_at(workspace)
        .with_env("XDG_CONFIG_HOME", root.join("xdg"))
        .with_env("NO_PROXY", "127.0.0.1,localhost")
        .with_env("no_proxy", "127.0.0.1,localhost")
}

fn assert_authenticated_install(
    package: &str,
    credentials: impl FnOnce(&str) -> String,
    expected_header: &str,
    frozen_reinstall: bool,
) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let registry_url = registry.url();
    let authority = registry_url.strip_prefix("http://").expect("mock registry is HTTP");
    write_project_config(root.path(), &workspace, &registry_url, &credentials(authority));

    let tarball = minimal_tarball(package, "1.0.0");
    let integrity = sha512_integrity(&tarball);
    let tarball_path = "/private-pkg-1.0.0.tgz";
    let packument_path = format!("/{}", package.replace('/', "%2F"));
    let packument = serde_json::json!({
        "name": package,
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": package,
                "version": "1.0.0",
                "dist": {
                    "integrity": integrity,
                    "tarball": format!("{registry_url}{tarball_path}"),
                },
            },
        },
    });
    let metadata = registry
        .mock("GET", packument_path.as_str())
        .match_header("authorization", expected_header)
        .with_status(200)
        .with_header("content-type", "application/vnd.npm.install-v1+json")
        .with_body(packument.to_string())
        .expect_at_least(1)
        .create();
    let tarballs = registry
        .mock("GET", tarball_path)
        .match_header("authorization", expected_header)
        .with_status(200)
        .with_body(tarball)
        .expect_at_least(if frozen_reinstall { 2 } else { 1 })
        .create();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { (package): "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    install_command(&workspace, root.path())
        .with_arg("install")
        .assert()
        .success();
    assert!(
        workspace
            .join("node_modules")
            .join(package)
            .exists(),
    );

    if frozen_reinstall {
        fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
        fs::remove_dir_all(root.path().join("store")).expect("remove cold store");
        install_command(&workspace, root.path())
            .with_args(["install", "--frozen-lockfile"])
            .assert()
            .success();
        assert!(
            workspace
                .join("node_modules")
                .join(package)
                .exists(),
        );
    }

    metadata.assert();
    tarballs.assert();
}

#[test]
fn bearer_auth_is_used_for_metadata_tarballs_and_cold_frozen_reinstall() {
    assert_authenticated_install(
        "private-pkg",
        |authority| format!("//{authority}/:_authToken=secret-token\n"),
        "Bearer secret-token",
        true,
    );
}

#[test]
fn username_and_password_authenticates_install() {
    assert_authenticated_install(
        "private-pkg",
        |authority| format!("//{authority}/:username=foo\n//{authority}/:_password=YmFy\n"),
        "Basic Zm9vOmJhcg==",
        false,
    );
}

#[test]
fn legacy_basic_auth_authenticates_install() {
    assert_authenticated_install(
        "private-pkg",
        |authority| format!("//{authority}/:_auth=Zm9vOmJhcg==\n"),
        "Basic Zm9vOmJhcg==",
        false,
    );
}

/// An `_auth` written without its `=` padding authenticates the same as
/// the canonical spelling: the header carries the credential the value
/// decodes to, not the value as written (pnpm/pnpm#14257).
#[test]
fn unpadded_legacy_basic_auth_authenticates_install() {
    assert_authenticated_install(
        "private-pkg",
        |authority| format!("//{authority}/:_auth=Zm9vOmJhcg\n"),
        "Basic Zm9vOmJhcg==",
        false,
    );
}

#[test]
fn package_scope_bearer_auth_wins_for_scoped_install() {
    assert_authenticated_install(
        "@private/foo",
        |authority| {
            format!(
                "//{authority}/:_authToken=wrong-token\n\
                 //{authority}/:@private:_authToken=scoped-token\n",
            )
        },
        "Bearer scoped-token",
        true,
    );
}

#[test]
fn package_scope_legacy_auth_wins_for_scoped_install() {
    assert_authenticated_install(
        "@private/foo",
        |authority| {
            format!(
                "//{authority}/:_authToken=wrong-token\n\
                 //{authority}/:@private:_auth=Zm9vOmJhcg==\n",
            )
        },
        "Basic Zm9vOmJhcg==",
        true,
    );
}

#[test]
fn metadata_authorization_failure_is_reported() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    write_project_config(root.path(), &workspace, &registry.url(), "");
    let forbidden = registry
        .mock("GET", "/private-pkg")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(403)
        .with_body("Forbidden")
        .expect(1)
        .create();
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"private-pkg":"1.0.0"}}"#)
        .expect("write package.json");

    let output = install_command(&workspace, root.path())
        .with_arg("install")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_FETCH_403"), "got {stderr}");
    assert!(stderr.contains("Forbidden - 403"), "got {stderr}");
    assert!(
        stderr.contains("No authorization header was set for the request."),
        "the report must say which credential, if any, was sent: {stderr}",
    );

    forbidden.assert();
}

/// A registry configured with inline `user:pass@` basic-auth must not
/// leak either half into the fetch error, and the report must still name
/// the credential that was sent — `AuthHeaders` derives a `Basic` header
/// from that userinfo, so a hint computed against the redacted URL would
/// claim no header was set.
#[test]
fn inline_registry_credentials_are_redacted_but_still_reported() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let authority = registry.url().replace("http://", "");
    write_project_config(
        root.path(),
        &workspace,
        &format!("http://basicuser:super-secret-password@{authority}"),
        "",
    );
    let missing = registry
        .mock("GET", "/private-pkg")
        .with_status(404)
        .with_body("Not Found")
        .expect(1)
        .create();
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"private-pkg":"1.0.0"}}"#)
        .expect("write package.json");

    let output = install_command(&workspace, root.path())
        .with_arg("install")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("stderr={stderr}");
    assert!(stderr.contains("ERR_PNPM_FETCH_404"), "got {stderr}");
    assert!(
        !stderr.contains("super-secret-password") && !stderr.contains("basicuser"),
        "the inline credentials must not reach the terminal: {stderr}",
    );
    assert!(
        stderr.contains("An authorization header was used: Basic "),
        "the header derived from the inline credentials must still be reported: {stderr}",
    );

    missing.assert();
}

#[test]
fn tarball_authorization_failure_is_reported() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let registry_url = registry.url();
    write_project_config(root.path(), &workspace, &registry_url, "");
    let tarball = minimal_tarball("private-pkg", "1.0.0");
    let integrity = sha512_integrity(&tarball);
    let metadata = registry
        .mock("GET", "/private-pkg")
        .with_status(200)
        .with_header("content-type", "application/vnd.npm.install-v1+json")
        .with_body(
            serde_json::json!({
                "name": "private-pkg",
                "dist-tags": { "latest": "1.0.0" },
                "versions": {
                    "1.0.0": {
                        "name": "private-pkg",
                        "version": "1.0.0",
                        "dist": {
                            "integrity": integrity,
                            "tarball": format!("{registry_url}/private-pkg-1.0.0.tgz"),
                        },
                    },
                },
            })
            .to_string(),
        )
        .expect_at_least(1)
        .create();
    let forbidden = registry
        .mock("GET", "/private-pkg-1.0.0.tgz")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(403)
        .with_body("Forbidden")
        .expect_at_least(1)
        .create();
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"private-pkg":"1.0.0"}}"#)
        .expect("write package.json");

    let output = install_command(&workspace, root.path())
        .with_arg("install")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("HTTP 403"), "got {stderr}");

    metadata.assert();
    forbidden.assert();
}

#[test]
fn scoped_registry_auth_env_warns_and_uses_configured_auth_for_frozen_verification() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let workspace = dunce::canonicalize(&workspace).expect("canonicalize workspace");
    let mut registry = mockito::Server::new();
    let registry_url = format!("{}/api/v4/projects/96/packages/npm", registry.url());
    let authority = registry_url.strip_prefix("http://").unwrap();
    let credentials = format!("//{authority}/:_authToken=${{REGISTRY_TOKEN}}\n");
    write_project_config(root.path(), &workspace, &registry.url(), "");
    fs::write(
        workspace.join(".npmrc"),
        format!("@private:registry={registry_url}/\n{credentials}"),
    )
    .unwrap();
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"@private/foo":"1.0.0"}}"#)
        .unwrap();
    let tarball = minimal_tarball("@private/foo", "1.0.0");
    let integrity = sha512_integrity(&tarball);
    let tarball_path = "/api/v4/projects/96/packages/npm/foo-1.0.0.tgz";
    let tarball_url = format!("{}{tarball_path}", registry.url());
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        format!(
            r"lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      '@private/foo':
        specifier: 1.0.0
        version: 1.0.0
packages:
  '@private/foo@1.0.0':
    resolution: {{integrity: {integrity}, tarball: {tarball_url}}}
snapshots:
  '@private/foo@1.0.0': {{}}
",
        ),
    )
    .unwrap();
    let packument_path = "/api/v4/projects/96/packages/npm/@private%2Ffoo";
    let unauthorized = registry
        .mock("GET", packument_path)
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(401)
        .expect(3)
        .create();
    for reporter in ["append-only", "ndjson", "silent"] {
        let output = install_command(&workspace, root.path())
            .with_env("REGISTRY_TOKEN", "secret-token")
            .with_args(["install", "--frozen-lockfile", "--reporter", reporter])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("stderr={stderr}");
        assert!(!output.status.success());
        assert!(stderr.contains("ERR_PNPM_META_FETCH_FAIL"), "got {stderr}");
        assert_eq!(stderr.matches("Ignored project-level auth setting").count(), 1);
        let stdout = String::from_utf8_lossy(&output.stdout);
        eprintln!("stdout={stdout}");
        assert!(!stdout.contains("Ignored project-level auth setting"));
        assert!(!stdout.contains("secret-token"));
        assert!(!stderr.contains("secret-token"), "got {stderr}");
    }
    unauthorized.assert();

    let metadata = registry
        .mock("GET", packument_path)
        .match_header("authorization", "Bearer secret-token")
        .with_status(200)
        .with_header("content-type", "application/vnd.npm.install-v1+json")
        .with_body(
            serde_json::json!({
                "name": "@private/foo",
                "dist-tags": { "latest": "1.0.0" },
                "versions": { "1.0.0": {
                    "name": "@private/foo",
                    "version": "1.0.0",
                    "dist": { "integrity": integrity, "tarball": tarball_url },
                } },
            })
            .to_string(),
        )
        .expect_at_least(2)
        .create();
    let tarballs = registry
        .mock("GET", tarball_path)
        .match_header("authorization", "Bearer secret-token")
        .with_status(200)
        .with_body(tarball)
        .expect(2)
        .create();
    let user_npmrc = root.path().join("user.npmrc");
    fs::write(&user_npmrc, credentials).unwrap();
    for auth_file in [workspace.join(".npmrc"), user_npmrc] {
        let uses_project_auth_file = auth_file == workspace.join(".npmrc");
        let output = install_command(&workspace, root.path())
            .with_env("REGISTRY_TOKEN", "secret-token")
            .with_env("PNPM_CONFIG_NPMRC_AUTH_FILE", auth_file)
            .with_args(["install", "--frozen-lockfile"])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("stderr={stderr}");
        assert!(output.status.success(), "got {stderr}");
        if uses_project_auth_file {
            assert!(!stderr.contains("Ignored project-level auth setting"), "got {stderr}");
        }
        assert!(workspace.join("node_modules/@private/foo").exists());
        fs::remove_dir_all(workspace.join("node_modules")).unwrap();
        fs::remove_dir_all(root.path().join("store")).unwrap();
        fs::remove_dir_all(root.path().join("cache")).unwrap();
    }
    metadata.assert();
    tarballs.assert();
}
