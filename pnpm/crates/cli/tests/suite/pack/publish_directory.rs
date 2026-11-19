use super::read_manifest_from_tarball;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, diagnostics::assert_diagnostic_contains};
use serde_json::json;
use std::fs;

#[test]
fn pack_rejects_publish_config_directory_outside_project() {
    let outside_dir = tempfile::tempdir().expect("outside tempdir");
    let outside = outside_dir.path().join("outside");
    fs::create_dir_all(&outside).expect("create outside dir");
    fs::write(
        outside.join("package.json"),
        json!({ "name": "outside-pkg", "version": "1.0.0" }).to_string(),
    )
    .expect("write outside package.json");
    fs::write(outside.join("secret.txt"), "SENSITIVE_DATA=123\n").expect("write secret.txt");

    let outside_str = outside
        .to_str()
        .expect("utf8 outside")
        .to_string();
    for directory in [outside_str.as_str(), "../../outside", "/dist"] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        let pkg_dir = workspace.join("pkg");
        fs::create_dir_all(&pkg_dir).expect("create pkg dir");
        fs::write(
            pkg_dir.join("package.json"),
            json!({
                "name": "repro",
                "version": "1.0.0",
                "publishConfig": { "directory": directory },
            })
            .to_string(),
        )
        .expect("write pkg package.json");

        let output = pacquet
            .with_args(["--dir", pkg_dir.to_str().expect("utf8 pkg"), "pack", "--ignore-scripts"])
            .output()
            .expect("spawn pnpm pack");
        assert!(!output.status.success(), "directory {directory:?} must fail");
        let combined = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert_diagnostic_contains(&combined, "ERR_PNPM_INVALID_PUBLISH_DIRECTORY");
        assert!(!outside.join("outside-pkg-1.0.0.tgz").exists());
        assert!(!pkg_dir.join("repro-1.0.0.tgz").exists());
        drop(root);
    }
}

#[test]
fn pack_allows_relative_publish_config_directory() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let dist_dir = workspace.join("dist");
    fs::create_dir_all(&dist_dir).expect("create dist");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "pkg",
            "version": "1.0.0",
            "publishConfig": { "directory": "./dist" },
        })
        .to_string(),
    )
    .expect("write root package.json");
    fs::write(
        dist_dir.join("package.json"),
        json!({ "name": "pkg-dist", "version": "1.0.0" }).to_string(),
    )
    .expect("write dist package.json");

    pacquet
        .with_args(["pack", "--ignore-scripts"])
        .assert()
        .success();

    let manifest = read_manifest_from_tarball(&dist_dir.join("pkg-dist-1.0.0.tgz"));
    assert_eq!(manifest["name"], "pkg-dist");

    drop(root);
}
