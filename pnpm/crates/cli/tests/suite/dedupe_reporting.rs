use crate::_utils::{assert_success, ndjson_records};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pnpm_testing_utils::command_env::CommandTestExt;
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    path::Path,
    process::{Command, Output},
};

const SIMPLE_GRAPH: &[&str] =
    &["@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0", "@pnpm.e2e/pkg-with-1-dep@100.0.0"];

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

fn dedupe_fixture() -> CommandTempCwd<AddMockedRegistry> {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    fs::write(
        fixture.workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet_at(&fixture.workspace)
        .with_arg("install")
        .assert()
        .success();
    fixture
}

#[test]
fn full_dedupe_reports_one_store_status_per_resolved_package() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } = dedupe_fixture();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    assert_one_store_status_per_resolved_package(
        &pacquet_at(&workspace)
            .with_args(["dedupe", "--reporter=ndjson"])
            .output()
            .expect("run dedupe"),
        Some(SIMPLE_GRAPH),
    );

    drop((root, npmrc_info));
}

#[test]
fn resolve_only_dedupe_preserves_store_reuse_reporting() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } = dedupe_fixture();

    assert_one_store_status_per_resolved_package(
        &pacquet_at(&workspace)
            .with_args(["dedupe", "--lockfile-only", "--reporter=ndjson"])
            .output()
            .expect("run lockfile-only dedupe"),
        Some(SIMPLE_GRAPH),
    );

    fs::write(workspace.join("pnpm-workspace.yaml"), "enableModulesDir: false\n")
        .expect("write pnpm-workspace.yaml");
    assert_one_store_status_per_resolved_package(
        &pacquet_at(&workspace)
            .with_args(["dedupe", "--reporter=ndjson"])
            .output()
            .expect("run config resolve-only dedupe"),
        Some(SIMPLE_GRAPH),
    );

    drop((root, npmrc_info));
}

#[test]
fn filtered_dedupe_reports_the_whole_resolved_graph() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    for (name, dependency) in [("a", "@pnpm.e2e/pkg-with-1-dep"), ("b", "@pnpm.e2e/bar")] {
        let project = workspace.join("packages").join(name);
        fs::create_dir_all(&project).expect("create project");
        fs::write(
            project.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": {
                    (dependency): "100.0.0",
                },
            })
            .to_string(),
        )
        .expect("write package.json");
    }
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert_one_store_status_per_resolved_package(
        &pacquet_at(&workspace)
            .with_args(["dedupe", "--filter", "a", "--reporter=ndjson"])
            .output()
            .expect("run filtered dedupe"),
        Some(&[
            "@pnpm.e2e/bar@100.0.0",
            "@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0",
            "@pnpm.e2e/pkg-with-1-dep@100.0.0",
        ]),
    );

    drop((root, mock_instance));
}

fn assert_one_store_status_per_resolved_package(output: &Output, expected: Option<&[&str]>) {
    assert_success(output);
    let records = ndjson_records(output);
    let last_progress = records
        .iter()
        .rposition(|record| record.get("name").and_then(Value::as_str) == Some("pnpm:progress"))
        .expect("progress event");
    let summary = records
        .iter()
        .position(|record| record.get("name").and_then(Value::as_str) == Some("pnpm:summary"))
        .expect("summary event");
    assert!(last_progress < summary, "package progress must precede summary\n{output:?}");
    let mut resolved = package_ids(&records, &["resolved"]);
    resolved.dedup();
    let statuses = package_ids(&records, &["fetched", "found_in_store"]);
    assert!(!resolved.is_empty(), "no resolved events\n{output:?}");
    assert_eq!(
        statuses
            .iter()
            .collect::<HashSet<_>>()
            .len(),
        statuses.len(),
        "duplicate terminal package statuses\n{output:?}",
    );
    assert_eq!(statuses, resolved, "package statuses did not match resolved packages\n{output:?}");
    if let Some(expected) = expected {
        assert_eq!(
            resolved,
            expected
                .iter()
                .map(|package_id| (*package_id).to_string())
                .collect::<Vec<_>>(),
            "unexpected filtered resolve graph\n{output:?}",
        );
    }
}

fn package_ids(records: &[Value], statuses: &[&str]) -> Vec<String> {
    let mut package_ids: Vec<String> = records
        .iter()
        .filter(|record| {
            record.get("name").and_then(Value::as_str) == Some("pnpm:progress")
                && record
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| statuses.contains(&status))
        })
        .filter_map(|record| {
            record
                .get("packageId")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    package_ids.sort();
    package_ids
}
