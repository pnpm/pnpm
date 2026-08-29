//! End-to-end `pacquet add` / `pacquet update` auto-cataloging tests.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{Lockfile, PkgName};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
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
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
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

#[test]
fn workspace_catalog_switches_between_local_project_and_registry_package() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, "{}");
    append_workspace_yaml(
        &workspace,
        &format!("packages:\n  - 'packages/*'\ncatalog:\n  '{FOO}': workspace:*\n"),
    );
    let app = workspace.join("packages/app");
    let local_dependency = workspace.join("packages/local-foo");
    fs::create_dir_all(&app).expect("create the app project");
    fs::create_dir_all(&local_dependency).expect("create the local dependency project");
    fs::write(
        app.join("package.json"),
        serde_json::json!({
            "name": "catalog-bridge-consumer",
            "version": "1.0.0",
            "dependencies": { FOO: "catalog:" },
        })
        .to_string(),
    )
    .expect("write the app manifest");
    fs::write(
        local_dependency.join("package.json"),
        serde_json::json!({ "name": FOO, "version": "9.0.0" }).to_string(),
    )
    .expect("write the local dependency manifest");

    run_ok(&workspace, &["install"]);

    let installed_manifest = app.join("node_modules/@pnpm.e2e/foo/package.json");
    let installed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&installed_manifest).expect("read local package"))
            .expect("parse local package");
    assert_eq!(installed["version"], "9.0.0");
    assert_eq!(importer_dep_version(&workspace, "packages/app", FOO), "link:../local-foo");

    fs::remove_dir_all(&local_dependency).expect("remove the local dependency project");
    let workspace_yaml = read(&workspace, "pnpm-workspace.yaml")
        .replace(&format!("'{FOO}': workspace:*"), &format!("'{FOO}': 1.0.0"));
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml)
        .expect("switch the catalog to the registry package");

    run_ok(&workspace, &["install"]);

    let installed: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&installed_manifest).expect("read registry package"),
    )
    .expect("parse registry package");
    assert_eq!(installed["version"], "1.0.0");
    assert_eq!(importer_dep_version(&workspace, "packages/app", FOO), "1.0.0");
    assert_eq!(dep_spec(&app, FOO).as_deref(), Some("catalog:"));

    drop((root, anchor));
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

#[test]
fn save_catalog_flag_writes_the_default_catalog() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, "{}");

    run_ok(&workspace, &["add", "--lockfile-only", "--save-catalog", &format!("{FOO}@1.0.0")]);

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("catalog:"));
    assert!(read(&workspace, "pnpm-workspace.yaml").contains(&format!("'{FOO}': 1.0.0")));
    assert_eq!(catalog_snapshot(&workspace, FOO), ("1.0.0".to_string(), "1.0.0".to_string()));

    drop((root, anchor));
}

#[test]
fn save_catalog_name_preserves_the_dependency_group() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, "{}");

    run_ok(
        &workspace,
        &[
            "add",
            "--lockfile-only",
            "--save-dev",
            "--save-catalog-name",
            "tools",
            &format!("{FOO}@1.0.0"),
        ],
    );

    let manifest = PackageManifest::from_path(workspace.join("package.json")).unwrap();
    assert_eq!(
        manifest.dependencies([DependencyGroup::Dev]).collect::<Vec<_>>(),
        vec![(FOO, "catalog:tools")],
    );
    let workspace_yaml = read(&workspace, "pnpm-workspace.yaml");
    assert!(workspace_yaml.contains("tools:"));
    assert!(workspace_yaml.contains(&format!("'{FOO}': 1.0.0")));

    drop((root, anchor));
}

/// A `saveCatalogName` carrying a newline would render as a YAML block
/// scalar and splice a corrupt `catalogs:` header into
/// `pnpm-workspace.yaml`. The whole chain — workspace yaml → `Config` →
/// catalog decision → manifest writer — must refuse it and leave the
/// file as it was.
#[test]
fn a_catalog_name_with_a_control_character_is_refused() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, "{}");
    append_workspace_yaml(&workspace, "saveCatalogName: \"tools\\n  injected: oops\"\n");
    let before = read(&workspace, "pnpm-workspace.yaml");

    let output = pacquet(&workspace, ["add", "--lockfile-only", &format!("{FOO}@1.0.0")])
        .output()
        .expect("run pacquet");

    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}");
    assert!(!output.status.success(), "the control character must be refused");
    assert!(
        stderr.contains("ERR_PNPM_WORKSPACE_MANIFEST_WRITER_INVALID_CONTROL_CHARACTER"),
        "expected the control-character diagnostic; got:\n{stderr}",
    );
    assert_eq!(read(&workspace, "pnpm-workspace.yaml"), before, "the manifest must be untouched");

    drop((root, anchor));
}

#[test]
fn install_with_catalog_reference_writes_catalog_snapshot() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "catalog:" }}"#));
    append_workspace_yaml(&workspace, &format!("catalog:\n  '{FOO}': 1.0.0\n"));

    run_ok(&workspace, &["install", "--lockfile-only"]);

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("catalog:"));
    assert_eq!(catalog_snapshot(&workspace, FOO), ("1.0.0".to_string(), "1.0.0".to_string()));

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

