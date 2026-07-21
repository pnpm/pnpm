//! `pacquet view` (aliases `info`, `show`, `v`) prints package metadata
//! from the registry: a formatted summary, a JSON dump (`--json`), or
//! selected fields.
//!
//! Covered here: command structure, the missing-name / non-registry /
//! not-found / no-matching-version errors, field selection (single, nested,
//! multiple, JSON), the summary sections (header, bin, dist, dist-tags, deps
//! count, deprecation, published-by), and the nearest-manifest fallback when
//! the package name is omitted.
//!
//! The registry is a `mockito` server the spawned `pacquet` connects to over
//! loopback, so each test serves a crafted packument and asserts on the exact
//! rendered output. An empty `--npmrc-auth-file` replaces the developer's real
//! `~/.npmrc` so a token or `registry=` already on the machine can't influence
//! the test. Output is captured through a pipe, so `supports-color` is off and
//! the assertions match plain (un-colored) text — the CLI omits styling when
//! stdout is not a TTY.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// An empty user-level `.npmrc`, returned for `--npmrc-auth-file`, so the
/// developer's real `~/.npmrc` cannot leak a registry or token into the test.
fn empty_auth_file(root: &Path) -> PathBuf {
    let auth_file = root.join("auth-npmrc");
    fs::write(&auth_file, "").expect("write empty auth .npmrc");
    auth_file
}

/// Point the workspace's project `.npmrc` at `registry`.
fn write_registry_npmrc(workspace: &Path, registry: &str) {
    fs::write(workspace.join(".npmrc"), format!("registry={registry}\n"))
        .expect("write project .npmrc");
}

fn run_view(workspace: &Path, auth_file: &Path, args: &[&str]) -> std::process::Output {
    pacquet_at(workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("view")
        .with_args(args.iter().copied())
        .output()
        .expect("spawn pacquet view")
}

fn stdout_of(output: &std::process::Output) -> String {
    assert!(
        output.status.success(),
        "view must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A full-metadata packument for `is-negative` with two live versions, a
/// licence, description, maintainer, dist block, and per-version publish
/// times — enough to exercise every summary section.
fn is_negative_body() -> String {
    serde_json::json!({
        "name": "is-negative",
        "dist-tags": { "latest": "2.1.0" },
        "versions": {
            "1.0.0": {
                "name": "is-negative",
                "version": "1.0.0",
                "license": "MIT",
                "description": "Check if a number is negative",
                "homepage": "https://github.com/kevva/is-negative#readme",
                "maintainers": [{ "name": "kevva", "email": "kevva@example.com" }],
                "dist": {
                    "shasum": "1d06e1c0aa697471e487f3f32c39ba8a6b485e1e",
                    "tarball": "https://registry.npmjs.org/is-negative/-/is-negative-1.0.0.tgz",
                    "integrity": "sha512-1negative1.0.0integrityplaceholdervaluexxxxxxxxxxxxxxxxxx==",
                    "unpackedSize": 1234
                }
            },
            "2.1.0": {
                "name": "is-negative",
                "version": "2.1.0",
                "license": "MIT",
                "description": "Check if a number is negative",
                "maintainers": [{ "name": "kevva", "email": "kevva@example.com" }],
                "dist": {
                    "shasum": "2c3a6e3e3e3e3e3e3e3e3e3e3e3e3e3e3e3e3e3e",
                    "tarball": "https://registry.npmjs.org/is-negative/-/is-negative-2.1.0.tgz"
                }
            }
        },
        "time": {
            "1.0.0": "2015-01-01T00:00:00.000Z",
            "2.1.0": "2016-01-01T00:00:00.000Z"
        }
    })
    .to_string()
}

/// Serve `body` at `GET /<path>` and return the mock so the test can keep the
/// server alive for the duration of the request.
fn serve(server: &mut mockito::Server, path: &str, body: &str) -> mockito::Mock {
    server.mock("GET", path).with_status(200).with_body(body).expect_at_least(1).create()
}

#[test]
fn aliases_are_recognised() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    for alias in ["view", "info", "show", "v"] {
        let output = pacquet_at(&workspace)
            .with_args([alias, "--help"])
            .output()
            .unwrap_or_else(|err| panic!("run pacquet {alias} --help: {err}"));
        assert!(output.status.success(), "{alias} --help should succeed");
    }
    drop(root);
}

#[test]
fn fails_without_package_name_or_manifest() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &[]);

    assert!(!output.status.success(), "view without a package name must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_MISSING_PACKAGE_NAME"),
        "stderr must name the missing-package-name diagnostic; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn rejects_non_registry_spec() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["github:user/repo"]);

    assert!(!output.status.success(), "a non-registry spec must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_INVALID_PACKAGE_NAME"),
        "stderr must name the invalid-package-name diagnostic; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn prints_summary_for_a_package() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative"]);

    mock.assert();
    let stdout = stdout_of(&output);
    assert!(stdout.contains("is-negative"), "summary must mention the package: {stdout:?}");
    drop((root, server));
}

