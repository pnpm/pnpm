use crate::_utils::{importer_version, read_lockfile};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use std::{fs, path::Path, process::Command};

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const PARENT: &str = "@pnpm.e2e/pkg-with-1-dep";

fn pnpm_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

fn write_settings(workspace: &Path, settings: impl AsRef<str>) -> std::io::Result<()> {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut current: serde_json::Map<String, serde_json::Value> =
        serde_saphyr::from_str(&fs::read_to_string(&path)?).unwrap();
    current.retain(|key, _| {
        matches!(key.as_str(), "storeDir" | "cacheDir" | "enableGlobalVirtualStore")
    });
    let new: serde_json::Map<String, serde_json::Value> =
        serde_saphyr::from_str(settings.as_ref()).unwrap();
    current.extend(new);
    fs::write(path, serde_saphyr::to_string(&current).unwrap())
}

fn write_project(workspace: &Path, project: &str, version: &str) {
    let dir = workspace.join(project);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.json"),
        serde_json::json!({
            "name": project,
            "dependencies": {DEP: version, PARENT: "100.0.0"},
        })
        .to_string(),
    )
    .unwrap();
}

fn prepare_duplicates(workspace: &Path) {
    write_settings(workspace, "packages:\n  - low\n  - high\n").unwrap();
    write_project(workspace, "low", "100.0.0");
    write_project(workspace, "high", "100.1.0");
    pnpm_at(workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    write_project(workspace, "low", "^100.0.0");
    let mut lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    let low = lockfile.importers.get_mut("low").unwrap();
    low.dependencies
        .as_mut()
        .unwrap()
        .get_mut(&DEP.parse().unwrap())
        .unwrap()
        .specifier = "^100.0.0".into();
    lockfile
        .save_to_path(&workspace.join("pnpm-lock.yaml"))
        .unwrap();
}

#[test]
fn enabling_auto_dedupe_consolidates_existing_versions_without_changing_lockfile_format() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    prepare_duplicates(&workspace);
    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    assert_eq!(
        importer_version(&read_lockfile(&workspace.join("pnpm-lock.yaml")), "low", DEP),
        "100.0.0",
    );
    pnpm_at(&workspace)
        .with_args(["install", "--auto-dedupe", "--lockfile-only"])
        .assert()
        .success();
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, "low", DEP), "100.1.0");
    assert!(
        !lockfile.packages
            .as_ref()
            .unwrap()
            .contains_key(&format!("{DEP}@100.0.0").parse().unwrap()),
    );
    assert!(lockfile.extra.is_empty());
    drop((root, mock_instance));
}

#[test]
fn frozen_install_does_not_record_a_dedupe_baseline_and_repeat_install_skips_after_resolution() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    prepare_duplicates(&workspace);
    write_settings(&workspace, "packages:\n  - low\n  - high\nautoDedupe: true\n").unwrap();
    let path = workspace.join("pnpm-lock.yaml");
    let original = fs::read(&path).unwrap();
    pnpm_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_eq!(fs::read(&path).unwrap(), original);
    pnpm_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(
        importer_version(&read_lockfile(&workspace.join("pnpm-lock.yaml")), "low", DEP),
        "100.1.0",
    );
    let output = pnpm_at(&workspace)
        .with_args(["install", "--offline"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Already up to date"), "{stdout}");
    drop((root, mock_instance));
}

#[test]
fn auto_dedupe_preserves_incompatible_exact_versions_and_can_be_disabled() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    prepare_duplicates(&workspace);
    write_settings(&workspace, "packages:\n  - low\n  - high\nautoDedupe: true\n").unwrap();
    pnpm_at(&workspace)
        .with_args(["install", "--no-auto-dedupe", "--lockfile-only"])
        .assert()
        .success();
    assert_eq!(
        importer_version(&read_lockfile(&workspace.join("pnpm-lock.yaml")), "low", DEP),
        "100.0.0",
    );
    write_project(&workspace, "low", "100.0.0");
    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, "low", DEP), "100.0.0");
    assert_eq!(importer_version(&lockfile, "high", DEP), "100.1.0");
    drop((root, mock_instance));
}

#[test]
fn partial_change_discovers_a_new_candidate_and_updates_existing_consumers() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_settings(&workspace, "packages:\n  - low\n  - high\n").unwrap();
    write_project(&workspace, "low", "100.0.0");
    write_project(&workspace, "high", "100.0.0");
    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    write_project(&workspace, "low", "^100.0.0");
    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    assert_eq!(
        importer_version(&read_lockfile(&workspace.join("pnpm-lock.yaml")), "low", DEP),
        "100.0.0",
    );
    pnpm_at(&workspace.join("high"))
        .with_args(["add", &format!("{DEP}@100.1.0"), "--auto-dedupe", "--lockfile-only"])
        .assert()
        .success();
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, "low", DEP), "100.1.0");
    assert_eq!(importer_version(&lockfile, "high", DEP), "100.1.0");
    assert!(
        !lockfile.packages
            .as_ref()
            .unwrap()
            .contains_key(&format!("{DEP}@100.0.0").parse().unwrap()),
    );
    drop((root, npmrc_info));
}

#[test]
fn auto_dedupe_respects_overrides_and_explicit_updates() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    prepare_duplicates(&workspace);
    write_settings(
        &workspace,
        format!("packages:\n  - low\n  - high\nautoDedupe: true\noverrides:\n  '{DEP}': 100.0.0\n"),
    )
    .unwrap();
    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, "low", DEP), "100.0.0");
    assert_eq!(importer_version(&lockfile, "high", DEP), "100.0.0");
    write_settings(&workspace, "packages:\n  - low\n  - high\nautoDedupe: true\n").unwrap();
    pnpm_at(&workspace)
        .with_args(["update", "-r", DEP, "--latest", "--lockfile-only"])
        .assert()
        .success();
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, "low", DEP), "101.0.0");
    assert_eq!(importer_version(&lockfile, "high", DEP), "101.0.0");
    drop((root, npmrc_info));
}

#[test]
fn auto_dedupe_from_environment_preserves_incompatible_auto_installed_peers() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_settings(&workspace, "packages:\n  - low\n  - high\n").unwrap();
    for (project, dependencies) in [
        ("low", serde_json::json!({"@pnpm.e2e/wants-peer-c-1": "1.0.0"})),
        ("high", serde_json::json!({"@pnpm.e2e/peer-c": "^2.0.0"})),
    ] {
        fs::create_dir_all(workspace.join(project)).unwrap();
        fs::write(
            workspace.join(project).join("package.json"),
            serde_json::json!({"name": project, "dependencies": dependencies}).to_string(),
        )
        .unwrap();
    }
    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .with_env("PNPM_CONFIG_AUTO_DEDUPE", "true")
        .assert()
        .success();
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(
        importer_version(&lockfile, "low", "@pnpm.e2e/wants-peer-c-1"),
        "1.0.0(@pnpm.e2e/peer-c@1.0.1)",
    );
    assert_eq!(importer_version(&lockfile, "high", "@pnpm.e2e/peer-c"), "2.0.0");
    drop((root, npmrc_info));
}

#[test]
fn auto_dedupe_rejects_delegated_resolution_before_contacting_the_server() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_project(&workspace, "low", "100.0.0");
    let output = pnpm_at(&workspace.join("low"))
        .with_args([
            "install",
            "--auto-dedupe",
            "--pnpr-server",
            "http://127.0.0.1:1",
            "--lockfile-only",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_AUTO_DEDUPE_WITH_PNPR_SERVER"), "{stderr}");
    drop((root, npmrc_info));
}
