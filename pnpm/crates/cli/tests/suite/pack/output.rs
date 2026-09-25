use super::{assert_pack_json_lifecycle_streams, read_manifest_from_tarball};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, diagnostics::assert_diagnostic_contains};
use serde_json::json;
use std::{fs, path::Path};

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
fn hook_configuration_suppresses_pack_output() {
    for setting in ["reporter", "loglevel"] {
        let CommandTempCwd { pacquet, root, workspace, .. } = prepare_package_with_lifecycle();
        write_output_config_hook(&workspace, setting, "silent");

        pacquet
            .with_arg("pack")
            .assert()
            .success()
            .stdout("")
            .stderr("");

        assert_lifecycle_stages(&workspace);
        let manifest = read_manifest_from_tarball(&workspace.join("pkg-1.0.0.tgz"));
        assert_eq!(manifest["name"], "pkg");
        drop(root);
    }
}

#[test]
fn hook_configuration_restores_pack_output() {
    for (setting, value) in [("reporter", "default"), ("loglevel", "info")] {
        let CommandTempCwd { pacquet, root, workspace, .. } = prepare_package_with_lifecycle();
        fs::write(workspace.join("pnpm-workspace.yaml"), format!("{setting}: silent\n"))
            .expect("write silent configuration");
        write_output_config_hook(&workspace, setting, value);

        let assertion = pacquet
            .with_arg("pack")
            .assert()
            .success();
        let output = assertion.get_output();
        let printed = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        eprintln!("OUTPUT:\n{printed}");
        assert!(printed.contains("Tarball Contents"));
        assert!(printed.contains("prepack stdout"));
        assert!(printed.contains("postpack stderr"));
        assert_lifecycle_stages(&workspace);
        drop(root);
    }
}

#[test]
fn cli_flags_override_hook_output_settings() {
    for (setting, value, flag, silent) in [
        ("reporter", "silent", "--reporter=default", false),
        ("loglevel", "silent", "--loglevel=info", false),
        ("reporter", "default", "--silent", true),
        ("loglevel", "info", "--loglevel=silent", true),
    ] {
        let CommandTempCwd { pacquet, root, workspace, .. } = prepare_package();
        write_output_config_hook(&workspace, setting, value);

        let assertion = pacquet
            .with_args(["pack", flag])
            .assert()
            .success()
            .stderr("");
        let stdout = String::from_utf8_lossy(&assertion.get_output().stdout);
        eprintln!("{setting}: {value}; {flag}; STDOUT:\n{stdout}");
        if silent {
            assert_eq!(stdout, "");
        } else {
            assert!(stdout.contains("Tarball Contents"));
        }
        let manifest = read_manifest_from_tarball(&workspace.join("pkg-1.0.0.tgz"));
        assert_eq!(manifest["name"], "pkg");
        drop(root);
    }
}

#[test]
fn silent_pack_runs_lifecycle_scripts_without_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = prepare_package_with_lifecycle();

    pacquet
        .with_args(["pack", "--silent"])
        .assert()
        .success()
        .stdout("")
        .stderr("");

    assert_lifecycle_stages(&workspace);
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
    dbg!(&result);
    assert_eq!(result["name"], "pkg");
    assert_eq!(result["version"], "1.0.0");
    assert_eq!(result["files"], json!([{ "path": "package.json" }]));
    assert_eq!(result["filename"], "pkg-1.0.0.tgz");
    let manifest = read_manifest_from_tarball(&workspace.join("pkg-1.0.0.tgz"));
    assert_eq!(manifest["name"], "pkg");
    drop(root);
}

#[test]
fn silent_pack_json_preserves_lifecycle_streams() {
    for recursive in [false, true] {
        assert_pack_json_lifecycle_streams(None, recursive, &["--silent"]);
    }
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

fn write_output_config_hook(workspace: &Path, setting: &str, value: &str) {
    let settings = json!({ (setting): value });
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            "module.exports = {{ hooks: {{ updateConfig(config) {{ return {{ ...config, ...{settings} }} }} }} }};\n",
        ),
    )
    .expect("write output configuration hook");
}

fn assert_lifecycle_stages(workspace: &Path) {
    let stages = fs::read_to_string(workspace.join("stages.txt")).expect("read executed stages");
    eprintln!("LIFECYCLE STAGES:\n{stages}");
    assert_eq!(stages, "prepack\nprepare\npostpack\n");
}

fn prepare_package_with_lifecycle() -> CommandTempCwd<()> {
    let fixture = prepare_package();
    fs::write(
        fixture.workspace.join("package.json"),
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
        fixture.workspace.join("lifecycle.cjs"),
        r"const fs = require('node:fs');
const stage = process.argv[2];
fs.appendFileSync('stages.txt', `${stage}\n`);
console.log(`${stage} stdout`);
console.error(`${stage} stderr`);
",
    )
    .expect("write lifecycle script");
    fixture
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