#[test]
fn package_not_found_is_fetch_404() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = server.mock("GET", "/not-a-real-package").with_status(404).create();
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["not-a-real-package"]);

    mock.assert();
    assert!(!output.status.success(), "a 404 must fail the command");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_FETCH_404"),
        "stderr must name the fetch-404 diagnostic; got:\n{stderr}",
    );
    drop((root, server));
}

#[test]
fn no_matching_version_errors() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative@99999.0.0"]);

    mock.assert();
    assert!(!output.status.success(), "an unsatisfiable version must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_PACKAGE_NOT_FOUND"),
        "stderr must name the package-not-found diagnostic; got:\n{stderr}",
    );
    drop((root, server));
}

#[test]
fn json_output_parses_and_carries_name() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative", "--json"]);

    mock.assert();
    let parsed: Value = serde_json::from_str(&stdout_of(&output)).expect("output is valid JSON");
    assert_eq!(parsed["name"], "is-negative");
    drop((root, server));
}

#[test]
fn single_field_prints_plain_value() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative", "name"]);

    mock.assert();
    assert_eq!(stdout_of(&output).trim(), "is-negative");
    drop((root, server));
}

#[test]
fn specific_version_field() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative@1.0.0", "version"]);

    mock.assert();
    assert_eq!(stdout_of(&output).trim(), "1.0.0");
    drop((root, server));
}

#[test]
fn multiple_fields_quote_strings() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative@1.0.0", "name", "version"]);

    mock.assert();
    let stdout = stdout_of(&output);
    assert!(stdout.contains("name = 'is-negative'"), "quoted name expected: {stdout:?}");
    assert!(stdout.contains("version = '1.0.0'"), "quoted version expected: {stdout:?}");
    drop((root, server));
}

#[test]
fn version_range_resolves_to_matching_version() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative@^1.0.0", "version"]);

    mock.assert();
    assert_eq!(stdout_of(&output).trim(), "1.0.0");
    drop((root, server));
}

#[test]
fn dist_tag_resolves() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative@latest", "version"]);

    mock.assert();
    assert_eq!(stdout_of(&output).trim(), "2.1.0");
    drop((root, server));
}

#[test]
fn nested_field_selection() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative@1.0.0", "dist.shasum"]);

    mock.assert();
    assert_eq!(stdout_of(&output).trim(), "1d06e1c0aa697471e487f3f32c39ba8a6b485e1e");
    drop((root, server));
}

#[test]
fn field_selection_with_json() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output =
        run_view(&workspace, &auth_file, &["is-negative@1.0.0", "name", "version", "--json"]);

    mock.assert();
    let parsed: Value = serde_json::from_str(&stdout_of(&output)).expect("valid JSON");
    assert_eq!(parsed["name"], "is-negative");
    assert_eq!(parsed["version"], "1.0.0");
    drop((root, server));
}

#[test]
fn single_field_json_unwraps_value() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative@1.0.0", "name", "--json"]);

    mock.assert();
    let parsed: Value = serde_json::from_str(&stdout_of(&output)).expect("valid JSON");
    assert_eq!(parsed, Value::String("is-negative".to_string()));
    drop((root, server));
}

#[test]
fn object_field_renders_as_json() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative@1.0.0", "dist"]);

    mock.assert();
    let parsed: Value = serde_json::from_str(&stdout_of(&output)).expect("dist renders as JSON");
    assert!(parsed["tarball"].is_string(), "dist.tarball present: {parsed:?}");
    assert!(parsed["shasum"].is_string(), "dist.shasum present: {parsed:?}");
    drop((root, server));
}

#[test]
fn versions_field_returns_array() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative", "versions"]);

    mock.assert();
    let parsed: Value =
        serde_json::from_str(&stdout_of(&output)).expect("versions is a JSON array");
    let versions = parsed.as_array().expect("array");
    assert!(versions.contains(&Value::String("1.0.0".to_string())), "{versions:?}");
    drop((root, server));
}

