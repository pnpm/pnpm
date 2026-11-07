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

/// The `(project dir name, alias, version)` of each `pnpm:root` `removed` event.
fn removals(records: &[Value]) -> Vec<(String, String, Value)> {
    records
        .iter()
        .filter(|record| record["name"] == "pnpm:root")
        .filter_map(|record| {
            let prefix = Path::new(record["prefix"].as_str()?);
            let project = prefix
                .file_name()?
                .to_string_lossy()
                .into_owned();
            let removed = &record["removed"];
            Some((project, removed["name"].as_str()?.to_owned(), removed["version"].clone()))
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

/// What a scenario saw of the link it removes.
#[derive(Debug)]
struct LinkRemoval {
    linked_before: bool,
    present_after: bool,
    /// See [`removals`].
    removals: Vec<(String, String, Value)>,
}

#[test]
fn a_removed_link_dependency_is_reported() {
    let observed = under_each_linker(|pacquet, workspace| {
        write_json(
            &workspace.join("lib/package.json"),
            &json!({ "name": "lib", "version": "1.0.0" }),
        );
        write_root_manifest(workspace, &json!({ "lib": "link:lib" }));
        install(pacquet);
        let link = workspace.join("node_modules/lib");
        let linked_before = is_linked(&link);

        write_root_manifest(workspace, &json!({}));
        let records = install(pacquet_in(workspace));
        LinkRemoval {
            linked_before,
            present_after: fs::symlink_metadata(&link).is_ok(),
            removals: removals(&records),
        }
    });
    dbg!(&observed);
    for (linker, removal) in observed {
        assert!(removal.linked_before, "nodeLinker: {linker}");
        assert!(!removal.present_after, "nodeLinker: {linker}");
        let removed: Vec<_> = removal.removals
            .into_iter()
            .map(|(_, alias, version)| (alias, version))
            .collect();
        assert_eq!(removed, [("lib".to_owned(), json!("1.0.0"))], "nodeLinker: {linker}");
    }
}

/// Whether the removed dependency declares a bin. With one, the link
/// leaves a real `.bin`, so the removal reads the manifest to remove its
/// bins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkedBins {
    None,
    One,
}

/// What [`remove_a_link_dependency_with_a_malformed_manifest`] saw.
#[derive(Debug)]
struct MalformedManifestRemoval {
    has_bins_dir: bool,
    present_after: bool,
    /// The `(alias, version)` of each [`removals`] entry.
    removed: Vec<(String, Value)>,
}

fn remove_a_link_dependency_with_a_malformed_manifest(bins: LinkedBins) {
    let observed = under_each_linker(|pacquet, workspace| {
        let mut manifest = json!({ "name": "lib", "version": "1.0.0" });
        if bins == LinkedBins::One {
            manifest["bin"] = json!("cli.js");
        }
        write_json(&workspace.join("lib/package.json"), &manifest);
        fs::write(workspace.join("lib/cli.js"), "#!/usr/bin/env node\n").expect("write the bin");
        write_root_manifest(workspace, &json!({ "lib": "link:lib" }));
        install(pacquet);
        let has_bins_dir = workspace.join("node_modules/.bin").is_dir();
        fs::write(workspace.join("lib/package.json"), "{").expect("break the manifest");

        write_root_manifest(workspace, &json!({}));
        let records = install(pacquet_in(workspace));
        MalformedManifestRemoval {
            has_bins_dir,
            present_after: fs::symlink_metadata(workspace.join("node_modules/lib")).is_ok(),
            removed: removals(&records)
                .into_iter()
                .map(|(_, alias, version)| (alias, version))
                .collect(),
        }
    });
    dbg!(bins, &observed);
    for (linker, removal) in observed {
        assert_eq!(removal.has_bins_dir, bins == LinkedBins::One, "nodeLinker: {linker}");
        assert!(!removal.present_after, "nodeLinker: {linker}");
        assert_eq!(removal.removed, [("lib".to_owned(), Value::Null)], "nodeLinker: {linker}");
    }
}

#[test]
fn a_removed_link_dependency_with_a_malformed_manifest_is_unlinked() {
    remove_a_link_dependency_with_a_malformed_manifest(LinkedBins::None);
}

#[test]
fn a_removed_link_dependency_with_bins_and_a_malformed_manifest_is_unlinked() {
    remove_a_link_dependency_with_a_malformed_manifest(LinkedBins::One);
}

/// The root names `packages/shared` as `link:packages/shared`, the project
/// as `link:../shared`: one target, so the root's link dedupes the
/// project's.
fn dedupe_a_project_link_under_another_spelling(pacquet: Command, workspace: &Path) -> LinkRemoval {
    append_workspace_yaml_key(workspace, "dedupeDirectDeps", true);
    write_json(
        &workspace.join("packages/shared/package.json"),
        &json!({ "name": "shared", "version": "1.0.0" }),
    );
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
    LinkRemoval {
        linked_before,
        present_after: fs::symlink_metadata(&app_link).is_ok(),
        removals: removals(&records),
    }
}

#[test]
fn a_project_link_the_root_starts_providing_under_another_spelling_is_removed() {
    let observed = under_each_linker(dedupe_a_project_link_under_another_spelling);
    dbg!(&observed);
    for (linker, removal) in observed {
        assert!(removal.linked_before, "nodeLinker: {linker}");
        assert!(!removal.present_after, "nodeLinker: {linker}");
        assert_eq!(
            removal.removals,
            [("app".to_owned(), "shared".to_owned(), json!("1.0.0"))],
            "nodeLinker: {linker}",
        );
    }
}

/// The lockfile still records the project's deduped link, so each later
/// install finds it stale again, with nothing left to unlink.
#[test]
fn a_deduped_link_is_reported_once() {
    let observed = under_each_linker(|pacquet, workspace| {
        dedupe_a_project_link_under_another_spelling(pacquet, workspace);
        write_json(&workspace.join("other/package.json"), &json!({ "name": "other" }));
        write_root_manifest(
            workspace,
            &json!({ "shared": "link:packages/shared", "other": "link:other" }),
        );
        removals(&install(pacquet_in(workspace)))
    });
    dbg!(&observed);
    for (linker, removals) in observed {
        assert_eq!(removals, [], "nodeLinker: {linker}");
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
