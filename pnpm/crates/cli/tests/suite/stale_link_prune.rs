//! Which `link:` dependencies an install unlinks and reports as removed,
//! under each node linker.

use crate::_utils;

use _utils::{append_workspace_yaml_key, assert_success, ndjson_records, pacquet_in};
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

const LINKERS: [&str; 2] = ["isolated", "hoisted"];

fn write_json(path: &Path, value: &Value) {
    fs::create_dir_all(path.parent().expect("a parent dir")).expect("create the parent dir");
    fs::write(path, value.to_string()).expect("write the JSON file");
}

fn write_root_manifest(workspace: &Path, dependencies: &Value) {
    write_json(
        &workspace.join("package.json"),
        &json!({ "name": "ws-root", "private": true, "dependencies": dependencies }),
    );
}

fn install(pacquet: Command) -> Vec<Value> {
    let output = pacquet
        .with_args(["install", "--reporter=ndjson"])
        .output()
        .expect("run pnpm install");
    assert_success(&output);
    ndjson_records(&output)
}

/// The `(project dir name, alias)` of each `pnpm:root` `removed` event.
fn removals(records: &[Value]) -> Vec<(String, String)> {
    records
        .iter()
        .filter(|record| record["name"] == "pnpm:root")
        .filter_map(|record| {
            let prefix = Path::new(record["prefix"].as_str()?);
            let project = prefix
                .file_name()?
                .to_string_lossy()
                .into_owned();
            Some((project, record["removed"]["name"].as_str()?.to_owned()))
        })
        .collect()
}

fn is_linked(path: &Path) -> bool {
    is_symlink_or_junction(path).expect("query the link")
}

/// Runs `scenario` in a fresh workspace per node linker, returning what
/// it observed under each.
fn under_each_linker<Observed>(
    scenario: impl Fn(Command, &Path) -> Observed,
) -> Vec<(&'static str, Observed)> {
    LINKERS
        .into_iter()
        .map(|linker| {
            let CommandTempCwd {
                pacquet,
                root,
                workspace,
                npmrc_info,
                ..
            } = CommandTempCwd::init().add_mocked_registry();
            let AddMockedRegistry { mock_instance, .. } = npmrc_info;
            append_workspace_yaml_key(&workspace, "nodeLinker", linker);
            append_workspace_yaml_key(&workspace, "packages", "['packages/*']");
            let observed = scenario(pacquet, &workspace);
            drop((root, mock_instance));
            (linker, observed)
        })
        .collect()
}

#[test]
fn a_removed_link_dependency_is_reported() {
    let observed = under_each_linker(|pacquet, workspace| {
        write_json(&workspace.join("lib/package.json"), &json!({ "name": "lib" }));
        write_root_manifest(workspace, &json!({ "lib": "link:lib" }));
        install(pacquet);
        let link = workspace.join("node_modules/lib");
        let linked_before = is_linked(&link);

        write_root_manifest(workspace, &json!({}));
        let records = install(pacquet_in(workspace));
        (linked_before, link.exists(), removals(&records))
    });
    dbg!(&observed);
    for (linker, (linked_before, exists_after, removals)) in observed {
        assert!(linked_before, "nodeLinker: {linker}");
        assert!(!exists_after, "nodeLinker: {linker}");
        let workspace_dir_removals: Vec<_> = removals
            .into_iter()
            .map(|(_, alias)| alias)
            .collect();
        assert_eq!(workspace_dir_removals, ["lib"], "nodeLinker: {linker}");
    }
}

/// The root names `packages/shared` as `link:packages/shared`, the project
/// as `link:../shared`: one target, so the root's link dedupes the
/// project's.
#[test]
fn a_project_link_the_root_starts_providing_under_another_spelling_is_removed() {
    let observed = under_each_linker(|pacquet, workspace| {
        append_workspace_yaml_key(workspace, "dedupeDirectDeps", true);
        write_json(&workspace.join("packages/shared/package.json"), &json!({ "name": "shared" }));
        write_json(
            &workspace.join("packages/app/package.json"),
            &json!({ "name": "app", "dependencies": { "shared": "link:../shared" } }),
        );
        write_root_manifest(workspace, &json!({}));
        install(pacquet);
        let app_link = workspace.join("packages/app/node_modules/shared");
        let linked_before = is_linked(&app_link);

        write_root_manifest(workspace, &json!({ "shared": "link:packages/shared" }));
        let records = install(pacquet_in(workspace));
        let exists_after = fs::symlink_metadata(&app_link).is_ok();
        (linked_before, exists_after, removals(&records))
    });
    dbg!(&observed);
    for (linker, (linked_before, exists_after, removals)) in observed {
        assert!(linked_before, "nodeLinker: {linker}");
        assert!(!exists_after, "nodeLinker: {linker}");
        assert_eq!(removals, [("app".to_owned(), "shared".to_owned())], "nodeLinker: {linker}");
    }
}

/// The root and the project both record `link:vendor/x`, each meaning its
/// own `vendor/x`, so the root's link does not dedupe the project's.
#[test]
fn a_project_link_spelled_like_the_roots_to_another_dir_is_kept() {
    let observed = under_each_linker(|pacquet, workspace| {
        append_workspace_yaml_key(workspace, "dedupeDirectDeps", true);
        write_json(&workspace.join("vendor/x/package.json"), &json!({ "name": "root-x" }));
        write_json(
            &workspace.join("packages/app/vendor/x/package.json"),
            &json!({ "name": "app-x" }),
        );
        write_json(
            &workspace.join("packages/app/package.json"),
            &json!({ "name": "app", "dependencies": { "x": "link:vendor/x" } }),
        );
        write_root_manifest(workspace, &json!({ "x": "link:vendor/x" }));
        install(pacquet);
        // A second install with a change to make, so it runs the prune.
        write_json(&workspace.join("other/package.json"), &json!({ "name": "other" }));
        write_root_manifest(workspace, &json!({ "x": "link:vendor/x", "other": "link:other" }));
        let records = install(pacquet_in(workspace));
        let app_link = workspace.join("packages/app/node_modules/x");
        let target_name = fs::read_to_string(app_link.join("package.json"))
            .ok()
            .and_then(|manifest| serde_json::from_str::<Value>(&manifest).ok())
            .map(|manifest| manifest["name"].clone());
        (target_name, removals(&records))
    });
    dbg!(&observed);
    for (linker, (target_name, removals)) in observed {
        assert_eq!(target_name, Some(json!("app-x")), "nodeLinker: {linker}");
        assert_eq!(removals, [], "nodeLinker: {linker}");
    }
}