#[test]
fn dist_tags_field_returns_mapping() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative", "dist-tags", "--json"]);

    mock.assert();
    let parsed: Value = serde_json::from_str(&stdout_of(&output)).expect("valid JSON");
    assert_eq!(parsed["latest"], "2.1.0");
    drop((root, server));
}

#[test]
fn time_field_returns_publish_timestamps() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative", "time", "--json"]);

    mock.assert();
    let parsed: Value = serde_json::from_str(&stdout_of(&output)).expect("valid JSON");
    assert!(parsed["1.0.0"].is_string(), "per-version time present: {parsed:?}");
    drop((root, server));
}

#[test]
fn summary_header_dist_and_published_sections() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative@1.0.0"]);

    mock.assert();
    let stdout = stdout_of(&output);
    let first_line = stdout.lines().next().unwrap_or_default();
    assert!(first_line.contains("is-negative@1.0.0"), "header: {first_line:?}");
    assert!(first_line.contains("deps: none"), "no-deps package: {first_line:?}");
    assert!(stdout.contains(".tarball:"), "dist tarball: {stdout:?}");
    assert!(stdout.contains(".shasum:"), "dist shasum: {stdout:?}");
    assert!(stdout.contains("published "), "published line: {stdout:?}");
    assert!(stdout.contains(" ago by "), "published-by line: {stdout:?}");
    drop((root, server));
}

#[test]
fn summary_dist_tags_section() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative"]);

    mock.assert();
    let stdout = stdout_of(&output);
    assert!(stdout.contains("dist-tags:"), "dist-tags section: {stdout:?}");
    assert!(stdout.contains("latest:"), "latest tag: {stdout:?}");
    drop((root, server));
}

#[test]
fn summary_shows_deps_count_and_deprecation() {
    let body = serde_json::json!({
        "name": "demo-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "demo-pkg",
                "version": "1.0.0",
                "deprecated": "use something else",
                "dependencies": { "left-pad": "^1.0.0" },
                "dist": { "tarball": "https://example.com/demo-pkg-1.0.0.tgz" }
            }
        }
    })
    .to_string();

    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/demo-pkg", &body);
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["demo-pkg@1.0.0"]);

    mock.assert();
    let stdout = stdout_of(&output);
    let first_line = stdout.lines().next().unwrap_or_default();
    assert!(first_line.contains("deps: "), "deps count: {first_line:?}");
    assert!(!first_line.contains("deps: none"), "should not be none: {first_line:?}");
    assert!(stdout.contains("DEPRECATED! - use something else"), "deprecation: {stdout:?}");
    drop((root, server));
}

#[test]
fn summary_bin_from_object_and_string() {
    let object_bin = serde_json::json!({
        "name": "demo-bin",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "demo-bin",
                "version": "1.0.0",
                "bin": { "touch-file-one-bin": "index.js" },
                "dist": { "tarball": "https://example.com/demo-bin-1.0.0.tgz" }
            }
        }
    })
    .to_string();
    let string_bin = serde_json::json!({
        "name": "@demo/scoped-bin",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@demo/scoped-bin",
                "version": "1.0.0",
                "bin": "index.js",
                "dist": { "tarball": "https://example.com/scoped-bin-1.0.0.tgz" }
            }
        }
    })
    .to_string();

    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let object_mock = serve(&mut server, "/demo-bin", &object_bin);
    let string_mock = serve(&mut server, "/@demo%2Fscoped-bin", &string_bin);
    let auth_file = empty_auth_file(root.path());

    let object_output = run_view(&workspace, &auth_file, &["demo-bin@1.0.0"]);
    object_mock.assert();
    assert!(
        stdout_of(&object_output).contains("bin: touch-file-one-bin"),
        "object bin lists its key",
    );

    let string_output = run_view(&workspace, &auth_file, &["@demo/scoped-bin"]);
    string_mock.assert();
    assert!(
        stdout_of(&string_output).contains("bin: scoped-bin"),
        "string bin derives the scope-stripped name",
    );
    drop((root, server));
}

#[test]
fn scoped_package_lookup() {
    let body = serde_json::json!({
        "name": "@demo/pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@demo/pkg",
                "version": "1.0.0",
                "dist": { "tarball": "https://example.com/pkg-1.0.0.tgz" }
            }
        }
    })
    .to_string();

    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/@demo%2Fpkg", &body);
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["@demo/pkg@1.0.0", "name"]);

    mock.assert();
    assert_eq!(stdout_of(&output).trim(), "@demo/pkg");
    drop((root, server));
}