#[test]
fn update_latest_keeps_catalog_reference_in_manual_mode() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "catalog:" }}"#));
    append_workspace_yaml(&workspace, &format!("catalog:\n  '{FOO}': 1.0.0\n"));

    run_ok(&workspace, &["install", "--lockfile-only"]);
    run_ok(&workspace, &["update", "--latest", "--lockfile-only", FOO]);

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("catalog:"));
    assert_ne!(catalog_snapshot(&workspace, FOO).0, "1.0.0");

    drop((root, anchor));
}

/// `update --latest` bumping a catalog that an override resolves through
/// must keep `pnpm-lock.yaml`'s `overrides` in sync with the bumped
/// catalog. A scoped selector is used so the override does not shadow the
/// direct `catalog:` dependency. If the override is not re-resolved against
/// the bumped catalog, lockfile `overrides` lags `catalogs` and the
/// follow-up `--frozen-lockfile` install fails with an overrides/catalogs
/// mismatch.
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
/// `pnpm-workspace.yaml`: the save step is skipped unless `--save` is in
/// effect.
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

    run_ok(&workspace, &["install", "--no-frozen-lockfile"]);
    assert_eq!(catalog_snapshot(&workspace, FOO), ("2.0.0".to_string(), "2.0.0".to_string()));

    drop((root, anchor));
}

/// With `catalogPrune: true`, a manifest-persisting command (`pnpm add`
/// here) drops the catalog entries no importer references while keeping
/// the referenced ones.
#[test]
fn removes_unused_entries_from_the_workspace_catalog() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "catalog:" }}"#));
    append_workspace_yaml(
        &workspace,
        &format!("catalogPrune: true\ncatalog:\n  '{FOO}': 1.0.0\n  '@pnpm.e2e/bar': 100.0.0\n"),
    );

    run_ok(&workspace, &["add", "@pnpm.e2e/peer-a@1.0.0"]);

    let workspace_yaml = read(&workspace, "pnpm-workspace.yaml");
    assert!(
        workspace_yaml.contains(&format!("'{FOO}': 1.0.0")),
        "the referenced catalog entry must survive:\n{workspace_yaml}",
    );
    assert!(
        !workspace_yaml.contains("@pnpm.e2e/bar"),
        "the unreferenced catalog entry must be removed:\n{workspace_yaml}",
    );

    drop((root, anchor));
}

/// With `minimumReleaseAgeExcludePrune: true`, a
/// manifest-persisting command (`pnpm add` here) prunes the
/// `minimumReleaseAgeExclude` entries the freshly resolved lockfile no
/// longer records: a version union is narrowed to the resolved version,
/// an entry whose package is absent is dropped, and a glob is kept.
#[test]
fn prunes_the_minimum_release_age_excludes() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, "{}");
    append_workspace_yaml(
        &workspace,
        &format!(
            "minimumReleaseAgeExcludePrune: true\n\
             minimumReleaseAgeExclude:\n  \
             - '{FOO}@1.0.0 || 2.0.0'\n  \
             - '@pnpm.e2e/bar@100.0.0'\n  \
             - '@pnpm.e2e/*'\n",
        ),
    );

    run_ok(&workspace, &["add", &format!("{FOO}@2.0.0")]);

    let workspace_yaml = read(&workspace, "pnpm-workspace.yaml");
    assert!(
        workspace_yaml.contains(&format!("{FOO}@2.0.0")),
        "the narrowed exclude must keep the resolved version:\n{workspace_yaml}",
    );
    assert!(
        !workspace_yaml.contains("1.0.0"),
        "the version no longer resolved must be pruned:\n{workspace_yaml}",
    );
    assert!(
        !workspace_yaml.contains("@pnpm.e2e/bar"),
        "the exclude for an absent package must be dropped:\n{workspace_yaml}",
    );
    assert!(
        workspace_yaml.contains("@pnpm.e2e/*"),
        "a glob exclude must survive:\n{workspace_yaml}",
    );

    drop((root, anchor));
}

/// Regression test for [pnpm#13715](https://github.com/pnpm/pnpm/issues/13715).
#[test]
fn add_moves_a_catalog_locked_on_another_version() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "catalog:" }}"#));
    append_workspace_yaml(
        &workspace,
        &format!("catalogMode: strict\ncatalog:\n  '{FOO}': 1.0.0\n"),
    );
    run_ok(&workspace, &["install", "--lockfile-only"]);

    // Widen the entry to a range that keeps 1.0.0 resolved, so the wanted
    // version below is inside the range but is not what the entry resolves to.
    let widened = read(&workspace, "pnpm-workspace.yaml").replace("': 1.0.0", "': ^1.0.0");
    std::fs::write(workspace.join("pnpm-workspace.yaml"), widened).expect("widen the catalog");
    run_ok(&workspace, &["install", "--lockfile-only"]);
    assert_eq!(catalog_snapshot(&workspace, FOO), ("^1.0.0".to_string(), "1.0.0".to_string()));

    run_ok(&workspace, &["add", "--lockfile-only", &format!("{FOO}@1.1.0")]);

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("catalog:"));
    assert_eq!(catalog_snapshot(&workspace, FOO), ("^1.0.0".to_string(), "1.1.0".to_string()));

    drop((root, anchor));
}

