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

#[derive(Debug, PartialEq, Eq)]
struct Ran {
    app: bool,
    other: bool,
}

/// Commit a workspace of `app`, which declares `lib: <app_lib>`, and
/// `other`, change the `local` catalog's `lib` from `lib_before` to
/// `lib_after`, run `-r --filter [HEAD~1] run build`, and return whether
/// each of `app` and `other` ran.
fn run_after_catalog_change(
    app_lib: &str,
    overrides: &str,
    (lib_before, lib_after): (&str, &str),
) -> Ran {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut app = build_writes_marker("app");
    app["dependencies"] = json!({ "lib": app_lib });
    write_workspace(&workspace, &[("app", app), ("other", build_writes_marker("other"))]);
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let packages = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    let write_catalog = |lib: &str| {
        let settings = format!("overrides:\n{overrides}catalogs:\n  local:\n    lib: {lib}\n");
        fs::write(&workspace_yaml, format!("{packages}{settings}"))
            .expect("write pnpm-workspace.yaml");
    };
    write_catalog(lib_before);
    git(&workspace, &["init", "--initial-branch=main"]);
    git(&workspace, &["config", "user.email", "x@y.z"]);
    git(&workspace, &["config", "user.name", "xyz"]);
    git(&workspace, &["add", "."]);
    git(&workspace, &["commit", "-m", "base", "--no-gpg-sign"]);
    write_catalog(lib_after);
    git(&workspace, &["commit", "-am", "bump the catalog", "--no-gpg-sign"]);

    pacquet
        .with_args(["-r", "--filter", "[HEAD~1]", "run", "build"])
        .assert()
        .success();

    let ran = Ran {
        app: workspace.join("app/ran.txt").exists(),
        other: workspace.join("other/ran.txt").exists(),
    };
    drop(root);
    ran
}

#[test]
fn diff_selector_selects_projects_using_a_changed_catalog_through_overrides() {
    let ran = run_after_catalog_change("^1.0.0", "  lib: 'catalog:local'\n", ("^1.0.0", "^1.1.0"));
    assert_eq!(ran, Ran { app: true, other: false });
}

#[test]
fn diff_selector_selects_a_catalog_dependency_a_range_scoped_override_starts_to_match() {
    let ran = run_after_catalog_change("catalog:local", "  lib@^2: 3.0.0\n", ("1.0.0", "2.0.0"));
    assert_eq!(ran, Ran { app: true, other: false });
}
