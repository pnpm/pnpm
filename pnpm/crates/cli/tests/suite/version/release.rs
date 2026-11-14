use super::pacquet_version_assuming_published;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{fs, path::Path};

fn write_release_workspace(workspace: &Path, workspace_yaml: &str) {
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml)
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), "{\"name\": \"e2e-root\", \"private\": true}\n")
        .expect("write root package.json");
}

fn write_pkg(workspace: &Path, dir: &str, manifest: &Value) {
    let pkg_dir = workspace.join("packages").join(dir);
    fs::create_dir_all(&pkg_dir).expect("create package dir");
    fs::write(pkg_dir.join("package.json"), format!("{manifest}\n")).expect("write package.json");
}

fn write_intent(workspace: &Path, id: &str, content: &str) {
    let changeset_dir = workspace.join(".changeset");
    fs::create_dir_all(&changeset_dir).expect("create .changeset");
    fs::write(changeset_dir.join(format!("{id}.md")), content).expect("write intent");
}

#[test]
fn version_recursive_dry_run_json_emits_planned_releases_as_json_without_writing_or_running_scripts()
 {
    let CommandTempCwd { workspace, root: _root, .. } = CommandTempCwd::init();
    write_release_workspace(
        &workspace,
        "packages:\n  - packages/*\nversioning:\n  fixed:\n    - [pkg-a, pkg-b]\n",
    );
    write_pkg(
        &workspace,
        "pkg-a",
        &json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "scripts": {
                "version": r#"node -e "require('node:fs').writeFileSync('ran.txt', '1')""#,
            },
        }),
    );
    write_pkg(&workspace, "pkg-b", &json!({ "name": "pkg-b", "version": "1.0.0" }));
    write_intent(&workspace, "demo", "---\n\"pkg-a\": minor\n---\n\nDemo change.\n");

    let output =
        pacquet_version_assuming_published(&workspace, &["version", "-r", "--dry-run", "--json"]);
    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("expected valid JSON output, got {error}: {stdout}"));
    assert_eq!(
        parsed,
        json!([
            { "name": "pkg-a", "currentVersion": "1.0.0", "newVersion": "1.1.0" },
            { "name": "pkg-b", "currentVersion": "1.0.0", "newVersion": "1.1.0" },
        ]),
    );
    let pkg_a_manifest: Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("packages/pkg-a/package.json")).expect("read pkg-a"),
    )
    .expect("parse pkg-a");
    assert_eq!(pkg_a_manifest["version"], "1.0.0");
    assert!(workspace.join(".changeset/demo.md").exists());
    assert!(!workspace.join("packages/pkg-a/ran.txt").exists());
}

#[test]
fn version_recursive_runs_preversion_version_and_postversion_lifecycle_scripts_in_order() {
    let CommandTempCwd { workspace, root: _root, .. } = CommandTempCwd::init();
    write_release_workspace(&workspace, "packages:\n  - packages/*\n");
    for (name, version) in [("pkg-a", "1.0.0"), ("pkg-b", "2.0.0")] {
        write_pkg(
            &workspace,
            name,
            &json!({
                "name": name,
                "version": version,
                "scripts": {
                    "preversion": r#"node -e "const p = require('./package.json'); require('node:fs').appendFileSync('../../order.txt', p.name + ':preversion:' + p.version + '\n')""#,
                    "version": r#"node -e "const p = require('./package.json'); require('node:fs').appendFileSync('../../order.txt', p.name + ':version:' + p.version + '\n')""#,
                    "postversion": r#"node -e "const p = require('./package.json'); require('node:fs').appendFileSync('../../order.txt', p.name + ':postversion:' + p.version + '\n')""#,
                },
            }),
        );
    }
    write_intent(
        &workspace,
        "bump-both",
        "---\n\"pkg-a\": minor\n\"pkg-b\": patch\n---\n\nFeature.\n",
    );

    let output =
        pacquet_version_assuming_published(&workspace, &["version", "-r", "--no-git-checks"]);
    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let order = fs::read_to_string(workspace.join("order.txt"))
        .expect("lifecycle scripts must write order.txt");
    assert_eq!(
        order,
        concat!(
            "pkg-a:preversion:1.0.0\n",
            "pkg-b:preversion:2.0.0\n",
            "pkg-a:version:1.1.0\n",
            "pkg-b:version:2.0.1\n",
            "pkg-a:postversion:1.1.0\n",
            "pkg-b:postversion:2.0.1\n",
        ),
    );
}

