//! End-to-end coverage for `file:` and `link:` catalog entries.
//!
//! A catalog lives in `pnpm-workspace.yaml`, so its relative paths are
//! written from the workspace directory while the projects that
//! dereference them sit anywhere below it.
//!
//! Covers <https://github.com/pnpm/pnpm/issues/8642>.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::Lockfile;
use pnpm_testing_utils::{bin::CommandTempCwd, fixtures::tarball_with_manifest};
use pretty_assertions::assert_eq;
use std::{fs, path::Path, process::Command};

const TARBALL: &str = "tarballs/pkg-from-tarball-1.0.0.tgz";

/// Build a workspace whose default catalog points at a local tarball and
/// a local directory, with one project per entry in `projects`.
fn workspace_with_local_catalog(workspace: &Path, projects: &[&str]) {
    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("read pnpm-workspace.yaml");
    let yaml = format!(
        "{yaml}packages:\n  - projects/*\n  - projects/*/*\ncatalog:\n  \
         pkg-from-tarball: file:./{TARBALL}\n  local-lib: link:./libs/local-lib\n",
    );
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("tarballs")).expect("create the tarball directory");
    fs::write(
        workspace.join(TARBALL),
        tarball_with_manifest(
            &serde_json::json!({ "name": "pkg-from-tarball", "version": "1.0.0" }),
        ),
    )
    .expect("write the tarball");

    fs::create_dir_all(workspace.join("libs/local-lib")).expect("create the local library");
    write_manifest(
        &workspace.join("libs/local-lib"),
        &serde_json::json!({ "name": "local-lib", "version": "2.0.0" }),
    );

    write_manifest(
        workspace,
        &serde_json::json!({ "name": "root", "version": "1.0.0", "private": true }),
    );
    for project in projects {
        let dir = workspace.join(project);
        fs::create_dir_all(&dir).expect("create the project directory");
        write_manifest(
            &dir,
            &serde_json::json!({
                "name": Path::new(project).file_name().unwrap().to_str().unwrap(),
                "version": "1.0.0",
                "dependencies": {
                    "pkg-from-tarball": "catalog:",
                    "local-lib": "catalog:",
                },
            }),
        );
    }
}

fn write_manifest(dir: &Path, manifest: &serde_json::Value) {
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

fn installed_version(project_dir: &Path, dep: &str) -> String {
    let manifest = project_dir
        .join("node_modules")
        .join(dep)
        .join("package.json");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&manifest)
            .unwrap_or_else(|error| panic!("read {}: {error}", manifest.display())),
    )
    .expect("parse the installed manifest");
    manifest["version"]
        .as_str()
        .expect("the installed manifest names a version")
        .to_string()
}

#[test]
fn local_catalog_entries_resolve_from_the_workspace_directory() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let projects = ["projects/foo", "projects/nested/bar"];
    workspace_with_local_catalog(&workspace, &projects);

    pacquet
        .with_arg("install")
        .assert()
        .success();

    for project in projects {
        let project_dir = workspace.join(project);
        assert_eq!(
            installed_version(&project_dir, "pkg-from-tarball"),
            "1.0.0",
            "{project} must install the tarball the catalog names",
        );
        assert_eq!(
            installed_version(&project_dir, "local-lib"),
            "2.0.0",
            "{project} must link the directory the catalog names",
        );
        assert_eq!(
            fs::canonicalize(project_dir.join("node_modules/local-lib"))
                .expect("resolve the linked directory"),
            fs::canonicalize(workspace.join("libs/local-lib")).expect("resolve the local library"),
            "{project}'s link must point at the workspace-relative directory",
        );
    }

    drop((root, npmrc_info));
}

#[test]
fn retargeting_a_local_catalog_entry_reinstalls_from_the_new_path() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    workspace_with_local_catalog(&workspace, &["projects/foo"]);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(installed_version(&workspace.join("projects/foo"), "pkg-from-tarball"), "1.0.0");

    fs::write(
        workspace.join("tarballs/pkg-from-tarball-2.0.0.tgz"),
        tarball_with_manifest(
            &serde_json::json!({ "name": "pkg-from-tarball", "version": "2.0.0" }),
        ),
    )
    .expect("write the second tarball");
    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("read pnpm-workspace.yaml")
        .replace("pkg-from-tarball-1.0.0.tgz", "pkg-from-tarball-2.0.0.tgz");
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write pnpm-workspace.yaml");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(installed_version(&workspace.join("projects/foo"), "pkg-from-tarball"), "2.0.0");

    drop((root, npmrc_info));
}

#[test]
fn the_lockfile_records_local_catalog_entries_as_the_catalog_writes_them() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    workspace_with_local_catalog(&workspace, &["projects/foo"]);

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let lockfile: Lockfile = serde_saphyr::from_str(
        &fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml"),
    )
    .expect("parse pnpm-lock.yaml");
    let catalog = lockfile.catalogs
        .as_ref()
        .and_then(|catalogs| catalogs.get("default"))
        .expect("the lockfile records the default catalog");
    // A local entry has no version of its own, so the recorded version
    // repeats the specifier instead of naming one importer's path.
    assert_eq!(catalog["pkg-from-tarball"].specifier, format!("file:./{TARBALL}"));
    assert_eq!(catalog["pkg-from-tarball"].version, format!("file:./{TARBALL}"));
    assert_eq!(catalog["local-lib"].specifier, "link:./libs/local-lib");
    assert_eq!(catalog["local-lib"].version, "link:./libs/local-lib");

    // The recorded entries are complete enough to install from without
    // re-resolving, which is what a `--frozen-lockfile` install proves.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    fs::remove_dir_all(workspace.join("projects/foo/node_modules")).expect("remove node_modules");
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_eq!(installed_version(&workspace.join("projects/foo"), "pkg-from-tarball"), "1.0.0");

    drop((root, npmrc_info));
}
