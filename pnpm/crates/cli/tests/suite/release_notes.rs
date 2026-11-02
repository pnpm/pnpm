//! Release notes against a `mockito` registry declared `serverType: pnpr`:
//! the `notes` that `pnpm publish` sends, and the notes `pnpm view` reads.
//! The routes and body fields are those of the release-notes RFC.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn pnpm(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_env("GITHUB_ACTIONS")
        .without_env("GITLAB_CI")
        .without_env("NPM_ID_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_URL")
}

/// A project publishing to `registry`, declared `serverType: pnpr` when
/// `stores_notes`.
fn write_project(dir: &Path, registry: &str, stores_notes: bool) {
    fs::write(dir.join(".npmrc"), format!("registry={registry}\n")).expect("write .npmrc");
    if stores_notes {
        fs::write(
            dir.join("pnpm-workspace.yaml"),
            format!("registries:\n  '{registry}':\n    serverType: pnpr\n"),
        )
        .expect("write pnpm-workspace.yaml");
    }
    let manifest = json!({ "name": "notes-pkg", "version": "1.1.0" });
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    fs::write(
        dir.join("CHANGELOG.md"),
        "# notes-pkg\r\n\r\n## 1.1.0\r\n\r\n- Added `--fast`.\r\n\r\n## 1.0.0\r\n\r\n- First.\r\n",
    )
    .expect("write CHANGELOG.md");
}

fn publish(dir: &Path, args: &[&str]) -> std::process::Output {
    pnpm(dir)
        .with_args(["publish", "--no-git-checks"])
        .with_args(args.iter().copied())
        .output()
        .expect("spawn pnpm publish")
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// A `PUT /notes-pkg` whose document's `notes` field is `notes`.
fn expect_notes(server: &mut mockito::Server, notes: Option<&'static str>) -> mockito::Mock {
    server
        .mock("PUT", "/notes-pkg")
        .match_request(move |request| {
            let body: Value =
                serde_json::from_str(&request.utf8_lossy_body().expect("body")).expect("JSON body");
            body.get("notes").and_then(Value::as_str) == notes
        })
        .with_status(200)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create()
}

#[test]
fn publish_sends_the_changelog_section_to_a_pnpr_registry() {
    let dir = tempfile::tempdir().expect("project dir");
    let mut server = mockito::Server::new();
    write_project(dir.path(), &format!("{}/", server.url()), true);
    let mock = expect_notes(&mut server, Some("- Added `--fast`.\n"));

    let output = publish(dir.path(), &[]);

    assert_success(&output);
    mock.assert();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Release notes of notes-pkg@1.1.0: CHANGELOG.md"), "{stdout}");
}

#[test]
fn publish_sends_no_notes_to_a_registry_not_declared_pnpr() {
    let dir = tempfile::tempdir().expect("project dir");
    let mut server = mockito::Server::new();
    write_project(dir.path(), &format!("{}/", server.url()), false);
    let mock = expect_notes(&mut server, None);

    assert_success(&publish(dir.path(), &["--release-notes", "Ignored."]));
    mock.assert();
}

#[test]
fn release_notes_option_gives_the_version_note() {
    let dir = tempfile::tempdir().expect("project dir");
    let mut server = mockito::Server::new();
    write_project(dir.path(), &format!("{}/", server.url()), true);
    fs::write(dir.path().join("notes.md"), "From a file.\r\n").expect("write notes.md");
    let from_file = expect_notes(&mut server, Some("From a file.\n"));

    assert_success(&publish(dir.path(), &["--release-notes", "notes.md"]));
    from_file.assert();
}

#[test]
fn release_notes_option_takes_text_and_no_release_notes_sends_none() {
    let dir = tempfile::tempdir().expect("project dir");
    let mut server = mockito::Server::new();
    write_project(dir.path(), &format!("{}/", server.url()), true);
    let text = expect_notes(&mut server, Some("Written inline."));
    assert_success(&publish(dir.path(), &["--release-notes", "Written inline."]));
    text.assert();

    let none = expect_notes(&mut server, None);
    assert_success(&publish(dir.path(), &["--no-release-notes", "--force"]));
    none.assert();
}

#[test]
fn a_note_with_a_control_character_fails_before_uploading() {
    let dir = tempfile::tempdir().expect("project dir");
    let mut server = mockito::Server::new();
    write_project(dir.path(), &format!("{}/", server.url()), true);
    let upload = server
        .mock("PUT", "/notes-pkg")
        .expect(0)
        .create();

    let output = publish(dir.path(), &["--release-notes", "clear \u{1b}[2J the screen"]);

    assert!(!output.status.success(), "the publish must fail");
    upload.assert();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_RELEASE_NOTES_FORBIDDEN_CHARACTER"), "{stderr}");
}

#[test]
fn a_batch_sends_version_notes_and_the_publication() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    fs::write(workspace.join(".npmrc"), format!("registry={registry}\n")).expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n  - a\n  - b\nregistries:\n  '{registry}':\n    serverType: pnpr\n"),
    )
    .expect("write pnpm-workspace.yaml");
    for name in ["a", "b"] {
        let dir = workspace.join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(
            dir.join("package.json"),
            json!({ "name": name, "version": "2.0.0" }).to_string(),
        )
        .expect("write package.json");
    }
    fs::write(workspace.join("a/CHANGELOG.md"), "## 2.0.0\n\n- A changed.\n").expect("changelog");
    fs::write(workspace.join("summary.md"), "## Highlights\n").expect("summary");
    let batch = server
        .mock("PUT", "/-/pnpr/v0/publish")
        .match_body(Matcher::PartialJson(json!({
            "name": "v2.0.0",
            "notes": "## Highlights\n",
            "packages": [{ "name": "a", "notes": "- A changed.\n" }, { "name": "b" }],
        })))
        .with_status(201)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();

    let output = pnpm(&workspace)
        .with_args(["-r", "publish", "--batch", "--force", "--no-git-checks"])
        .with_args(["--release-notes", "summary.md", "--publication-name", "v2.0.0"])
        .output()
        .expect("spawn pnpm publish");

    assert_success(&output);
    batch.assert();
    drop(root);
}

