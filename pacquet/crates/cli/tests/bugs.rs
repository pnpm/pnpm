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

fn version_response(name: &str, extra_fields: &serde_json::Value) -> String {
    let mut version = serde_json::json!({
        "name": name,
        "version": "1.0.0",
        "dist": {
            "tarball": "https://example.com/pkg.tgz",
        },
    });
    if let Some(obj) = version.as_object_mut()
        && let Some(extra) = extra_fields.as_object()
    {
        for (k, v) in extra {
            obj.insert(k.clone(), v.clone());
        }
    }
    version.to_string()
}

#[test]
fn prints_bugs_url_from_local_manifest_bugs_object() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"test-pkg","bugs":{"url":"https://github.com/test/pkg/issues"}}"#,
    )
    .expect("write package.json");

    let output = run_bugs(&workspace, &auth_file, &[]);

    assert!(
        output.status.success(),
        "bugs must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "https://github.com/test/pkg/issues");
    drop(root);
}

#[test]
fn prints_bugs_url_from_local_manifest_bugs_string() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"test-pkg","bugs":"https://github.com/test/pkg/issues"}"#,
    )
    .expect("write package.json");

    let output = run_bugs(&workspace, &auth_file, &[]);

    assert!(
        output.status.success(),
        "bugs must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "https://github.com/test/pkg/issues");
    drop(root);
}

#[test]
fn prints_repository_issues_url_when_bugs_is_missing() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"test-pkg","repository":"https://github.com/test/pkg"}"#,
    )
    .expect("write package.json");

    let output = run_bugs(&workspace, &auth_file, &[]);

    assert!(
        output.status.success(),
        "bugs must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "https://github.com/test/pkg/issues");
    drop(root);
}

#[test]
fn normalizes_git_plus_https_repository_url_with_dot_git() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"test-pkg","repository":{"url":"git+https://github.com/test/pkg.git"}}"#,
    )
    .expect("write package.json");

    let output = run_bugs(&workspace, &auth_file, &[]);

    assert!(
        output.status.success(),
        "bugs must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "https://github.com/test/pkg/issues");
    drop(root);
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
fn looks_up_package_on_registry_by_name() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let body = version_response(
        "is-negative",
        &serde_json::json!({
            "bugs": { "url": "https://github.com/kevva/is-negative/issues" },
        }),
    );
    let mock = server.mock("GET", "/is-negative/latest").with_status(200).with_body(&body).create();

    fs::write(workspace.join(".npmrc"), format!("registry={registry}\n"))
        .expect("write project .npmrc");
    let auth_file = empty_auth_file(root.path());

    let output = run_bugs(&workspace, &auth_file, &["is-negative"]);

    mock.assert();
    assert!(
        output.status.success(),
        "bugs must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "https://github.com/kevva/is-negative/issues");
    drop((root, server));
}

#[test]
fn prints_repository_issues_url_from_registry_package() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let body = version_response(
        "test-pkg",
        &serde_json::json!({
            "repository": { "url": "git+https://github.com/test/pkg.git" },
        }),
    );
    let mock = server.mock("GET", "/test-pkg/latest").with_status(200).with_body(&body).create();

    fs::write(workspace.join(".npmrc"), format!("registry={registry}\n"))
        .expect("write project .npmrc");
    let auth_file = empty_auth_file(root.path());

    let output = run_bugs(&workspace, &auth_file, &["test-pkg"]);

    mock.assert();
    assert!(
        output.status.success(),
        "bugs must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "https://github.com/test/pkg/issues");
    drop((root, server));
}

#[test]
fn fails_when_registry_package_has_no_bugs_url() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let body = version_response("no-bugs-pkg", &serde_json::json!({}));
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

#[test]
fn encodes_scoped_package_name_in_registry_request() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let body = version_response(
        "@scope/pkg",
        &serde_json::json!({
            "bugs": { "url": "https://github.com/scope/pkg/issues" },
        }),
    );
    let mock =
        server.mock("GET", "/@scope%2Fpkg/latest").with_status(200).with_body(&body).create();

    fs::write(workspace.join(".npmrc"), format!("registry={registry}\n"))
        .expect("write project .npmrc");
    let auth_file = empty_auth_file(root.path());

    let output = run_bugs(&workspace, &auth_file, &["@scope/pkg"]);

    mock.assert();
    assert!(
        output.status.success(),
        "bugs must succeed for a scoped package (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "https://github.com/scope/pkg/issues");
    drop((root, server));
}
