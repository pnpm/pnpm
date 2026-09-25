use super::{
    _utils, DEP, add_workspace_package, assert_eq, fs, importer_specifier, importer_version,
    lockfile_package_keys, pacquet, read_lockfile, set_overrides, setup, write_manifest,
};
use assert_cmd::assert::OutputAssertExt;
use std::collections::HashMap;

/// `pnpm update --latest <name>` moves the override that pins the named
/// dependency, keeping the override's own range style: an exact pin lands on
/// the new exact version. The declaration in `package.json` stays, because
/// the override — not the declaration — is what the resolution answers to.
/// Covers <https://github.com/pnpm/pnpm/issues/8701>.
#[test]
fn update_latest_moves_the_override_that_pins_a_targeted_dependency() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    set_overrides(&workspace, &[(DEP, "100.0.0")]);
    pacquet(&workspace, ["install"]).assert().success();
    let before_manifest =
        fs::read_to_string(workspace.join("package.json")).expect("read package.json");
    assert_eq!(
        importer_version(&read_lockfile(&workspace.join("pnpm-lock.yaml")), ".", DEP),
        "100.0.0",
    );

    pacquet(&workspace, ["update", "--latest", DEP]).assert().success();

    assert_eq!(
        fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
        before_manifest,
        "the override governs the dependency, so the declaration is not the update's to move",
    );
    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");
    assert!(yaml.contains(&format!(r#""{DEP}": 101.0.0"#)), "override moved: {yaml}");
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_specifier(&lockfile, ".", DEP), "101.0.0");
    assert_eq!(importer_version(&lockfile, ".", DEP), "101.0.0");
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// The override's range style survives the move: a `~` override stays a `~`
/// override rather than adopting the project's save prefix.
#[test]
fn update_latest_moves_an_override_keeping_its_range_style() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    set_overrides(&workspace, &[(DEP, "~100.1.0")]);
    pacquet(&workspace, ["install"]).assert().success();
    assert_eq!(
        importer_version(&read_lockfile(&workspace.join("pnpm-lock.yaml")), ".", DEP),
        "100.1.0",
    );

    pacquet(&workspace, ["update", "--latest", DEP]).assert().success();

    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");
    assert!(yaml.contains(&format!(r#""{DEP}": ~101.0.0"#)), "override kept its style: {yaml}");
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_specifier(&lockfile, ".", DEP), "~101.0.0");
    assert_eq!(importer_version(&lockfile, ".", DEP), "101.0.0");
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// An override whose value names another package (`npm:` alias) cannot move
/// with the update, so the update reports it instead of failing silently.
#[test]
fn update_latest_warns_when_the_override_cannot_move() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, r#"{ "@pnpm.e2e/foo": "^100.0.0" }"#);
    set_overrides(&workspace, &[("@pnpm.e2e/foo", "npm:@pnpm.e2e/bar@100.0.0")]);
    pacquet(&workspace, ["install"]).assert().success();
    let before_manifest =
        fs::read_to_string(workspace.join("package.json")).expect("read package.json");
    let before_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");

    let output = pacquet(&workspace, ["update", "--latest", "@pnpm.e2e/foo"])
        .output()
        .expect("run update --latest");
    assert!(output.status.success(), "update failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("is controlled by an override"),
        "the pinned override must be reported to the user: {stdout}",
    );

    assert_eq!(
        fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
        before_manifest,
    );
    assert_eq!(
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml"),
        before_yaml,
    );
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// An update without selectors leaves an override-pinned dependency alone:
/// the pin is the user's standing instruction, and `--latest` run over every
/// dependency is not a request to rewrite it. Mirrors the v11 test
/// "update --latest preserves override-owned dependency resolutions".
#[test]
fn update_latest_without_selectors_preserves_an_override_pinned_dependency() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    set_overrides(&workspace, &[(DEP, "100.0.0")]);
    pacquet(&workspace, ["install"]).assert().success();
    let before_manifest =
        fs::read_to_string(workspace.join("package.json")).expect("read package.json");
    let before_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");
    let before_lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    assert_eq!(
        fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
        before_manifest,
        "the override governs the dependency, so the declaration must not move either",
    );
    assert_eq!(
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml"),
        before_yaml,
    );
    assert_eq!(
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile"),
        before_lockfile,
    );
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// A compatible update cannot move an exact override pin: within-range
/// re-resolution has no room when the override fixes the one version. The
/// update says so instead of quietly doing nothing.
#[test]
fn update_warns_when_a_compatible_update_cannot_move_an_exact_override_pin() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    set_overrides(&workspace, &[(DEP, "100.0.0")]);
    pacquet(&workspace, ["install"]).assert().success();
    let before_manifest =
        fs::read_to_string(workspace.join("package.json")).expect("read package.json");
    let before_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");

    let output = pacquet(&workspace, ["update", DEP]).output().expect("run update");
    assert!(output.status.success(), "update failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("is pinned to"),
        "the exact pin must be reported to the user: {stdout}",
    );

    assert_eq!(
        fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
        before_manifest,
    );
    assert_eq!(
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml"),
        before_yaml,
    );
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// In a workspace, the override lives in the root `pnpm-workspace.yaml` while
/// the dependency is declared by a subproject; the targeted update still
/// moves the root override.
#[test]
fn update_latest_moves_a_root_override_for_a_subproject_dependency() {
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
    set_overrides(&workspace, &[(DEP, "100.0.0")]);
    pacquet(&workspace, ["install"]).assert().success();
    let before_manifest =
        fs::read_to_string(project.join("package.json")).expect("read project package.json");

    pacquet(&workspace, ["update", "--latest", DEP, "--recursive"]).assert().success();

    assert_eq!(
        fs::read_to_string(project.join("package.json")).expect("read project package.json"),
        before_manifest,
    );
    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");
    assert!(yaml.contains(&format!(r#""{DEP}": 101.0.0"#)), "override moved: {yaml}");
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_specifier(&lockfile, "pkg", DEP), "101.0.0");
    assert_eq!(importer_version(&lockfile, "pkg", DEP), "101.0.0");
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

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
