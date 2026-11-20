use super::{CommandExtra, CommandTempCwd, build_writes_marker, fs, json, write_workspace};
use assert_cmd::assert::OutputAssertExt;
use std::path::Path;

fn git(cwd: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("spawn git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// A `[<since>]` selector selects a project whose dependency an override
/// points at a changed catalog entry, though its manifest declares a plain
/// range.
#[test]
fn diff_selector_selects_projects_using_a_changed_catalog_through_overrides() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut app = build_writes_marker("app");
    app["dependencies"] = json!({ "lib": "^1.0.0" });
    write_workspace(&workspace, &[("app", app), ("other", build_writes_marker("other"))]);
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let packages = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    let write_catalog = |lib: &str| {
        let settings =
            format!("overrides:\n  lib: 'catalog:local'\ncatalogs:\n  local:\n    lib: {lib}\n");
        fs::write(&workspace_yaml, format!("{packages}{settings}"))
            .expect("write pnpm-workspace.yaml");
    };
    write_catalog("^1.0.0");
    git(&workspace, &["init", "--initial-branch=main"]);
    git(&workspace, &["config", "user.email", "x@y.z"]);
    git(&workspace, &["config", "user.name", "xyz"]);
    git(&workspace, &["add", "."]);
    git(&workspace, &["commit", "-m", "base", "--no-gpg-sign"]);
    write_catalog("^1.1.0");
    git(&workspace, &["commit", "-am", "bump the catalog", "--no-gpg-sign"]);

    pacquet
        .with_args(["-r", "--filter", "[HEAD~1]", "run", "build"])
        .assert()
        .success();

    assert!(
        workspace.join("app/ran.txt").exists(),
        "app resolves lib through the changed catalog entry",
    );
    assert!(!workspace.join("other/ran.txt").exists(), "other uses no changed catalog entry");

    drop(root);
}
