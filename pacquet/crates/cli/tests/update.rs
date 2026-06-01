use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const FOO: &str = "@pnpm.e2e/foo";

/// Spin up a temp workspace with the mocked registry and return the
/// pieces a multi-step update test needs.
fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    (root, workspace, npmrc_info)
}

/// Build a fresh `pacquet` command bound to `workspace`. The
/// `assert_cmd` `Command` is single-shot, so each install/update step
/// needs its own.
fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn write_manifest(workspace: &Path, dependencies: &str) {
    let manifest = format!(
        r#"{{ "name": "test-update", "version": "1.0.0", "dependencies": {dependencies} }}"#,
    );
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

fn dep_spec(workspace: &Path, name: &str) -> Option<String> {
    let manifest = PackageManifest::from_path(workspace.join("package.json")).unwrap();
    manifest
        .dependencies([DependencyGroup::Prod])
        .find(|(key, _)| *key == name)
        .map(|(_, spec)| spec.to_string())
}

fn virtual_store_has(workspace: &Path, name_at_version: &str) -> bool {
    workspace.join("node_modules").join(".pnpm").join(name_at_version).exists()
}

/// `pacquet update` re-resolves a dependency to the highest version
/// inside its range, even when the lockfile pins an older one — the
/// behaviour that distinguishes it from a plain `install` (which keeps
/// the pin because it still satisfies the range).
#[test]
fn update_bumps_within_range() {
    let (root, workspace, anchor) = setup();

    // Pin 100.0.0 exactly, then widen the range to `^100.0.0`. A plain
    // install would keep 100.0.0 (it satisfies `^100.0.0`); update must
    // bump to 100.1.0 (101.0.0 is outside the range).
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0"));

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["update"]).assert().success();

    assert!(
        virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"),
        "update should have bumped the dependency to the highest version in range",
    );
    // Compatible updates do not rewrite the manifest range.
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));

    drop((root, anchor));
}

/// `pacquet update --latest` ignores the manifest range, bumps to the
/// `latest` dist-tag, and rewrites `package.json`.
#[test]
fn update_latest_rewrites_manifest() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    // latest tag is the max published version, 101.0.0.
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@101.0.0"));
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^101.0.0"));

    drop((root, anchor));
}

/// `--save-exact` writes the bumped version without a range operator.
#[test]
fn update_latest_save_exact() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest", "--save-exact"]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("101.0.0"));

    drop((root, anchor));
}

/// A package selector only updates the matched dependency; others keep
/// their manifest ranges.
#[test]
fn update_latest_with_selector_is_scoped() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0", "{FOO}": "^1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest", FOO]).assert().success();

    // foo's latest is 100.1.0; dep-of-pkg-with-1-dep is untouched.
    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("^100.1.0"));
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));

    drop((root, anchor));
}

/// A negation selector (`!@scope/*`) updates everything *except* the
/// matched packages — ports pnpm's "update with negation pattern" test.
#[test]
fn update_latest_with_negation_selector() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0", "{FOO}": "^1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    // Update everything except dep-of-pkg-with-1-dep.
    pacquet(&workspace, ["update", "--latest", &format!("!{DEP}")]).assert().success();

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("^100.1.0"));
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));

    drop((root, anchor));
}

/// `--no-save` bumps the lockfile but leaves `package.json` untouched —
/// ports pnpm's "update --no-save should not update package.json" test.
#[test]
fn update_latest_no_save_keeps_manifest() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));

    pacquet(&workspace, ["update", "--latest", "--no-save"]).assert().success();

    // package.json range is unchanged...
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));
    // ...but the lockfile/store was re-resolved (101.0.0 is latest; the
    // in-memory `^101.0.0` drove resolution even though it wasn't saved).
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@101.0.0"));

    drop((root, anchor));
}

/// `update <pkg> --depth 0` where the package is not a direct dependency
/// fails with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES`.
#[test]
fn update_depth_zero_unknown_package_errors() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["update", "--depth", "0", "@pnpm.e2e/not-a-dependency"])
        .output()
        .expect("run pacquet update");
    assert!(!output.status.success(), "depth-0 update of a non-dependency should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("None of the specified packages were found in the dependencies"),
        "stderr did not mention NO_PACKAGE_IN_DEPENDENCIES: {stderr}",
    );

    drop((root, anchor));
}

/// `up` and `upgrade` are accepted as aliases of `update`.
#[test]
fn update_aliases_work() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["up", "--latest"]).assert().success();
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^101.0.0"));

    drop((root, anchor));
}

/// `--latest` combined with a versioned selector is rejected, matching
/// pnpm's `ERR_PNPM_LATEST_WITH_SPEC`.
#[test]
fn update_latest_with_spec_is_rejected() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["update", "--latest", &format!("{DEP}@2")])
        .output()
        .expect("run pacquet update");
    assert!(!output.status.success(), "update --latest with a spec should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Specs are not allowed to be used with --latest"),
        "stderr did not mention the LATEST_WITH_SPEC error: {stderr}",
    );

    drop((root, anchor));
}
