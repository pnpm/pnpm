//! Release notes from pnpm's versioning in `registry` changelog storage, for
//! a `mockito` registry declared `serverType: pnpr`: `pnpm version -r`
//! composes version and publication notes from the change intents,
//! `pnpm publish -r --batch` sends them, and a later `pnpm version -r`
//! deletes the intents once the registry holds the notes.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use pnpm_release_notes::note_digest;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{fs, path::Path, process::Command};

const PUBLICATION_NOTE: &str =
    "### Minor Changes\n\n- Shared by a and b.\n\n  Packages: `a@1.1.0`, `b@1.1.0`\n";
const VERSION_NOTE_A: &str = "### Minor Changes\n\n- Only a.\n";

fn pnpm(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_env("PACQUET_ASSUME_VERSIONS_PUBLISHED", "1")
        .without_env("GITHUB_ACTIONS")
        .without_env("GITLAB_CI")
        .without_env("NPM_ID_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_URL")
}

fn run(mut command: Command) -> String {
    let output = command.output().expect("run pnpm");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn write_workspace(workspace: &Path, registry: &str) {
    fs::write(workspace.join(".npmrc"), format!("registry={registry}\n")).expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - packages/*\nversioning:\n  fixed:\n    - [a, b]\nregistries:\n  '{registry}':\n    serverType: pnpr\n",
        ),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), r#"{"name": "root", "private": true}"#)
        .expect("write root package.json");
    for name in ["a", "b"] {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("create package dir");
        fs::write(
            dir.join("package.json"),
            json!({ "name": name, "version": "1.0.0" }).to_string(),
        )
        .expect("write package.json");
    }
    let changes = workspace.join(".changeset");
    fs::create_dir_all(&changes).expect("create .changeset");
    fs::write(changes.join("only-a.md"), "---\n\"a\": minor\n---\n\nOnly a.\r\n")
        .expect("write intent");
    fs::write(
        changes.join("shared.md"),
        "---\n\"a\": minor\n\"b\": minor\n---\n\nShared by a and b.\n",
    )
    .expect("write intent");
}

fn intent_files(workspace: &Path) -> Vec<String> {
    let mut files: Vec<String> = fs::read_dir(workspace.join(".changeset"))
        .expect("read .changeset")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".md"))
        .collect();
    files.sort();
    files
}

#[test]
fn versioning_composes_sends_and_collects_release_notes() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_workspace(&workspace, &registry);

    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));
    let parked = workspace.join(".changeset/changelogs");
    assert_eq!(fs::read_to_string(parked.join("a@1.1.0.md")).expect("a note"), VERSION_NOTE_A);
    assert_eq!(fs::read_to_string(parked.join("b@1.1.0.md")).expect("b note"), "");

    let batch = server
        .mock("PUT", "/-/pnpr/v0/publish")
        .match_body(Matcher::PartialJson(json!({
            "name": "v1.1.0",
            "notes": PUBLICATION_NOTE,
            "packages": [{ "name": "a", "notes": VERSION_NOTE_A }, { "name": "b" }],
        })))
        .with_status(201)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();
    run(pnpm(&workspace).with_args(["-r", "publish", "--batch", "--force", "--no-git-checks"]));
    batch.assert();

    // Before the registry holds the notes, the intents stay.
    let missing = server
        .mock("GET", Matcher::Regex("/notes$".into()))
        .with_status(404)
        .create();
    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));
    assert_eq!(intent_files(&workspace), ["only-a.md", "shared.md"]);
    missing.remove();

    let link = r#"</-/publications/example/1/notes>; rel="publication-notes""#;
    let digest = note_digest(PUBLICATION_NOTE);
    let _note_a = server
        .mock("GET", "/a/1.1.0/notes")
        .with_header("link", link)
        .with_header("pnpr-publication-initial-digest", &digest)
        .with_body("### Minor Changes\n\n- Only a, edited later.\n")
        .with_header("pnpr-note-initial-digest", &note_digest(VERSION_NOTE_A))
        .create();
    let _note_b = server
        .mock("GET", "/b/1.1.0/notes")
        .with_status(204)
        .with_header("link", link)
        .with_header("pnpr-publication-initial-digest", &digest)
        .create();
    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));

    assert_eq!(intent_files(&workspace), Vec::<String>::new());
    assert!(!parked.join("a@1.1.0.md").exists(), "the version note was collected");
    assert!(
        !parked
            .join("publications")
            .read_dir()
            .is_ok_and(|mut dir| dir.next().is_some()),
    );
    drop(root);
}
