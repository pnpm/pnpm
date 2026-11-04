//! Publishing a package's `publishConfig.variants` to a `mockito` registry
//! declared `serverType: pnpr`: from one machine, and from several, each
//! uploading its archive with `--upload-variant` before a final publish
//! references them with `--variant-integrity`.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{fs, path::Path};

/// A package with an `any` archive and one Linux variant, published to
/// `registry`, declared `serverType: pnpr` when `pnpr`.
fn write_package(workspace: &Path, registry: &str, pnpr: bool) {
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "example-cli",
            "version": "4.0.0",
            "publishConfig": {
                "variants": [
                    { "targets": ["any"], "files": ["lib/**"] },
                    { "targets": ["linux-x64", "linux-x64-musl"], "files": ["native/example-cli"] },
                ],
            },
        })
        .to_string(),
    )
    .unwrap();
    fs::create_dir_all(workspace.join("lib")).unwrap();
    fs::write(workspace.join("lib/index.js"), "module.exports = 1\n").unwrap();
    fs::create_dir_all(workspace.join("native")).unwrap();
    fs::write(workspace.join("native/example-cli"), "binary").unwrap();
    fs::write(
        workspace.join(".npmrc"),
        format!(
            "registry={registry}\n//{}:_authToken=token\n",
            registry.trim_start_matches("http:"),
        ),
    )
    .unwrap();
    let declared = if pnpr {
        format!("registries:\n  '{registry}':\n    serverType: pnpr\n")
    } else {
        String::new()
    };
    fs::write(workspace.join("pnpm-workspace.yaml"), declared).unwrap();
}

fn clear_ci<Command: CommandExtra>(command: Command) -> Command {
    command
        .without_env("GITHUB_ACTIONS")
        .without_env("GITLAB_CI")
        .without_env("NPM_ID_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_URL")
}

fn body(request: &mockito::Request) -> Value {
    serde_json::from_str(&request.utf8_lossy_body().expect("body")).expect("JSON body")
}

#[test]
fn publishes_every_archive_by_its_staged_integrity_from_one_machine() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_package(&workspace, &registry, true);
    let mut staged = |method: &str, status: usize| {
        server
            .mock(method, "/-/pnpr/v0/artifacts/blob")
            .match_query(Matcher::Regex("^integrity=sha512-".to_string()))
            .with_status(status)
            .expect(2)
            .create()
    };
    let lookups = staged("HEAD", 404);
    let uploads = staged("PUT", 201);
    let publish = server
        .mock("PUT", "/example-cli")
        .match_request(|request| {
            let document = body(request);
            let attachments = document["_attachments"].as_object().unwrap();
            let names: Vec<&String> = attachments.keys().collect();
            names == ["example-cli-4.0.0.tgz", "example-cli-4.0.0~linux-x64+linux-x64-musl.tgz"]
                && attachments
                    .values()
                    .all(|attachment| {
                        attachment.get("data").is_none() && attachment["integrity"].is_string()
                    })
                && document.pointer("/versions/4.0.0/publishConfig/variants").is_none()
        })
        .with_status(201)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();

    clear_ci(pacquet)
        .with_args(["publish", "--no-git-checks", "--publish-wait-timeout=0"])
        .assert()
        .success();
    lookups.assert();
    uploads.assert();
    publish.assert();
    drop(root);
}

#[test]
fn refuses_variants_for_a_registry_not_declared_pnpr_unless_told_to_drop_them() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_package(&workspace, &registry, false);
    let output = clear_ci(pacquet)
        .with_args(["publish", "--no-git-checks", "--dry-run"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ERR_PNPM_VARIANTS_NEED_PNPR"));

    let publish = server
        .mock("PUT", "/example-cli")
        .match_request(|request| {
            let attachments = body(request)["_attachments"]
                .as_object()
                .unwrap()
                .clone();
            attachments.keys().collect::<Vec<_>>() == ["example-cli-4.0.0.tgz"]
                && attachments["example-cli-4.0.0.tgz"]["data"].is_string()
        })
        .with_status(201)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();
    let CommandTempCwd { pacquet, .. } = CommandTempCwd::init();
    clear_ci(pacquet)
        .with_current_dir(&workspace)
        .with_args(["publish", "--no-git-checks", "--no-variants", "--publish-wait-timeout=0"])
        .assert()
        .success();
    publish.assert();
    drop(root);
}

#[test]
fn uploads_a_variant_and_a_final_publish_references_it() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_package(&workspace, &registry, true);
    let upload = server
        .mock("PUT", "/-/pnpr/v0/artifacts/blob")
        .match_query(Matcher::Regex("^integrity=sha512-".to_string()))
        .with_status(201)
        .expect(1)
        .create();
    let output = clear_ci(pacquet)
        .with_args(["publish", "--no-git-checks", "--upload-variant", "linux-x64-musl"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    upload.assert();
    // Stdout holds only the value, which a CI job captures whole.
    let printed = String::from_utf8(output.stdout).unwrap();
    let line = printed.trim_end();
    assert_eq!(line.lines().count(), 1, "{printed}");
    let integrity = line
        .strip_prefix("linux-x64-musl=")
        .unwrap_or_else(|| panic!("{printed}"));
    assert!(integrity.starts_with("sha512-"), "{line}");

    let held = server
        .mock("HEAD", "/-/pnpr/v0/artifacts/blob")
        .match_query(Matcher::Regex("^integrity=sha512-".to_string()))
        .with_status(200)
        .with_header("content-length", "321")
        .expect(2)
        .create();
    let integrity = integrity.to_string();
    let expected = integrity.clone();
    let publish = server
        .mock("PUT", "/example-cli")
        .match_request(move |request| {
            let document = body(request);
            let variant =
                &document["_attachments"]["example-cli-4.0.0~linux-x64+linux-x64-musl.tgz"];
            variant["integrity"] == json!(expected)
                && variant["length"] == json!(321)
                && document["versions"]["4.0.0"]["dist"] == json!({ "integrity": "sha512-any" })
        })
        .with_status(201)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();
    let CommandTempCwd { pacquet, .. } = CommandTempCwd::init();
    clear_ci(pacquet)
        .with_current_dir(&workspace)
        .with_args([
            "publish",
            "--no-git-checks",
            "--publish-wait-timeout=0",
            "--variant-integrity",
            "any=sha512-any",
            "--variant-integrity",
            &format!("linux-x64={integrity}"),
        ])
        .assert()
        .success();
    held.assert();
    publish.assert();

    let missing = clear_ci(CommandTempCwd::init().pacquet)
        .with_current_dir(&workspace)
        .with_args(["publish", "--no-git-checks", "--variant-integrity", "any=sha512-any"])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(stderr.contains("gives no archive for linux-x64, linux-x64-musl"), "{stderr}");
    drop(root);
}

#[test]
fn no_variants_and_variant_integrity_are_refused_together() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_package(&workspace, "http://127.0.0.1:9/", true);
    let output = clear_ci(pacquet)
        .with_args([
            "publish",
            "--no-git-checks",
            "--no-variants",
            "--variant-integrity",
            "any=sha512-AAAA",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_INVALID_VARIANT_FLAGS"), "{stderr}");
    drop(root);
}
