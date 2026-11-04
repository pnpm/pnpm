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

    let _staged = archives_staged(&mut server);
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

#[test]
fn a_summary_given_to_version_goes_above_the_publication_note() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_workspace(&workspace, &registry);
    fs::write(workspace.join("summary.md"), "## Highlights\r\n").expect("write summary");

    run(pnpm(&workspace)
        .with_args(["version", "-r", "--no-git-checks", "--release-notes", "summary.md"]));
    let publication_note = format!("## Highlights\n\n{PUBLICATION_NOTE}");
    let parked = workspace.join(".changeset/changelogs");
    assert_eq!(fs::read_to_string(parked.join("a@1.1.0.md")).expect("a note"), VERSION_NOTE_A);

    // `pnpm publish` sends the composed note as it is and takes no summary
    // of its own for it.
    let refused = pnpm(&workspace)
        .with_args(["-r", "publish", "--batch", "--force", "--no-git-checks"])
        .with_args(["--release-notes", "summary.md"])
        .output()
        .expect("run pnpm publish");
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("ERR_PNPM_RELEASE_NOTES_FROM_VERSIONING"), "{stderr}");

    let _staged = archives_staged(&mut server);
    let batch = server
        .mock("PUT", "/-/pnpr/v0/publish")
        .match_body(Matcher::PartialJson(json!({
            "name": "v1.1.0",
            "notes": publication_note,
            "packages": [{ "name": "a", "notes": VERSION_NOTE_A }, { "name": "b" }],
        })))
        .with_status(201)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();
    run(pnpm(&workspace).with_args(["-r", "publish", "--batch", "--force", "--no-git-checks"]));
    batch.assert();

    // The intent check verifies the digest of the note with its summary.
    let link = r#"</-/publications/example/1/notes>; rel="publication-notes""#;
    let digest = note_digest(&publication_note);
    let _note_a = server
        .mock("GET", "/a/1.1.0/notes")
        .with_header("link", link)
        .with_header("pnpr-publication-initial-digest", &digest)
        .with_header("pnpr-note-initial-digest", &note_digest(VERSION_NOTE_A))
        .with_body(VERSION_NOTE_A)
        .create();
    let _note_b = server
        .mock("GET", "/b/1.1.0/notes")
        .with_status(204)
        .with_header("link", link)
        .with_header("pnpr-publication-initial-digest", &digest)
        .create();
    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));
    assert_eq!(intent_files(&workspace), Vec::<String>::new());
    drop(root);
}

#[test]
fn version_refuses_a_summary_with_nowhere_to_go() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let registry = "https://registry.example/";
    write_workspace(&workspace, registry);
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nversioning:\n  fixed:\n    - [a, b]\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("summary.md"), "Summary.\n").expect("write summary");

    for args in [
        &["version", "-r", "--no-git-checks", "--release-notes", "summary.md"][..],
        &["version", "patch", "--no-git-checks", "--release-notes", "summary.md"][..],
    ] {
        let output = pnpm(&workspace)
            .with_args(args)
            .output()
            .expect("run pnpm version");
        assert!(!output.status.success(), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("ERR_PNPM_RELEASE_NOTES_WITHOUT_"), "{args:?}: {stderr}");
    }
    assert_eq!(intent_files(&workspace), ["only-a.md", "shared.md"]);
    drop(root);
}

#[test]
fn a_batch_publishes_a_composed_release_whole() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_workspace(&workspace, &registry);
    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));
    let upload = server
        .mock("PUT", Matcher::Any)
        .expect(0)
        .create();

    let output = pnpm(&workspace)
        .with_args(["--filter", "a", "-r", "publish", "--batch", "--force", "--no-git-checks"])
        .output()
        .expect("run pnpm publish");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_RELEASE_NOTES_PARTIAL_PUBLICATION"), "{stderr}");
    upload.assert();
    drop(root);
}

