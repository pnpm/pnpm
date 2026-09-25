use super::read_manifest_from_tarball;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, diagnostics::assert_diagnostic_contains};
use serde_json::json;
use std::fs;

#[test]
fn silent_flags_suppress_pack_output() {
    for flag in ["--silent", "-s", "--reporter=silent", "--loglevel=silent"] {
        let CommandTempCwd { pacquet, root, workspace, .. } = prepare_package();

        pacquet
            .with_args(["pack", flag])
            .assert()
            .success()
            .stdout("")
            .stderr("");

        let manifest = read_manifest_from_tarball(&workspace.join("pkg-1.0.0.tgz"));
        assert_eq!(manifest["name"], "pkg");
        assert_eq!(manifest["version"], "1.0.0");
        drop(root);
    }
}

#[test]
fn silent_configuration_suppresses_pack_output() {
    for config in ["reporter: silent\n", "loglevel: silent\n"] {
        let CommandTempCwd { pacquet, root, workspace, .. } = prepare_package();
        fs::write(workspace.join("pnpm-workspace.yaml"), config)
            .expect("write silent configuration");

        pacquet
            .with_arg("pack")
            .assert()
            .success()
            .stdout("")
            .stderr("");

        let manifest = read_manifest_from_tarball(&workspace.join("pkg-1.0.0.tgz"));
        assert_eq!(manifest["name"], "pkg");
        drop(root);
    }
}

#[test]
fn silent_pack_runs_lifecycle_scripts_without_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = prepare_package();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "pkg",
            "version": "1.0.0",
            "scripts": {
                "prepack": "node lifecycle.cjs prepack",
                "prepare": "node lifecycle.cjs prepare",
                "postpack": "node lifecycle.cjs postpack",
            },
        })
        .to_string(),
    )
    .expect("write lifecycle scripts");
    fs::write(
        workspace.join("lifecycle.cjs"),
        r"const fs = require('node:fs');
const stage = process.argv[2];
fs.appendFileSync('stages.txt', `${stage}\n`);
console.log(`${stage} stdout`);
console.error(`${stage} stderr`);
",
    )
    .expect("write lifecycle script");

    pacquet
        .with_args(["pack", "--silent"])
        .assert()
        .success()
        .stdout("")
        .stderr("");

    assert_eq!(
        fs::read_to_string(workspace.join("stages.txt")).expect("read executed stages"),
        "prepack\nprepare\npostpack\n",
    );
    let manifest = read_manifest_from_tarball(&workspace.join("pkg-1.0.0.tgz"));
    assert_eq!(manifest["name"], "pkg");
    drop(root);
}

#[test]
fn silent_pack_preserves_json_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = prepare_package();

    let assertion = pacquet
        .with_args(["pack", "--silent", "--json"])
        .assert()
        .success()
        .stderr("");
    let result: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("parse pack JSON");
    assert_eq!(result["name"], "pkg");
    assert_eq!(result["version"], "1.0.0");
    assert_eq!(result["files"], json!([{ "path": "package.json" }]));
    assert_eq!(result["filename"], "pkg-1.0.0.tgz");
    let manifest = read_manifest_from_tarball(&workspace.join("pkg-1.0.0.tgz"));
    assert_eq!(manifest["name"], "pkg");
    drop(root);
}

#[test]
fn normal_pack_prints_tarball_contents() {
    let CommandTempCwd { pacquet, root, .. } = prepare_package();

    let assertion = pacquet
        .with_arg("pack")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assertion.get_output().stdout);
    assert!(
        stdout.contains("Tarball Contents\npackage.json\nTarball Details\n"),
        "output: {stdout}",
    );

    drop(root);
}

#[test]
fn silent_pack_still_fails_for_an_invalid_manifest() {
    let CommandTempCwd { pacquet, root, workspace, .. } = prepare_package();
    fs::write(workspace.join("package.json"), json!({ "name": "pkg" }).to_string())
        .expect("write invalid manifest");

    let assertion = pacquet
        .with_args(["pack", "--silent"])
        .assert()
        .failure()
        .stdout("");
    let stderr = String::from_utf8_lossy(&assertion.get_output().stderr);
    assert_diagnostic_contains(&stderr, "ERR_PNPM_PACKAGE_VERSION_NOT_FOUND");

    drop(root);
}

fn prepare_package() -> CommandTempCwd<()> {
    let fixture = CommandTempCwd::init();
    fs::write(
        fixture.workspace.join("package.json"),
        json!({ "name": "pkg", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");
    fixture
}
