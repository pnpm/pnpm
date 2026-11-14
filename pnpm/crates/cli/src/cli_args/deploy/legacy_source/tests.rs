use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;

use pnpm_lockfile::{ImporterDepVersion, Lockfile};
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::Project;
use serde_json::json;

use super::{
    super::{SelectedProject, index_projects},
    ConvertCtx, HashMap, LegacyDeploySource, create_file_url_key, relative_file_key,
};

#[test]
fn relative_file_keys_decode_spaces_in_the_source_path() {
    let root = tempfile::tempdir().unwrap();
    let source_root = root.path().join("workspace (old)");
    let source = source_root.join("source with spaces");
    let deploy = source_root.join("out");
    let projects = HashMap::new();
    let ctx = ConvertCtx {
        projects_by_path: &projects,
        deploy_dir: &deploy,
        lockfile_dir: root.path(),
        deployed_project_root: &source,
        references: crate::cli_args::deploy::lockfile::ReferenceLocation::Workspace,
    };
    let name = "fixture".parse().unwrap();
    let key = create_file_url_key(&source, "", &projects, Some(&name)).unwrap();
    assert_eq!(
        relative_file_key(&key, &ctx).unwrap().to_string(),
        "fixture@file:../source with spaces",
    );
}

fn project(root_dir: std::path::PathBuf, manifest: serde_json::Value) -> Project {
    let manifest_path = root_dir.join("package.json");
    Project {
        root_dir,
        manifest: PackageManifest::from_value(manifest_path, manifest),
        dependency_manifest: None,
    }
}

fn select_first(projects: Vec<Project>) -> SelectedProject {
    let projects_by_path = index_projects(&projects);
    let project = projects
        .into_iter()
        .next()
        .expect("first project");
    SelectedProject { project, projects_by_path, workspace_projects: Vec::new() }
}

#[test]
fn rebase_ignores_publish_directory_that_escapes_importer_root_or_mismatches_manifest() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().join("workspace");
    let outside = tmp.path().join("outside");
    let app_dir = workspace.join("packages/app");
    let lib_dir = workspace.join("packages/lib");
    let deploy_dir = workspace.join("deploy");
    fs::create_dir_all(&outside).expect("create outside");
    fs::create_dir_all(&app_dir).expect("create app_dir");
    fs::create_dir_all(&lib_dir).expect("create lib_dir");

    let app = project(app_dir, json!({ "name": "app", "dependencies": { "lib": "workspace:*" } }));
    // Even if `lib`'s manifest also declared `../../../outside`, containment rejects it.
    let lib_escaping = project(
        lib_dir,
        json!({ "name": "lib", "publishConfig": { "directory": "../../../outside" } }),
    );
    let selected = select_first(vec![app, lib_escaping]);
    let lockfile: Lockfile = serde_json::from_value(json!({
        "lockfileVersion": "9.0",
        "importers": {
            "packages/app": {
                "dependencies": {
                    "lib": { "specifier": "workspace:*", "version": "link:../../../outside" },
                },
            },
            "packages/lib": {
                "publishDirectory": "../../../outside",
            },
        },
    }))
    .expect("parse lockfile");

    let mut source = LegacyDeploySource { lockfile, path: workspace.join("pnpm-lock.yaml") };
    source.rebase(&selected, &deploy_dir, &workspace).expect("rebase");

    let packages = source.lockfile.packages.as_ref().expect("packages");
    assert!(
        !packages
            .keys()
            .any(|key| key.to_string().contains("outside")),
        "escaping publishDirectory must not synthesize a directory package: {packages:?}",
    );
    let lib_version = &source.lockfile.importers["."].dependencies
        .as_ref()
        .expect("app dependencies")[&"lib".parse().expect("pkg name")]
        .version;
    assert!(
        matches!(lib_version, ImporterDepVersion::Link(_)),
        "escaping link must remain a Link rather than becoming an Alias copy: {lib_version:?}",
    );
}

#[test]
fn rebase_ignores_publish_directory_absent_from_project_manifest_and_keeps_contained_one() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().join("workspace");
    let app_dir = workspace.join("packages/app");
    let stale_lib_dir = workspace.join("packages/stale-lib");
    let built_lib_dir = workspace.join("packages/built-lib");
    let deploy_dir = workspace.join("deploy");
    fs::create_dir_all(&app_dir).expect("create app_dir");
    fs::create_dir_all(stale_lib_dir.join("dist")).expect("create stale_lib_dir/dist");
    fs::create_dir_all(&built_lib_dir).expect("create built_lib_dir");

    let app = project(app_dir, json!({ "name": "app" }));
    let stale_lib = project(stale_lib_dir, json!({ "name": "stale-lib" }));
    // `built-lib/dist` is not yet created on disk, verifying unbuilt contained publish dirs work.
    let built_lib = project(
        built_lib_dir,
        json!({ "name": "built-lib", "publishConfig": { "directory": "dist" } }),
    );
    let selected = select_first(vec![app, stale_lib, built_lib]);
    let lockfile: Lockfile = serde_json::from_value(json!({
        "lockfileVersion": "9.0",
        "importers": {
            "packages/app": {},
            "packages/stale-lib": { "publishDirectory": "dist" },
            "packages/built-lib": { "publishDirectory": "dist" },
        },
    }))
    .expect("parse lockfile");

    let mut source = LegacyDeploySource { lockfile, path: workspace.join("pnpm-lock.yaml") };
    source.rebase(&selected, &deploy_dir, &workspace).expect("rebase");

    let keys: Vec<String> = source.lockfile.packages
        .as_ref()
        .expect("packages")
        .keys()
        .map(ToString::to_string)
        .collect();
    assert!(
        !keys
            .iter()
            .any(|key| key.contains("stale-lib") && key.contains("dist")),
        "stale lockfile publishDirectory absent from manifest must be ignored: {keys:?}",
    );
    assert!(
        keys.iter()
            .any(|key| key == "built-lib@file:../packages/built-lib/dist"),
        "manifest-declared contained publishDirectory must be kept: {keys:?}",
    );
}

#[cfg(unix)]
#[test]
fn rebase_ignores_publish_directory_that_traverses_a_symlink() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().join("workspace");
    let outside = tmp.path().join("outside");
    let app_dir = workspace.join("packages/app");
    let lib_dir = workspace.join("packages/lib");
    let deploy_dir = workspace.join("deploy");
    fs::create_dir_all(&outside).expect("create outside");
    fs::create_dir_all(&app_dir).expect("create app_dir");
    fs::create_dir_all(&lib_dir).expect("create lib_dir");
    symlink(&outside, lib_dir.join("dist")).expect("symlink dist -> outside");

    let app = project(app_dir, json!({ "name": "app" }));
    let lib = project(lib_dir, json!({ "name": "lib", "publishConfig": { "directory": "dist" } }));
    let selected = select_first(vec![app, lib]);
    let lockfile: Lockfile = serde_json::from_value(json!({
        "lockfileVersion": "9.0",
        "importers": {
            "packages/app": {},
            "packages/lib": { "publishDirectory": "dist" },
        },
    }))
    .expect("parse lockfile");

    let mut source = LegacyDeploySource { lockfile, path: workspace.join("pnpm-lock.yaml") };
    source.rebase(&selected, &deploy_dir, &workspace).expect("rebase");

    let keys: Vec<String> = source.lockfile.packages
        .as_ref()
        .expect("packages")
        .keys()
        .map(ToString::to_string)
        .collect();
    assert!(
        !keys
            .iter()
            .any(|key| key.contains("packages/lib/dist")),
        "symlinked publishDirectory must not synthesize a directory package: {keys:?}",
    );
}