#[test]
fn uses_manifest_name_when_package_omitted() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    fs::write(workspace.join("package.json"), r#"{"name":"is-negative"}"#)
        .expect("write package.json");
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &[]);

    mock.assert();
    assert!(stdout_of(&output).contains("is-negative"), "summary derived from manifest name");
    drop((root, server));
}

#[test]
fn searches_upward_for_manifest() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    fs::write(workspace.join("package.json"), r#"{"name":"is-negative"}"#)
        .expect("write package.json");
    let nested = workspace.join("a").join("b");
    fs::create_dir_all(&nested).expect("create nested dir");
    write_registry_npmrc(&nested, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&nested, &auth_file, &[]);

    mock.assert();
    assert!(stdout_of(&output).contains("is-negative"), "manifest found by upward search");
    drop((root, server));
}

#[test]
fn manifest_without_name_is_invalid_package_json() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"version":"1.0.0"}"#)
        .expect("write package.json");
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &[]);

    assert!(!output.status.success(), "a manifest with no name must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_INVALID_PACKAGE_JSON"),
        "stderr must name the invalid-package-json diagnostic; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn non_object_manifest_is_invalid_package_json() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "null").expect("write package.json");
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &[]);

    assert!(!output.status.success(), "a non-object manifest must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_INVALID_PACKAGE_JSON"),
        "stderr must name the invalid-package-json diagnostic; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn versions_field_with_json_returns_array() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative", "versions", "--json"]);

    mock.assert();
    let parsed: Value =
        serde_json::from_str(&stdout_of(&output)).expect("versions is a JSON array");
    let versions = parsed.as_array().expect("array");
    assert!(versions.contains(&Value::String("1.0.0".to_string())), "{versions:?}");
    drop((root, server));
}

#[test]
fn resolves_manifest_from_dir_flag_when_cwd_differs() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    fs::write(workspace.join("package.json"), r#"{"name":"is-negative"}"#)
        .expect("write package.json");
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    // Run from the parent dir but point `-C` at the workspace: the nearest
    // manifest is resolved from `--dir`, not the process cwd.
    let output = pacquet_at(root.path())
        .with_arg("-C")
        .with_arg(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("view")
        .output()
        .expect("spawn pacquet view");

    mock.assert();
    assert!(stdout_of(&output).contains("is-negative"), "manifest resolved from --dir");
    drop((root, server));
}

#[test]
fn derives_name_when_engines_pnpm_incompatible() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"is-negative","engines":{"pnpm":"999.0.0"}}"#,
    )
    .expect("write package.json");
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &[]);

    mock.assert();
    assert!(
        stdout_of(&output).contains("is-negative"),
        "view reads the manifest name without enforcing engines.pnpm",
    );
    drop((root, server));
}

#[test]
#[ignore = "pacquet's project-manifest reader (pnpm_workspace::try_read_project_manifest) \
            reads package.json only; package.yaml / package.json5 are not supported yet"]
fn uses_package_yaml_name_when_package_omitted() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    fs::write(workspace.join("package.yaml"), "name: is-negative\n").expect("write package.yaml");
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &[]);

    mock.assert();
    assert!(stdout_of(&output).contains("is-negative"), "summary derived from package.yaml name");
    drop((root, server));
}

#[test]
fn absent_field_produces_no_output() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/is-negative", &is_negative_body());
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["is-negative", "no-such-field"]);

    mock.assert();
    // An absent field renders as an empty string, so nothing is printed.
    assert_eq!(stdout_of(&output), "");
    drop((root, server));
}

#[test]
fn object_author_collapses_to_its_name() {
    let body = serde_json::json!({
        "name": "authored",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "authored",
                "version": "1.0.0",
                "author": { "name": "Jane Doe", "email": "jane@example.com" },
                "dist": { "tarball": "https://example.com/authored-1.0.0.tgz" }
            }
        }
    })
    .to_string();

    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));
    let mock = serve(&mut server, "/authored", &body);
    let auth_file = empty_auth_file(root.path());

    let output = run_view(&workspace, &auth_file, &["authored@1.0.0", "author"]);

    mock.assert();
    assert_eq!(stdout_of(&output).trim(), "Jane Doe");
    drop((root, server));
}