#[test]
fn version_refuses_a_summary_for_private_releases_only() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, "https://registry.example/");
    for name in ["a", "b"] {
        fs::write(
            workspace
                .join("packages")
                .join(name)
                .join("package.json"),
            json!({ "name": name, "version": "1.0.0", "private": true }).to_string(),
        )
        .expect("write package.json");
    }
    fs::write(workspace.join("summary.md"), "Summary.\n").expect("write summary");

    let output = pnpm(&workspace)
        .with_args(["version", "-r", "--no-git-checks", "--release-notes", "summary.md"])
        .output()
        .expect("run pnpm version");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_RELEASE_NOTES_WITHOUT_REGISTRY"), "{stderr}");
    assert_eq!(intent_files(&workspace), ["only-a.md", "shared.md"]);
    drop(root);
}

#[test]
fn publish_fails_when_a_parked_note_cannot_be_read() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_workspace(&workspace, &registry);
    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));
    let parked = workspace.join(".changeset/changelogs/a@1.1.0.md");
    fs::remove_file(&parked).expect("remove the parked note");
    fs::create_dir(&parked).expect("put an unreadable entry in its place");
    let upload = server
        .mock("PUT", Matcher::Any)
        .expect(0)
        .create();

    let output = pnpm(&workspace)
        .with_args(["-r", "publish", "--batch", "--force", "--no-git-checks"])
        .output()
        .expect("run pnpm publish");

    assert!(!output.status.success(), "the composed note is not replaced by another source");
    upload.assert();
    drop(root);
}

#[test]
fn a_batch_refuses_a_hook_that_moves_a_composed_release_to_another_registry() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_workspace(&workspace, &registry);
    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = {
  hooks: {
    beforePacking (manifest) {
      if (manifest.name === 'b') manifest.publishConfig = { registry: 'https://elsewhere.example/' }
      return manifest
    },
  },
}
",
    )
    .expect("write .pnpmfile.cjs");
    let upload = server
        .mock("PUT", Matcher::Any)
        .expect(0)
        .create();

    let output = pnpm(&workspace)
        .with_args(["-r", "publish", "--batch", "--force", "--no-git-checks"])
        .output()
        .expect("run pnpm publish");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_RELEASE_NOTES_PACKAGE_CHANGED"), "{stderr}");
    upload.assert();
    drop(root);
}

#[test]
fn a_batch_refuses_a_hook_that_changes_a_composed_version() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_workspace(&workspace, &registry);
    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = {
  hooks: {
    beforePacking (manifest) {
      if (manifest.name === 'a') manifest.version = '9.9.9'
      return manifest
    },
  },
}
",
    )
    .expect("write .pnpmfile.cjs");
    let upload = server
        .mock("PUT", Matcher::Any)
        .expect(0)
        .create();

    let output = pnpm(&workspace)
        .with_args(["-r", "publish", "--batch", "--force", "--no-git-checks"])
        .output()
        .expect("run pnpm publish");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_RELEASE_NOTES_PACKAGE_CHANGED"), "{stderr}");
    upload.assert();
    drop(root);
}

