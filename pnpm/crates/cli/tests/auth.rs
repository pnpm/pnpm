use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
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

    install_command(&workspace, root.path()).with_arg("install").assert().success();
    assert!(workspace.join("node_modules").join(package).exists());

    if frozen_reinstall {
        fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
        fs::remove_dir_all(root.path().join("store")).expect("remove cold store");
        install_command(&workspace, root.path())
            .with_args(["install", "--frozen-lockfile"])
            .assert()
            .success();
        assert!(workspace.join("node_modules").join(package).exists());
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

    let output = install_command(&workspace, root.path()).with_arg("install").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("403 Forbidden"), "got {stderr}");

    forbidden.assert();
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

    let output = install_command(&workspace, root.path()).with_arg("install").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("HTTP 403"), "got {stderr}");

    metadata.assert();
    forbidden.assert();
}
