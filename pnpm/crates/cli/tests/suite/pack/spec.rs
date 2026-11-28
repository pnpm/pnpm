//! `pack <spec>`: packing a positional package spec — a registry package,
//! a tarball URL, a local directory, or a local tarball file — instead of
//! the project in the working directory. Registry tests run against the
//! mocked registry; URL tests against a `mockito` server.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils_registry::{
    bin::{
        AddMockedRegistry,
        CommandTempCwd,
    },
    fixtures::minimal_tarball,
};
use serde_json::{
    Value,
    json,
};
use std::{
    fs,
    path::Path,
};

#[test]
fn packs_a_registry_package_spec() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let output = pacquet
        .with_args(["pack", "@pnpm.e2e/foo@1.0.0", "--json"])
        .assert()
        .success();

    let tarball = workspace.join("pnpm.e2e-foo-1.0.0.tgz");
    assert!(tarball.exists(), "the downloaded tarball lands in the working directory");
    let result: Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.get_output().stdout)).unwrap();
    assert_eq!(result["name"], "@pnpm.e2e/foo");
    assert_eq!(result["version"], "1.0.0");
    assert_eq!(result["filename"], "pnpm.e2e-foo-1.0.0.tgz");
    assert!(
        result["integrity"]
            .as_str()
            .is_some_and(|i| i.starts_with("sha512-"))
    );
    assert!(result["size"].as_u64().unwrap() > 0);
    assert!(
        result["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["path"] == "package.json")
    );
    assert_eq!(result["manifest"]["name"], "@pnpm.e2e/foo");

    drop(root);
}

#[test]
fn packs_a_registry_range_spec() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_args(["pack", "@pnpm.e2e/foo@^1", "--json"])
        .assert()
        .success();

    assert!(
        workspace.join("pnpm.e2e-foo-1.3.0.tgz").exists(),
        "the range resolves to the highest matching fixture version",
    );

    drop(root);
}

#[test]
fn packs_a_tarball_url_without_a_tgz_suffix() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let tarball = minimal_tarball("vite", "0.0.0-pr-24234");
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/vite@24234")
        .with_status(200)
        .with_body(tarball.clone())
        .create();
    let url = format!("{}/vite@24234", server.url());

    pacquet
        .with_args(["pack", &url])
        .assert()
        .success();

    mock.assert();
    let written = workspace.join("vite-0.0.0-pr-24234.tgz");
    assert!(written.exists(), "the tarball is named from its own manifest");
    assert_eq!(fs::read(&written).unwrap(), tarball, "the bytes are written through");

    drop((root, server));
}

#[test]
fn registry_spec_fails_on_integrity_mismatch() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = root.path().join("auth-npmrc");
    fs::write(&auth_file, "").unwrap();
    let mut server = mockito::Server::new();
    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url())).unwrap();
    let tarball = minimal_tarball("foo", "1.0.0");
    let tarball_url = format!("{}/foo/-/foo-1.0.0.tgz", server.url());
    // The SHA-512 of the empty string: well-formed, but not the tarball's.
    let wrong_integrity = "sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg==";
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": { "tarball": tarball_url, "integrity": wrong_integrity },
            }
        },
    })
    .to_string();
    server
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument)
        .create();
    server
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(tarball)
        .create();

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_args(["pack", "foo@1.0.0"])
        .output()
        .expect("spawn pacquet pack");

    assert!(!output.status.success(), "an integrity mismatch must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_TARBALL_INTEGRITY"),
        "stderr must name the integrity diagnostic; got:\n{stderr}",
    );
    assert!(!workspace.join("foo-1.0.0.tgz").exists(), "nothing written on mismatch");

    drop((root, server));
}

#[test]
fn packs_a_local_directory_spec_into_the_working_directory() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let sub = workspace.join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(
        sub.join("package.json"),
        json!({ "name": "sub-pkg", "version": "1.0.0" }).to_string(),
    )
    .unwrap();
    fs::write(sub.join("index.js"), "console.log()\n").unwrap();

    pacquet
        .with_args(["pack", "./sub"])
        .assert()
        .success();

    assert!(
        workspace.join("sub-pkg-1.0.0.tgz").exists(),
        "the tarball lands in the working directory, not the packed one",
    );
    assert!(!sub.join("sub-pkg-1.0.0.tgz").exists());

    drop(root);
}

#[test]
fn packs_a_local_tarball_file_spec() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let tarball = minimal_tarball("local-pkg", "2.3.4");
    fs::write(workspace.join("input.tgz"), &tarball).unwrap();

    pacquet
        .with_args(["pack", "./input.tgz", "--pack-destination", "out", "--json"])
        .assert()
        .success();

    let written = workspace.join("out/local-pkg-2.3.4.tgz");
    assert!(written.exists(), "the tarball is renamed from its manifest");
    assert_eq!(fs::read(&written).unwrap(), tarball);

    drop(root);
}

#[test]
fn packs_multiple_specs_as_a_json_array() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("a.tgz"), minimal_tarball("pkg-a", "1.0.0")).unwrap();
    fs::write(workspace.join("b.tgz"), minimal_tarball("pkg-b", "2.0.0")).unwrap();

    let output = pacquet
        .with_args(["pack", "./a.tgz", "./b.tgz", "--json"])
        .assert()
        .success();

    let results: Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.get_output().stdout)).unwrap();
    let results = results.as_array().expect("multiple specs report a JSON array");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["name"], "pkg-a");
    assert_eq!(results[1]["name"], "pkg-b");
    assert!(workspace.join("pkg-a-1.0.0.tgz").exists());
    assert!(workspace.join("pkg-b-2.0.0.tgz").exists());

    drop(root);
}

#[test]
fn dry_run_reports_a_url_spec_without_writing() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(minimal_tarball("dry-pkg", "1.0.0"))
        .create();
    let url = format!("{}/pkg.tgz", server.url());

    let output = pacquet
        .with_args(["pack", &url, "--dry-run", "--json"])
        .assert()
        .success();

    let result: Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.get_output().stdout)).unwrap();
    assert_eq!(result["name"], "dry-pkg");
    assert!(result.get("integrity").is_none(), "a dry run reports no digest");
    assert!(!workspace.join("dry-pkg-1.0.0.tgz").exists(), "a dry run writes nothing");

    drop((root, server));
}

#[test]
fn rejects_a_git_spec() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_args(["pack", "user/repo"])
        .output()
        .expect("spawn pacquet pack");

    assert!(!output.status.success(), "a git spec must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("packing a git dependency is not supported yet"),
        "stderr must say why; got:\n{stderr}",
    );

    drop(root);
}

#[test]
fn rejects_a_missing_local_path() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_args(["pack", "./no-such-dir"])
        .output()
        .expect("spawn pacquet pack");

    assert!(!output.status.success(), "a missing local path must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no-such-dir"), "stderr must name the path; got:\n{stderr}");

    drop(root);
}

#[test]
fn rejects_recursive_with_a_spec() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_args(["-r", "pack", "./whatever"])
        .output()
        .expect("spawn pacquet pack");

    assert!(!output.status.success(), "--recursive with a spec must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--recursive"),
        "stderr must name the conflicting option; got:\n{stderr}",
    );

    drop(root);
}

fn pacquet_at(workspace: &Path) -> std::process::Command {
    std::process::Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
}
