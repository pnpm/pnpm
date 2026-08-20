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
use pnpm_testing_utils::command_env::CommandTestExt;
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
        // These tests turn on publish settings a developer may also have set
        // globally, so pin every layer below `pnpm-workspace.yaml` to nothing:
        // `XDG_CONFIG_HOME` points at the workspace, which has no `pnpm/`
        // subdirectory to read a global `config.yaml` from.
        .with_env("XDG_CONFIG_HOME", workspace)
        .without_ambient_pnpm_config()
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
fn publish_sends_the_readme_to_the_registry_as_metadata() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-readme", "version": "1.0.0" }),
    );
    fs::write(dir.path().join("README.md"), "# Hello").expect("write README");

    // The readme rides the version metadata even though `embed-readme` defaults to false.
    let mock = server
        .mock("PUT", "/test-publish-readme")
        .match_body(Matcher::PartialJsonString(
            r##"{"versions":{"1.0.0":{"readme":"# Hello"}}}"##.to_owned(),
        ))
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
fn workspace_yaml_supplies_the_dist_tag_and_access() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "@scope/from-yaml", "version": "2.3.4" }),
    );
    fs::write(dir.path().join("pnpm-workspace.yaml"), "tag: next\naccess: restricted\n")
        .expect("write pnpm-workspace.yaml");

    let mock = server
        .mock("PUT", "/@scope%2ffrom-yaml")
        .match_body(Matcher::AllOf(vec![
            Matcher::PartialJsonString(r#"{"dist-tags":{"next":"2.3.4"}}"#.to_owned()),
            Matcher::PartialJsonString(r#"{"access":"restricted"}"#.to_owned()),
        ]))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    assert_success(&publish(dir.path(), &[]));
    mock.assert();
}

#[test]
fn the_tag_flag_outranks_the_workspace_yaml_tag() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-tag-flag", "version": "1.2.3" }),
    );
    fs::write(dir.path().join("pnpm-workspace.yaml"), "tag: from-yaml\n")
        .expect("write pnpm-workspace.yaml");

    let mock = server
        .mock("PUT", "/test-publish-tag-flag")
        .match_body(Matcher::PartialJsonString(r#"{"dist-tags":{"from-flag":"1.2.3"}}"#.to_owned()))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    assert_success(&publish(dir.path(), &["--tag", "from-flag"]));
    mock.assert();
}

#[test]
fn pnpm_config_tag_env_var_outranks_the_workspace_yaml_tag() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-tag-env", "version": "1.2.3" }),
    );
    fs::write(dir.path().join("pnpm-workspace.yaml"), "tag: from-yaml\n")
        .expect("write pnpm-workspace.yaml");

    let mock = server
        .mock("PUT", "/test-publish-tag-env")
        .match_body(Matcher::PartialJsonString(r#"{"dist-tags":{"from-env":"1.2.3"}}"#.to_owned()))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    let output = pacquet(dir.path())
        .with_arg("publish")
        .with_arg("--no-git-checks")
        .with_env("PNPM_CONFIG_TAG", "from-env")
        .output()
        .expect("spawn pacquet publish");
    assert_success(&output);
    mock.assert();
}

/// The manifest's `publishConfig.access` is the fallback for an *unset*
/// access level, so a configured one has to win over it.
#[test]
fn the_configured_access_outranks_publish_config_access() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({
            "name": "@scope/access-precedence",
            "version": "1.0.0",
            "publishConfig": { "access": "public" },
        }),
    );
    fs::write(dir.path().join("pnpm-workspace.yaml"), "access: restricted\n")
        .expect("write pnpm-workspace.yaml");

    let mock = server
        .mock("PUT", "/@scope%2faccess-precedence")
        .match_body(Matcher::PartialJsonString(r#"{"access":"restricted"}"#.to_owned()))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    assert_success(&publish(dir.path(), &[]));
    mock.assert();
}

/// `--config.<key>=<value>` is applied after the whole config chain, so it has
/// to beat both `pnpm-workspace.yaml` and `PNPM_CONFIG_*`.
#[test]
fn the_config_override_outranks_the_workspace_yaml_and_the_env_var() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-config-override", "version": "1.2.3" }),
    );
    fs::write(dir.path().join("pnpm-workspace.yaml"), "tag: from-yaml\n")
        .expect("write pnpm-workspace.yaml");

    let mock = server
        .mock("PUT", "/test-publish-config-override")
        .match_body(Matcher::PartialJsonString(r#"{"dist-tags":{"from-cli":"1.2.3"}}"#.to_owned()))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    let output = pacquet(dir.path())
        .with_arg("--config.tag=from-cli")
        .with_arg("publish")
        .with_arg("--no-git-checks")
        .with_env("PNPM_CONFIG_TAG", "from-env")
        .output()
        .expect("spawn pacquet publish");
    assert_success(&output);
    mock.assert();
}

/// The OTP rides on the first `PUT` rather than waiting for a 2FA challenge,
/// so whichever channel supplies it is observable as a header. A one-time
/// password is not a file setting: `pnpm-workspace.yaml` supplies nothing,
/// `PNPM_CONFIG_OTP` supplies the header.
#[test]
fn the_otp_comes_from_the_environment_and_never_from_the_workspace_yaml() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-otp", "version": "1.0.0" }),
    );
    fs::write(dir.path().join("pnpm-workspace.yaml"), "otp: '135791'\n")
        .expect("write pnpm-workspace.yaml");

    let from_yaml = server
        .mock("PUT", "/test-publish-otp")
        .match_header("npm-otp", "135791")
        .expect(0)
        .create();
    let without_otp = server
        .mock("PUT", "/test-publish-otp")
        .match_header("npm-otp", Matcher::Missing)
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    assert_success(&publish(dir.path(), &[]));
    from_yaml.assert();
    without_otp.assert();

    let from_env = server
        .mock("PUT", "/test-publish-otp")
        .match_header("npm-otp", "246810")
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    let output = pacquet(dir.path())
        .with_arg("publish")
        .with_arg("--no-git-checks")
        .with_env("PNPM_CONFIG_OTP", "246810")
        .output()
        .expect("spawn pacquet publish");
    assert_success(&output);
    from_env.assert();
}