fn packument() -> String {
    json!({
        "name": "notes-pkg",
        "dist-tags": { "latest": "1.1.0" },
        "versions": {
            "1.1.0": {
                "name": "notes-pkg",
                "version": "1.1.0",
                "dist": { "tarball": "https://example.com/notes-pkg-1.1.0.tgz" }
            }
        }
    })
    .to_string()
}

#[test]
fn view_prints_the_version_note_below_its_publication_note() {
    let dir = tempfile::tempdir().expect("project dir");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(dir.path(), &registry, true);
    let _packument = server
        .mock("GET", "/notes-pkg")
        .with_body(packument())
        .create();
    let _version_note = server
        .mock("GET", "/notes-pkg/1.1.0/notes")
        .with_header("content-type", "text/markdown; charset=utf-8")
        .with_header("link", r#"</-/publications/example/9f2c41/notes>; rel="publication-notes""#)
        .with_header("pnpr-publication-name", "v1.1.0")
        .with_body("- See [the guide](https://example.com/guide).\u{1b}[2J\u{202E}\n")
        .create();
    let publication = server
        .mock("GET", "/-/publications/example/9f2c41/notes")
        .with_body("## A new resolver\n")
        .expect(1)
        .create();

    let output = pnpm(dir.path())
        .with_args(["view", "notes-pkg@1.1.0", "releaseNotes"])
        .output()
        .expect("spawn pnpm view");

    assert_success(&output);
    publication.assert();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim_end(),
        "Release v1.1.0:\n  ## A new resolver\n\n1.1.0:\n  - See the guide (https://example.com/guide).[2J",
    );
}

#[test]
fn view_prints_nothing_when_the_version_has_no_notes() {
    let dir = tempfile::tempdir().expect("project dir");
    let mut server = mockito::Server::new();
    write_project(dir.path(), &format!("{}/", server.url()), true);
    let _packument = server
        .mock("GET", "/notes-pkg")
        .with_body(packument())
        .create();
    let _no_note = server
        .mock("GET", "/notes-pkg/1.1.0/notes")
        .with_status(404)
        .create();

    let output = pnpm(dir.path())
        .with_args(["view", "notes-pkg@1.1.0", "releaseNotes"])
        .output()
        .expect("spawn pnpm view");

    assert_success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");
}
