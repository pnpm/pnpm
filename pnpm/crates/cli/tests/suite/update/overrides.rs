use super::{
    _utils, DEP, add_workspace_package, assert_eq, fs, importer_specifier, importer_version,
    lockfile_package_keys, pacquet, read_lockfile, set_overrides, setup,
};
use assert_cmd::assert::OutputAssertExt;
use std::collections::HashMap;

/// The requested version fits the raw manifest range but not its effective override.
#[test]
fn update_no_save_skips_version_excluded_by_selector_override() {
    let (root, workspace, anchor) = setup();
    add_workspace_package(&workspace, "pkg", "1.0.0");
    let project = workspace.join("pkg");
    fs::write(
        project.join("package.json"),
        format!(
            r#"{{ "name": "pkg", "version": "1.0.0", "dependencies": {{ "{DEP}": "^100.0.0" }} }}"#,
        ),
    )
    .expect("write project package.json");
    let override_key = format!("{DEP}@>=100.0.0 <100.1.0");
    set_overrides(&workspace, &[(override_key.as_str(), "100.0.0")]);
    pacquet(&workspace, ["install", "--lockfile-only"]).assert().success();

    let before_manifest =
        fs::read_to_string(project.join("package.json")).expect("read project package.json");
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_specifier(&lockfile, "pkg", DEP), "100.0.0");
    assert_eq!(importer_version(&lockfile, "pkg", DEP), "100.0.0");

    let requested = format!("{DEP}@100.1.0");
    let output = pacquet(
        &workspace,
        ["update", "--no-save", requested.as_str(), "--lockfile-only", "--recursive"],
    )
    .output()
    .expect("run recursive update --no-save");
    assert!(output.status.success(), "update --no-save failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(r#"Skipping "{requested}""#)),
        "the excluded version must be reported to the user: {stdout}",
    );

    assert_eq!(
        fs::read_to_string(project.join("package.json")).expect("read project package.json"),
        before_manifest,
    );
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_specifier(&lockfile, "pkg", DEP), "100.0.0");
    assert_eq!(importer_version(&lockfile, "pkg", DEP), "100.0.0");
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// A removal override leaves no effective dependency for a versioned update to change.
#[test]
fn update_no_save_does_not_restore_dependency_removed_by_selector_override() {
    let (root, workspace, anchor) = setup();
    add_workspace_package(&workspace, "pkg", "1.0.0");
    let project = workspace.join("pkg");
    fs::write(
        project.join("package.json"),
        format!(
            r#"{{ "name": "pkg", "version": "1.0.0", "dependencies": {{ "{DEP}": "^100.0.0" }} }}"#,
        ),
    )
    .expect("write project package.json");
    let override_key = format!("{DEP}@>=100.0.0 <100.1.0");
    set_overrides(&workspace, &[(override_key.as_str(), "-")]);
    pacquet(&workspace, ["install", "--lockfile-only"]).assert().success();

    let before_manifest =
        fs::read_to_string(project.join("package.json")).expect("read project package.json");
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert!(
        _utils::importer(&lockfile, "pkg").dependencies.as_ref().is_none_or(HashMap::is_empty),
        "the override must remove the dependency from the importer",
    );

    let requested = format!("{DEP}@100.1.0");
    pacquet(
        &workspace,
        ["update", "--no-save", requested.as_str(), "--lockfile-only", "--recursive"],
    )
    .assert()
    .success();

    assert_eq!(
        fs::read_to_string(project.join("package.json")).expect("read project package.json"),
        before_manifest,
    );
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert!(
        _utils::importer(&lockfile, "pkg").dependencies.as_ref().is_none_or(HashMap::is_empty),
        "update must not restore the removed dependency",
    );
    assert!(
        !lockfile_package_keys(&workspace).contains(&format!("{DEP}@100.1.0")),
        "update must not resolve the removed dependency",
    );
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}
