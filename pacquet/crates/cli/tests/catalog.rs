//! End-to-end `pacquet add` / `pacquet update` auto-cataloging tests,
//! ported from pnpm's
//! [`installing/deps-installer/test/catalogs.ts`](https://github.com/pnpm/pnpm/blob/e7e99f04e4/installing/deps-installer/test/catalogs.ts).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_lockfile::Lockfile;
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

fn catalog_snapshot(workspace: &Path, name: &str) -> (String, String) {
    let lockfile: Lockfile =
        serde_saphyr::from_str(&read(workspace, "pnpm-lock.yaml")).expect("parse pnpm-lock.yaml");
    let entry = lockfile
        .catalogs
        .as_ref()
        .and_then(|catalogs| catalogs.get("default"))
        .and_then(|catalog| catalog.get(name))
        .unwrap_or_else(|| panic!("missing default catalog snapshot entry for {name}"));
    (entry.specifier.clone(), entry.version.clone())
}

fn lockfile_override(workspace: &Path, selector: &str) -> Option<String> {
    let lockfile: Lockfile =
        serde_saphyr::from_str(&read(workspace, "pnpm-lock.yaml")).expect("parse pnpm-lock.yaml");
    lockfile.overrides.as_ref().and_then(|overrides| overrides.get(selector).cloned())
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
        lockfile.contains(r"specifier: 'catalog:'"),
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

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("catalog:foo"));

    let workspace_yaml = read(&workspace, "pnpm-workspace.yaml");
    assert!(
        !workspace_yaml.contains(&format!("'{FOO}': 1.0.0")),
        "the named catalog entry should have been bumped off 1.0.0:\n{workspace_yaml}",
    );

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

/// `update --latest` bumping a catalog that an override resolves through
/// must keep `pnpm-lock.yaml`'s `overrides` in sync with the bumped
/// catalog. A scoped selector is used so the override does not shadow the
/// direct `catalog:` dependency. If the override is not re-resolved against
/// the bumped catalog, lockfile `overrides` lags `catalogs` and the
/// follow-up `--frozen-lockfile` install fails with an overrides/catalogs
/// mismatch. Ported from pnpm's "overrides that reference a catalog are
/// updated in the lockfile when the catalog is updated".
#[test]
fn update_latest_keeps_catalog_referencing_override_in_sync() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "catalog:" }}"#));
    let override_selector = format!("@pnpm.e2e/foobar>{FOO}");
    append_workspace_yaml(
        &workspace,
        &format!(
            "catalogMode: prefer\ncatalog:\n  '{FOO}': '^1.0.0'\noverrides:\n  '{override_selector}': 'catalog:'\n",
        ),
    );

    run_ok(&workspace, &["install", "--lockfile-only"]);

    // The override resolves through the catalog, so it records the catalog's
    // specifier rather than a literal version.
    let (initial_spec, _) = catalog_snapshot(&workspace, FOO);
    assert_eq!(
        lockfile_override(&workspace, &override_selector).as_deref(),
        Some(initial_spec.as_str()),
        "override should track the catalog specifier before the update",
    );

    run_ok(&workspace, &["update", "--latest", "--lockfile-only", FOO]);

    let (bumped_spec, _) = catalog_snapshot(&workspace, FOO);
    assert_ne!(bumped_spec, initial_spec, "update --latest should bump the catalog entry");
    assert_eq!(
        lockfile_override(&workspace, &override_selector).as_deref(),
        Some(bumped_spec.as_str()),
        "lockfile override must be re-resolved against the bumped catalog",
    );

    // The bumped catalog is written back to pnpm-workspace.yaml, so a
    // follow-up frozen install reads it and must not fail with an
    // overrides/catalogs mismatch.
    run_ok(&workspace, &["install", "--frozen-lockfile"]);

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

#[test]
fn install_reruns_when_catalog_entry_changes() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "catalog:" }}"#));
    append_workspace_yaml(&workspace, &format!("catalog:\n  '{FOO}': 1.0.0\n"));

    run_ok(&workspace, &["install"]);
    assert_eq!(catalog_snapshot(&workspace, FOO), ("1.0.0".to_string(), "1.0.0".to_string()));

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        workspace_yaml.replace(&format!("'{FOO}': 1.0.0"), &format!("'{FOO}': 2.0.0")),
    )
    .expect("rewrite pnpm-workspace.yaml catalog entry");

    run_ok(&workspace, &["install"]);
    assert_eq!(catalog_snapshot(&workspace, FOO), ("2.0.0".to_string(), "2.0.0".to_string()));

    drop((root, anchor));
}