/// Commit everything in `dir` on `branch` so the publish git checks see a
/// clean tree on a known branch.
fn commit_all_on_branch(dir: &Path, branch: &str) {
    let git = |args: &[&str]| {
        let output = Command::new("git").args(args).current_dir(dir).output().expect("spawn git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr),
        );
    };
    git(&["init", &format!("--initial-branch={branch}")]);
    git(&["config", "user.email", "x@y.z"]);
    git(&["config", "user.name", "xyz"]);
    git(&["add", "."]);
    git(&["commit", "-m", "base", "--no-gpg-sign"]);
}

/// With git checks on, the branch gate accepts the branch named by the
/// `publishBranch` setting — the `--publish-branch` flag is not the only way
/// to move it off the built-in `master` / `main` pair.
#[test]
fn workspace_yaml_supplies_the_publish_branch() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-branch", "version": "1.0.0" }),
    );
    fs::write(dir.path().join("pnpm-workspace.yaml"), "publishBranch: release\n")
        .expect("write pnpm-workspace.yaml");
    commit_all_on_branch(dir.path(), "release");

    let mock = server
        .mock("PUT", "/test-publish-branch")
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    let output = pacquet(dir.path()).with_arg("publish").output().expect("spawn pacquet publish");
    assert_success(&output);
    mock.assert();
}

/// The control for [`workspace_yaml_supplies_the_publish_branch`]: without the
/// setting, `release` is not a publish branch, and the confirmation prompt a
/// non-interactive run cannot answer turns into a hard error.
#[test]
fn publishing_off_the_default_branches_without_the_setting_is_rejected() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-branch", "version": "1.0.0" }),
    );
    commit_all_on_branch(dir.path(), "release");

    let mock = server.mock("PUT", "/test-publish-branch").expect(0).create();

    let output = pacquet(dir.path()).with_arg("publish").output().expect("spawn pacquet publish");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "publish must fail; stderr: {stderr}");
    assert!(
        stderr.contains("ERR_PNPM_GIT_NOT_CORRECT_BRANCH"),
        "publish must fail the branch check; stderr: {stderr}",
    );
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

#[test]
fn json_flag_suppresses_explicit_reporter_output() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-json-reporter", "version": "1.0.0" }),
    );
    server.mock("PUT", "/test-publish-json-reporter").with_status(200).with_body("{}").create();

    let output = publish(dir.path(), &["--json", "--reporter=ndjson"]);
    assert_success(&output);
    assert!(
        output.stderr.is_empty(),
        "--json must suppress explicit reporter output; stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is one JSON value: {error}; stdout: {stdout}; stderr: {}",
            String::from_utf8_lossy(&output.stderr),
        )
    });
    assert_eq!(parsed["id"], "test-publish-json-reporter@1.0.0");
}

#[test]
fn json_flag_prints_errors_to_stdout() {
    let dir = tempfile::tempdir().expect("workspace");
    write_project(
        dir.path(),
        "https://registry.example/",
        &json!({ "name": "test-publish-no-version" }),
    );

    let output = publish(dir.path(), &["--dry-run", "--json"]);
    assert!(!output.status.success(), "publish without a version must fail");
    assert!(
        output.stderr.is_empty(),
        "--json errors must not be rendered to stderr; stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str::<Value>(&stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is a JSON error envelope: {error}; stdout: {stdout}; stderr: {}",
            String::from_utf8_lossy(&output.stderr),
        )
    });
    assert_eq!(
        stdout,
        "{\n  \"error\": {\n    \"code\": \"ERR_PNPM_PACKAGE_VERSION_NOT_FOUND\",\n    \"message\": \"Package version is not defined in the package.json.\"\n  }\n}\n",
    );
}

#[test]
fn json_flag_preserves_webauth_urls_on_noninteractive_otp_errors() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        dir.path(),
        &registry,
        &json!({ "name": "test-publish-otp-json", "version": "1.0.0" }),
    );
    let mock = server
        .mock("PUT", "/test-publish-otp-json")
        .with_status(401)
        .with_body(
            json!({
                "error": "one-time pass required",
                "authUrl": "https://auth.example/login?token=abc",
                "doneUrl": "https://auth.example/done?token=abc",
            })
            .to_string(),
        )
        .expect(1)
        .create();

    let output = publish(dir.path(), &["--json"]);
    assert!(!output.status.success(), "publish requiring OTP must fail without a TTY");
    assert!(
        output.stderr.is_empty(),
        "--json errors must not be rendered to stderr; stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is a JSON error envelope: {error}; stdout: {stdout}; stderr: {}",
            String::from_utf8_lossy(&output.stderr),
        )
    });
    assert_eq!(parsed["error"]["code"], "ERR_PNPM_OTP_NON_INTERACTIVE");
    assert_eq!(
        parsed["error"]["message"],
        "The registry requires additional authentication, but pnpm is not running in an interactive terminal",
    );
    assert_eq!(parsed["error"]["authUrl"], "https://auth.example/login?token=abc");
    assert_eq!(parsed["error"]["doneUrl"], "https://auth.example/done?token=abc");
    mock.assert();
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