#[test]
fn update_moves_a_catalog_to_an_older_in_range_version() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "catalog:" }}"#));
    append_workspace_yaml(
        &workspace,
        &format!("catalogMode: strict\ncatalog:\n  '{FOO}': ^1.0.0\n"),
    );
    run_ok(&workspace, &["install", "--lockfile-only"]);
    let (_, resolved) = catalog_snapshot(&workspace, FOO);
    assert_ne!(resolved, "1.0.0", "the entry has to start on a newer version than the request");

    run_ok(&workspace, &["update", "--lockfile-only", &format!("{FOO}@1.0.0")]);

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("catalog:"));
    assert_eq!(catalog_snapshot(&workspace, FOO).1, "1.0.0");

    drop((root, anchor));
}

/// A filtered add moves the entry for the project it targets. A project that
/// declares the same package directly, and wasn't targeted, keeps its own
/// resolution.
#[test]
fn add_moving_a_catalog_leaves_an_untargeted_project_alone() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, "{}");
    append_workspace_yaml(
        &workspace,
        &format!("packages:\n  - 'packages/*'\ncatalogMode: strict\ncatalog:\n  '{FOO}': 1.0.0\n"),
    );
    for (name, spec) in [("a", "catalog:"), ("b", "^1.0.0")] {
        let dir = workspace.join("packages").join(name);
        std::fs::create_dir_all(&dir).expect("create the package dir");
        std::fs::write(
            dir.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": { FOO: spec },
            })
            .to_string(),
        )
        .expect("write the package manifest");
    }
    run_ok(&workspace, &["install", "--lockfile-only"]);
    let widened = read(&workspace, "pnpm-workspace.yaml").replace("': 1.0.0", "': ^1.0.0");
    std::fs::write(workspace.join("pnpm-workspace.yaml"), widened).expect("widen the catalog");
    run_ok(&workspace, &["install", "--lockfile-only"]);
    let untargeted_before = importer_dep_version(&workspace, "packages/b", FOO);

    run_ok(&workspace, &["--dir", "packages/a", "add", "--lockfile-only", &format!("{FOO}@1.1.0")]);

    assert_eq!(catalog_snapshot(&workspace, FOO), ("^1.0.0".to_string(), "1.1.0".to_string()));
    assert_eq!(importer_dep_version(&workspace, "packages/a", FOO), "1.1.0");
    assert_eq!(
        importer_dep_version(&workspace, "packages/b", FOO),
        untargeted_before,
        "the project the add didn't target keeps its resolution",
    );

    drop((root, anchor));
}

/// The version `importer` resolved `name` to, read back from the lockfile.
fn importer_dep_version(workspace: &Path, importer: &str, name: &str) -> String {
    let lockfile: Lockfile =
        serde_saphyr::from_str(&read(workspace, "pnpm-lock.yaml")).expect("parse pnpm-lock.yaml");
    lockfile
        .importers
        .get(importer)
        .and_then(|snapshot| snapshot.dependencies.as_ref())
        .and_then(|dependencies| {
            dependencies.get(&PkgName::parse(name).expect("parse the package name"))
        })
        .map_or_else(
            || panic!("{importer} has no resolved {name}"),
            |dependency| dependency.version.to_string(),
        )
}

/// The same move has to reach a project that keeps its own lockfile, where
/// importer ids are relative to the project rather than to the workspace.
#[test]
fn add_moves_a_catalog_with_a_per_project_lockfile() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, "{}");
    append_workspace_yaml(
        &workspace,
        &format!(
            "packages:\n  - 'packages/*'\nsharedWorkspaceLockfile: false\ncatalogMode: strict\ncatalog:\n  '{FOO}': 1.0.0\n",
        ),
    );
    let project = workspace.join("packages/a");
    std::fs::create_dir_all(&project).expect("create the package dir");
    std::fs::write(
        project.join("package.json"),
        serde_json::json!({
            "name": "a",
            "version": "1.0.0",
            "dependencies": { FOO: "catalog:" },
        })
        .to_string(),
    )
    .expect("write the package manifest");
    run_ok(&workspace, &["--dir", "packages/a", "install", "--lockfile-only"]);
    let widened = read(&workspace, "pnpm-workspace.yaml").replace("': 1.0.0", "': ^1.0.0");
    std::fs::write(workspace.join("pnpm-workspace.yaml"), widened).expect("widen the catalog");
    run_ok(&workspace, &["--dir", "packages/a", "install", "--lockfile-only"]);
    assert_eq!(catalog_snapshot(&project, FOO), ("^1.0.0".to_string(), "1.0.0".to_string()));

    run_ok(&workspace, &["--dir", "packages/a", "add", "--lockfile-only", &format!("{FOO}@1.1.0")]);

    assert_eq!(catalog_snapshot(&project, FOO), ("^1.0.0".to_string(), "1.1.0".to_string()));

    drop((root, anchor));
}
