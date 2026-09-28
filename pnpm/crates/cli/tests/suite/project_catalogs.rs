//! A workspace project that keeps its own lockfile and its own
//! `pnpm-workspace.yaml`, such as a git submodule that is also installed on
//! its own, resolves `catalog:` against its own catalogs.
//!
//! The example throughout: the workspace pins `@pnpm.e2e/foo` to 100.0.0 for
//! every project, while `apps/legacy` has to stay on 1.0.0. That pin only
//! makes sense for `apps/legacy`, and `apps/legacy` has to install the same
//! way on its own as it does from the workspace.

use crate::{
    catalog_extends::{
        FOO, append_workspace_yaml, foo_catalog_snapshot, pacquet_ok, pair, write_package,
        write_workspace_manifest,
    },
    catalog_local_deps::read_packed_manifest,
};
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

struct Monorepo {
    root: TempDir,
    workspace: PathBuf,
    web: PathBuf,
    legacy: PathBuf,
    registry: AddMockedRegistry,
}

/// The workspace keeps a lockfile per project and pins foo to 100.0.0.
/// `apps/web` follows it; `apps/legacy`, a submodule that installs on its own
/// too, pins foo to 1.0.0 in its own `pnpm-workspace.yaml`, and carries the
/// `.npmrc` a standalone clone of it would use.
fn monorepo(shared_workspace_lockfile: bool) -> Monorepo {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml(
        &workspace,
        &format!(
            "packages:\n  - apps/*\nsharedWorkspaceLockfile: {shared_workspace_lockfile}\ncatalog:\n  '{FOO}': 100.0.0\n",
        ),
    );
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "monorepo", "private": true }).to_string(),
    )
    .expect("write the root package.json");
    let web = workspace.join("apps/web");
    write_package(&web, "web");
    let legacy = workspace.join("apps/legacy");
    write_package(&legacy, "legacy");
    write_workspace_manifest(&legacy, &format!("catalog:\n  '{FOO}': 1.0.0\n"));
    fs::write(
        legacy.join(".npmrc"),
        format!(
            "registry={}\nstore-dir={}\ncache-dir={}\n",
            npmrc_info.mock_instance.url(),
            npmrc_info.store_dir.display(),
            npmrc_info.cache_dir.display(),
        ),
    )
    .expect("write the submodule's .npmrc");
    Monorepo { root, workspace, web, legacy, registry: npmrc_info }
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn read(dir: &Path, file: &str) -> String {
    fs::read_to_string(dir.join(file)).unwrap_or_else(|_| panic!("read {file}"))
}

#[test]
fn a_project_keeping_its_own_lockfile_resolves_against_its_own_catalogs() {
    let Monorepo {
        root,
        workspace,
        web,
        legacy,
        registry,
    } = monorepo(false);

    let output = pacquet_ok(&workspace, &["install"]);

    assert_eq!(foo_catalog_snapshot(&legacy), pair("1.0.0", "1.0.0"));
    assert_eq!(foo_catalog_snapshot(&web), pair("100.0.0", "100.0.0"));
    let stdout = stdout(&output);
    assert!(!stdout.contains("apps/legacy/pnpm-workspace.yaml"), "{stdout}");
    drop((root, registry));
}

/// Installed on its own, the submodule's `pnpm-workspace.yaml` is the
/// workspace root. Both installs write the same lockfile, so neither undoes
/// the other and a frozen install passes in both places.
#[test]
fn the_project_installs_on_its_own_to_the_same_lockfile() {
    let Monorepo {
        root, workspace, legacy, registry, ..
    } = monorepo(false);
    pacquet_ok(&workspace, &["install"]);
    let lockfile = read(&legacy, "pnpm-lock.yaml");

    pacquet_ok(&legacy, &["install", "--frozen-lockfile"]);
    pacquet_ok(&legacy, &["install"]);
    assert_eq!(read(&legacy, "pnpm-lock.yaml"), lockfile);

    pacquet_ok(&workspace, &["install", "--frozen-lockfile"]);
    pacquet_ok(&workspace, &["install"]);
    assert_eq!(read(&legacy, "pnpm-lock.yaml"), lockfile);
    drop((root, registry));
}

