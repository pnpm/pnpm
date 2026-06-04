//! End-to-end `pacquet add` / `pacquet update` auto-cataloging tests,
//! ported from pnpm's
//! [`installing/deps-installer/test/catalogs.ts`](https://github.com/pnpm/pnpm/blob/e7e99f04e4/installing/deps-installer/test/catalogs.ts).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

const FOO: &str = "@pnpm.e2e/foo";

fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    (root, workspace, npmrc_info)
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn write_manifest(workspace: &Path, dependencies: &str) {
    let manifest = format!(
        r#"{{ "name": "test-catalog", "version": "1.0.0", "dependencies": {dependencies} }}"#,
    );
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

/// Append a catalog configuration to the harness's `pnpm-workspace.yaml`.
fn append_workspace_yaml(workspace: &Path, extra: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(extra);
    fs::write(&path, yaml).expect("write pnpm-workspace.yaml");
}

fn dep_spec(workspace: &Path, name: &str) -> Option<String> {
    let manifest = PackageManifest::from_path(workspace.join("package.json")).unwrap();
    manifest
        .dependencies([DependencyGroup::Prod])
        .find(|(key, _)| *key == name)
        .map(|(_, spec)| spec.to_string())
}

fn read(workspace: &Path, file: &str) -> String {
    fs::read_to_string(workspace.join(file)).unwrap_or_else(|_| panic!("read {file}"))
}

fn run_ok(workspace: &Path, args: &[&str]) {
    let output = pacquet(workspace, args).output().expect("run pacquet");
    assert!(
        output.status.success(),
        "command {args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// `add <pkg>@<version>` under `catalogMode: strict` with no existing
/// catalog entry writes `catalog:` to the manifest, the specifier to
/// `pnpm-workspace.yaml`, and the resolved snapshot to `pnpm-lock.yaml`.
#[test]
fn add_strict_catalogs_a_new_dependency() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, "{}");
    append_workspace_yaml(&workspace, "catalogMode: strict\n");

    run_ok(&workspace, &["add", "--lockfile-only", &format!("{FOO}@1.0.0")]);

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("catalog:"));

    let workspace_yaml = read(&workspace, "pnpm-workspace.yaml");
    assert!(
        workspace_yaml.contains("catalog:") && workspace_yaml.contains(&format!("'{FOO}': 1.0.0")),
        "pnpm-workspace.yaml missing the catalog entry:\n{workspace_yaml}",
    );

    let lockfile = read(&workspace, "pnpm-lock.yaml");
    assert!(lockfile.contains("catalogs:"), "lockfile missing catalogs:\n{lockfile}");
    assert!(
        lockfile.contains("specifier: 1.0.0") && lockfile.contains("version: 1.0.0"),
        "lockfile missing the resolved catalog entry:\n{lockfile}",
    );
    assert!(
        lockfile.contains(r#"specifier: "catalog:""#),
        "importer specifier not catalog:\n{lockfile}",
    );

    drop((root, anchor));
}

/// Same as above but under `catalogMode: prefer`.
#[test]
fn add_prefer_catalogs_a_new_dependency() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, "{}");
    append_workspace_yaml(&workspace, "catalogMode: prefer\n");

    run_ok(&workspace, &["add", "--lockfile-only", &format!("{FOO}@1.0.0")]);

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("catalog:"));
    assert!(read(&workspace, "pnpm-workspace.yaml").contains(&format!("'{FOO}': 1.0.0")));

    drop((root, anchor));
}

/// Re-adding a dependency already pinned to the catalog (no explicit
/// version) keeps the `catalog:` reference and leaves the catalog entry's
/// original specifier untouched. Regression test for pnpm#10176.
#[test]
fn readd_catalog_dependency_preserves_specifier() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "catalog:" }}"#));
    append_workspace_yaml(
        &workspace,
        &format!("catalogMode: strict\ncatalog:\n  '{FOO}': ^1.0.0\n"),
    );

    run_ok(&workspace, &["add", "--lockfile-only", FOO]);

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("catalog:"));
    let workspace_yaml = read(&workspace, "pnpm-workspace.yaml");
    assert!(
        workspace_yaml.contains(&format!("'{FOO}': ^1.0.0")),
        "catalog specifier should be preserved as ^1.0.0:\n{workspace_yaml}",
    );

    drop((root, anchor));
}

/// `add <pkg>@<version>` whose version disagrees with the existing catalog
/// entry is rejected under `catalogMode: strict`.
#[test]
fn add_mismatched_version_strict_errors() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "catalog:" }}"#));
    append_workspace_yaml(
        &workspace,
        &format!("catalogMode: strict\ncatalog:\n  '{FOO}': 1.0.0\n"),
    );

    let output = pacquet(&workspace, ["add", "--lockfile-only", &format!("{FOO}@2.0.0")])
        .output()
        .expect("run pacquet add");
    assert!(!output.status.success(), "a strict catalog mismatch must fail the add");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_CATALOG_VERSION_MISMATCH"),
        "stderr did not carry the mismatch error code:\n{stderr}",
    );

    drop((root, anchor));
}

/// `update --latest` on a dependency pinned to a *named* catalog keeps the
/// `catalog:<name>` reference in the manifest and bumps the catalog entry
/// (and the lockfile snapshot) to the freshly-resolved version.
#[test]
fn update_latest_named_catalog_bumps_the_entry() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "catalog:foo" }}"#));
    append_workspace_yaml(
        &workspace,
        &format!("catalogMode: prefer\ncatalogs:\n  foo:\n    '{FOO}': 1.0.0\n"),
    );

    run_ok(&workspace, &["install", "--lockfile-only"]);
    run_ok(&workspace, &["update", "--latest", "--lockfile-only", FOO]);

    // The manifest keeps the named-catalog reference.
    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("catalog:foo"));

    // The catalog entry no longer pins the original 1.0.0.
    let workspace_yaml = read(&workspace, "pnpm-workspace.yaml");
    assert!(
        !workspace_yaml.contains(&format!("'{FOO}': 1.0.0")),
        "the named catalog entry should have been bumped off 1.0.0:\n{workspace_yaml}",
    );

    // The lockfile keeps the named-catalog wiring and its snapshot is bumped
    // off the original 1.0.0 alongside the workspace manifest.
    let lockfile = read(&workspace, "pnpm-lock.yaml");
    assert!(
        lockfile.contains("catalogs:") && lockfile.contains("specifier: catalog:foo"),
        "lockfile should keep the named-catalog wiring:\n{lockfile}",
    );
    assert!(
        !lockfile.contains("specifier: 1.0.0"),
        "the lockfile catalog snapshot should have been bumped off 1.0.0:\n{lockfile}",
    );

    drop((root, anchor));
}

/// `update --latest --no-save` must not persist catalog edits to
/// `pnpm-workspace.yaml`, matching pnpm's `if (opts.save !== false)` guard.
#[test]
fn update_latest_no_save_leaves_the_catalog_untouched() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "catalog:" }}"#));
    append_workspace_yaml(
        &workspace,
        &format!("catalogMode: prefer\ncatalog:\n  '{FOO}': 1.0.0\n"),
    );

    run_ok(&workspace, &["install", "--lockfile-only"]);
    run_ok(&workspace, &["update", "--latest", "--no-save", "--lockfile-only", FOO]);

    let workspace_yaml = read(&workspace, "pnpm-workspace.yaml");
    assert!(
        workspace_yaml.contains(&format!("'{FOO}': 1.0.0")),
        "--no-save must not rewrite the catalog entry:\n{workspace_yaml}",
    );

    drop((root, anchor));
}