#[test]
fn version_recursive_json_keeps_stdout_parseable_when_lifecycle_scripts_print_to_stdout() {
    let CommandTempCwd { workspace, root: _root, .. } = CommandTempCwd::init();
    write_release_workspace(&workspace, "packages:\n  - packages/*\n");
    write_pkg(
        &workspace,
        "pkg-a",
        &json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "scripts": {
                "preversion": r#"node -e "console.log('preversion-stdout')""#,
                "version": r#"node -e "console.log('version-stdout')""#,
                "postversion": r#"node -e "console.log('postversion-stdout')""#,
            },
        }),
    );
    write_intent(&workspace, "bump-a", "---\n\"pkg-a\": minor\n---\n\nFeature.\n");

    let output = pacquet_version_assuming_published(
        &workspace,
        &["version", "-r", "--no-git-checks", "--json"],
    );
    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("expected pure JSON stdout, got {error}: {stdout}"));
    assert_eq!(
        parsed,
        json!([{ "name": "pkg-a", "currentVersion": "1.0.0", "newVersion": "1.1.0" }]),
    );
}

#[test]
fn version_recursive_aborts_before_writing_when_preversion_fails() {
    let CommandTempCwd { workspace, root: _root, .. } = CommandTempCwd::init();
    write_release_workspace(&workspace, "packages:\n  - packages/*\n");
    write_pkg(
        &workspace,
        "pkg-a",
        &json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "scripts": {
                "preversion": r#"node -e "process.exit(1)""#,
            },
        }),
    );
    write_intent(&workspace, "bump-a", "---\n\"pkg-a\": minor\n---\n\nFeature.\n");

    let output =
        pacquet_version_assuming_published(&workspace, &["version", "-r", "--no-git-checks"]);
    assert!(!output.status.success(), "failing preversion must abort version -r");
    let pkg_a_manifest: Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("packages/pkg-a/package.json")).expect("read pkg-a"),
    )
    .expect("parse pkg-a");
    assert_eq!(pkg_a_manifest["version"], "1.0.0");
    assert!(!workspace.join(".changeset/ledger.yaml").exists());
}

#[test]
fn version_recursive_skips_lifecycle_scripts_when_ignore_scripts_is_passed() {
    let CommandTempCwd { workspace, root: _root, .. } = CommandTempCwd::init();
    write_release_workspace(&workspace, "packages:\n  - packages/*\n");
    write_pkg(
        &workspace,
        "pkg-a",
        &json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "scripts": {
                "preversion": r#"node -e "require('node:fs').writeFileSync('ran.txt', '1')""#,
                "version": r#"node -e "require('node:fs').writeFileSync('ran.txt', '1')""#,
                "postversion": r#"node -e "require('node:fs').writeFileSync('ran.txt', '1')""#,
            },
        }),
    );
    write_intent(&workspace, "bump-a", "---\n\"pkg-a\": minor\n---\n\nFeature.\n");

    let output = pacquet_version_assuming_published(
        &workspace,
        &["version", "-r", "--no-git-checks", "--ignore-scripts"],
    );
    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let pkg_a_manifest: Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("packages/pkg-a/package.json")).expect("read pkg-a"),
    )
    .expect("parse pkg-a");
    assert_eq!(pkg_a_manifest["version"], "1.1.0");
    assert!(!workspace.join("packages/pkg-a/ran.txt").exists());
}
