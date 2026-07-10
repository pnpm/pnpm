//! `pacquet bugs` / `pacquet issues` — open the bug tracker URL of a package
//! in the browser.
//!
//! Covers the error paths that never reach the browser: no `package.json`,
//! no derivable bugs URL, and a registry package without one. The
//! URL-opening happy paths are covered by the unit tests in
//! `src/cli_args/bugs/tests.rs` through the `OpenUrl` seam — running them
//! against the real binary would launch the developer's browser.

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

fn empty_auth_file(root: &Path) -> PathBuf {
    let auth_file = root.join("auth-npmrc");
    fs::write(&auth_file, "").expect("write empty auth .npmrc");
    auth_file
}

fn run_bugs(workspace: &Path, auth_file: &Path, args: &[&str]) -> std::process::Output {
    let mut command =
        pacquet_at(workspace).with_arg("--npmrc-auth-file").with_arg(auth_file).with_arg("bugs");
    for arg in args {
        command = command.with_arg(arg);
    }
    command.output().expect("spawn pacquet bugs")
}

#[test]
fn fails_when_no_bugs_url_can_be_derived() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());
    fs::write(workspace.join("package.json"), r#"{"name":"test-pkg"}"#)
        .expect("write package.json");

    let output = run_bugs(&workspace, &auth_file, &[]);

    assert!(
        !output.status.success(),
        "bugs must fail when no URL can be derived (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_NO_BUGS_URL"),
        "stderr must contain ERR_PNPM_NO_BUGS_URL; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn fails_when_no_package_json_exists() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());

    let output = run_bugs(&workspace, &auth_file, &[]);

    assert!(
        !output.status.success(),
        "bugs must fail when no package.json exists (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND"),
        "stderr must contain ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn fails_when_registry_package_has_no_bugs_url() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let body = serde_json::json!({
        "name": "no-bugs-pkg",
        "version": "1.0.0",
        "dist": {
            "tarball": "https://example.com/pkg.tgz",
        },
    })
    .to_string();
    let mock = server.mock("GET", "/no-bugs-pkg/latest").with_status(200).with_body(&body).create();

    fs::write(workspace.join(".npmrc"), format!("registry={registry}\n"))
        .expect("write project .npmrc");
    let auth_file = empty_auth_file(root.path());

    let output = run_bugs(&workspace, &auth_file, &["no-bugs-pkg"]);

    mock.assert();
    assert!(
        !output.status.success(),
        "bugs must fail when package has no bugs URL (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_NO_BUGS_URL"),
        "stderr must contain ERR_PNPM_NO_BUGS_URL; got:\n{stderr}",
    );
    drop((root, server));
}