#[test]
fn a_composed_publication_is_not_sent_to_another_registry_once_its_own_stores_no_notes() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_workspace(&workspace, &registry);
    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));
    let mut other = mockito::Server::new();
    let other_registry = format!("{}/", other.url());
    // The release's registry is no longer declared pnpr, and a new package
    // publishes to another registry that is.
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - packages/*\nversioning:\n  fixed:\n    - [a, b]\nregistries:\n  '{other_registry}':\n    serverType: pnpr\n",
        ),
    )
    .expect("write pnpm-workspace.yaml");
    let dir = workspace.join("packages/c");
    fs::create_dir_all(&dir).expect("create package dir");
    fs::write(
        dir.join("package.json"),
        json!({ "name": "c", "version": "1.0.0", "publishConfig": { "registry": other_registry } })
            .to_string(),
    )
    .expect("write package.json");
    let _own = server
        .mock("PUT", Matcher::Any)
        .with_status(201)
        .with_body(r#"{"ok":true}"#)
        .create();
    let _packuments = server
        .mock("GET", Matcher::Any)
        .with_status(404)
        .create();
    let _other_packuments = other
        .mock("GET", Matcher::Any)
        .with_status(404)
        .create();
    let _staged = archives_staged(&mut other);
    let elsewhere = other
        .mock("PUT", "/-/pnpr/v0/publish")
        .match_request(|request| {
            let body: serde_json::Value =
                serde_json::from_str(&request.utf8_lossy_body().expect("body")).expect("JSON");
            body.get("name").is_none() && body.get("notes").is_none()
        })
        .with_status(201)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();

    let output = pnpm(&workspace)
        .with_args(["-r", "publish", "--batch", "--force", "--no-git-checks"])
        .output()
        .expect("run pnpm publish");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    elsewhere.assert();
    drop(root);
}

#[test]
fn a_batch_refuses_a_composed_release_split_across_registries() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_workspace(&workspace, &registry);
    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));
    fs::write(
        workspace.join("packages/b/package.json"),
        json!({
            "name": "b",
            "version": "1.1.0",
            "publishConfig": { "registry": "https://elsewhere.example/" },
        })
        .to_string(),
    )
    .expect("write package.json");
    let upload = server
        .mock("PUT", Matcher::Any)
        .expect(0)
        .create();

    let output = pnpm(&workspace)
        .with_args(["-r", "publish", "--batch", "--force", "--no-git-checks"])
        .output()
        .expect("run pnpm publish");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_RELEASE_NOTES_SEVERAL_REGISTRIES"), "{stderr}");
    upload.assert();
    drop(root);
}

#[test]
fn a_hook_may_change_a_package_outside_the_composed_release() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_workspace(&workspace, &registry);
    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));
    let dir = workspace.join("packages/c");
    fs::create_dir_all(&dir).expect("create package dir");
    fs::write(dir.join("package.json"), json!({ "name": "c", "version": "1.0.0" }).to_string())
        .expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = {
  hooks: {
    beforePacking (manifest) {
      if (manifest.name === 'c') manifest.version = '1.0.1'
      return manifest
    },
  },
}
",
    )
    .expect("write .pnpmfile.cjs");
    let _packuments = server
        .mock("GET", Matcher::Any)
        .with_status(404)
        .create();
    let _staged = archives_staged(&mut server);
    let batch = server
        .mock("PUT", "/-/pnpr/v0/publish")
        .match_body(Matcher::PartialJson(json!({ "name": "v1.1.0", "notes": PUBLICATION_NOTE })))
        .with_status(201)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();

    run(pnpm(&workspace).with_args(["-r", "publish", "--batch", "--force", "--no-git-checks"]));

    batch.assert();
    drop(root);
}

#[test]
fn version_refuses_a_composed_release_bound_for_several_pnpr_registries() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let registry = "https://pnpr.example/";
    let other = "https://other-pnpr.example/";
    write_workspace(&workspace, registry);
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - packages/*\nversioning:\n  fixed:\n    - [a, b]\nregistries:\n  '{registry}':\n    serverType: pnpr\n  '{other}':\n    serverType: pnpr\n",
        ),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("packages/b/package.json"),
        json!({ "name": "b", "version": "1.0.0", "publishConfig": { "registry": other } })
            .to_string(),
    )
    .expect("write package.json");

    let output = pnpm(&workspace)
        .with_args(["version", "-r", "--no-git-checks"])
        .output()
        .expect("run pnpm version");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_RELEASE_NOTES_SEVERAL_REGISTRIES"), "{stderr}");
    let manifest =
        fs::read_to_string(workspace.join("packages/a/package.json")).expect("read package.json");
    assert!(manifest.contains(r#""version":"1.0.0""#), "{manifest}");
    assert!(!workspace.join(".changeset/changelogs").exists());
    assert_eq!(intent_files(&workspace), ["only-a.md", "shared.md"]);
    drop(root);
}

#[test]
fn a_same_named_project_on_another_registry_keeps_its_full_section() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut pnpr = mockito::Server::new();
    let registry = format!("{}/", pnpr.url());
    let other = "https://registry.example/";
    fs::write(workspace.join(".npmrc"), format!("registry={registry}\n")).expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n  - packages/*\nregistries:\n  '{registry}':\n    serverType: pnpr\n"),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), r#"{"name": "root", "private": true}"#)
        .expect("write root package.json");
    for (dir, manifest) in [
        ("a", json!({ "name": "a", "version": "1.0.0" })),
        (
            "a-next",
            json!({
                "name": "a",
                "version": "2.0.0",
                "publishConfig": { "registry": other },
            }),
        ),
    ] {
        let dir = workspace.join("packages").join(dir);
        fs::create_dir_all(&dir).expect("create package dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
    fs::create_dir_all(workspace.join(".changeset")).expect("create .changeset");
    fs::write(
        workspace.join(".changeset/next.md"),
        "---\n\"./packages/a-next\": minor\n---\n\nOnly the next line.\n",
    )
    .expect("write intent");

    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));

    let section = fs::read_to_string(workspace.join(".changeset/changelogs/a@2.1.0.md"))
        .expect("read parked section");
    assert!(section.starts_with("## 2.1.0\n"), "{section}");

    // Its release is confirmed from its own registry, not the notes route of
    // the same-named project's.
    let notes = pnpr
        .mock("GET", Matcher::Regex("/notes$".into()))
        .expect(0)
        .create();
    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));
    notes.assert();
    assert_eq!(intent_files(&workspace), ["next.md"]);
    drop(root);
}

#[test]
fn a_parked_release_is_confirmed_under_its_own_published_name() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut pnpr = mockito::Server::new();
    let registry = format!("{}/", pnpr.url());
    fs::write(workspace.join(".npmrc"), format!("registry={registry}\n")).expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n  - packages/*\nregistries:\n  '{registry}':\n    serverType: pnpr\n"),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), r#"{"name": "root", "private": true}"#)
        .expect("write root package.json");
    for (dir, manifest) in [
        (
            "a",
            json!({
                "name": "a",
                "version": "1.0.0",
                "publishConfig": { "name": "renamed-a", "registry": "https://registry.example/" },
            }),
        ),
        ("a-next", json!({ "name": "a", "version": "2.0.0" })),
    ] {
        let dir = workspace.join("packages").join(dir);
        fs::create_dir_all(&dir).expect("create package dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
    fs::create_dir_all(workspace.join(".changeset")).expect("create .changeset");
    fs::write(
        workspace.join(".changeset/next.md"),
        "---\n\"./packages/a-next\": minor\n---\n\nOnly the next line.\n",
    )
    .expect("write intent");
    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));
    assert_eq!(
        fs::read_to_string(workspace.join(".changeset/changelogs/a@2.1.0.md"))
            .expect("read parked note"),
        "### Minor Changes\n\n- Only the next line.\n",
    );

    let own = pnpr
        .mock("GET", "/a/2.1.0/notes")
        .with_status(404)
        .expect(1)
        .create();
    let renamed = pnpr
        .mock("GET", Matcher::Regex("renamed-a".into()))
        .expect(0)
        .create();
    run(pnpm(&workspace).with_args(["version", "-r", "--no-git-checks"]));

    own.assert();
    renamed.assert();
    assert_eq!(intent_files(&workspace), ["next.md"]);
    drop(root);
}

/// Answer every staging lookup on `server` as already staged, so a batch to a
/// registry declared `serverType: pnpr` commits without uploading.
fn archives_staged(server: &mut mockito::Server) -> mockito::Mock {
    server
        .mock("HEAD", "/-/pnpr/v0/artifacts/blob")
        .match_query(Matcher::Any)
        .with_status(200)
        .create()
}