#[test]
fn a_repeat_install_is_up_to_date() {
    let Monorepo { root, workspace, registry, .. } = monorepo(false);
    pacquet_ok(&workspace, &["install"]);

    let output = pacquet_ok(&workspace, &["install"]);

    let stdout = stdout(&output);
    assert!(stdout.contains("Already up to date"), "{stdout}");
    drop((root, registry));
}

#[test]
fn a_changed_project_catalog_is_resolved_again() {
    let Monorepo {
        root,
        workspace,
        web,
        legacy,
        registry,
    } = monorepo(false);
    pacquet_ok(&workspace, &["install"]);

    write_workspace_manifest(&legacy, &format!("catalog:\n  '{FOO}': 1.3.0\n"));
    pacquet_ok(&workspace, &["install"]);

    assert_eq!(foo_catalog_snapshot(&legacy), pair("1.3.0", "1.3.0"));
    assert_eq!(foo_catalog_snapshot(&web), pair("100.0.0", "100.0.0"));
    drop((root, registry));
}

/// A catalog entry added for the submodule lands in its own manifest, where
/// its standalone install reads it, and not in the workspace's.
#[test]
fn a_catalog_entry_added_to_the_project_is_saved_in_its_own_manifest() {
    let Monorepo {
        root, workspace, legacy, registry, ..
    } = monorepo(false);
    pacquet_ok(&workspace, &["install"]);

    pacquet_ok(
        &workspace,
        &["--filter", "legacy", "add", "--save-catalog", "@pnpm.e2e/bar@100.0.0"],
    );

    let legacy_yaml = read(&legacy, "pnpm-workspace.yaml");
    assert!(legacy_yaml.contains("'@pnpm.e2e/bar': 100.0.0"), "{legacy_yaml}");
    let workspace_yaml = read(&workspace, "pnpm-workspace.yaml");
    assert!(!workspace_yaml.contains("@pnpm.e2e/bar"), "{workspace_yaml}");
    pacquet_ok(&legacy, &["install", "--frozen-lockfile"]);
    drop((root, registry));
}

/// The published manifest carries the version the project installed.
#[test]
fn a_packed_project_replaces_catalog_with_its_own_catalog_entry() {
    let Monorepo {
        root, workspace, legacy, registry, ..
    } = monorepo(false);
    let destination = root.path().join("packed");

    pacquet_ok(
        &workspace,
        &["--filter", "legacy", "pack", "--pack-destination", &destination.to_string_lossy()],
    );

    let manifest = read_packed_manifest(&destination.join("legacy-1.0.0.tgz"));
    assert_eq!(manifest["dependencies"][FOO], "1.0.0");
    drop((root, legacy, registry));
}

/// Every settings key but the catalogs still comes from the workspace root
/// only, and the install says which ones it leaves out.
#[test]
fn the_other_settings_of_the_project_manifest_do_not_apply() {
    let Monorepo {
        root, workspace, legacy, registry, ..
    } = monorepo(false);
    write_workspace_manifest(
        &legacy,
        &format!("catalog:\n  '{FOO}': 1.0.0\noverrides:\n  '@pnpm.e2e/bar': 100.0.0\n"),
    );

    let output = pacquet_ok(&workspace, &["install"]);

    let stdout = stdout(&output);
    assert!(
        stdout.contains(
            r#"The settings "overrides" in apps/legacy/pnpm-workspace.yaml do not apply"#
        ),
        "{stdout}",
    );
    assert_eq!(foo_catalog_snapshot(&legacy), pair("1.0.0", "1.0.0"));
    drop((root, registry));
}

/// With one lockfile for the whole workspace, every project has to agree on
/// each catalog entry, so the workspace's catalogs apply to all of them.
#[test]
fn a_shared_lockfile_keeps_the_workspace_catalogs() {
    let Monorepo {
        root, workspace, legacy, registry, ..
    } = monorepo(true);

    let output = pacquet_ok(&workspace, &["install"]);

    assert_eq!(foo_catalog_snapshot(&workspace), pair("100.0.0", "100.0.0"));
    assert!(!legacy.join("pnpm-lock.yaml").exists());
    let stdout = stdout(&output);
    assert!(
        stdout.contains("The settings in apps/legacy/pnpm-workspace.yaml do not apply"),
        "{stdout}",
    );
    drop((root, registry));
}
