//! `lockfile.includeResolutionSettings` records the settings that shape a
//! resolution in the lockfile's `settings`, and holds later installs to them.

use crate::{
    _utils::read_lockfile,
    auto_dedupe::{pnpm_at, write_project, write_settings},
};
use command_extra::CommandExtra;
use pnpm_lockfile::ResolutionSettings;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::Value;
use std::{fs, path::Path};

const RECORDING: &str = "lockfile:\n  includeResolutionSettings: true\n";
const NOT_RECORDED: &str = "The lockfile does not record that it was resolved with autoDedupe";

fn recorded(workspace: &Path) -> ResolutionSettings {
    read_lockfile(&workspace.join("pnpm-lock.yaml")).settings.expect("the lockfile has settings")
        .resolution
}

fn install(workspace: &Path, args: &[&str]) -> std::process::Output {
    let output = pnpm_at(workspace)
        .with_env("PNPM_CONFIG_CI", "false")
        .with_args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    output
}

/// `a` injects the leaf workspace project `b`, which `dedupeInjectedDeps`
/// records as `link:` and otherwise as an injected `file:` entry.
fn write_injected_workspace(workspace: &Path, settings: &str) {
    fs::write(workspace.join("package.json"), r#"{"name":"root","private":true}"#).unwrap();
    write_settings(workspace, format!("packages:\n  - packages/*\n{settings}")).unwrap();
    for (dir, manifest) in [
        (
            "a",
            serde_json::json!({
                "name": "a",
                "version": "1.0.0",
                "dependencies": { "b": "workspace:*" },
                "dependenciesMeta": { "b": { "injected": true } },
            }),
        ),
        ("b", serde_json::json!({ "name": "b", "version": "1.0.0" })),
    ] {
        fs::create_dir_all(workspace.join("packages").join(dir)).unwrap();
        fs::write(
            workspace
                .join("packages")
                .join(dir)
                .join("package.json"),
            manifest.to_string(),
        )
        .unwrap();
    }
}

#[test]
fn the_lockfile_records_resolution_settings_only_when_asked() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_injected_workspace(&workspace, "");
    install(&workspace, &["install"]);
    assert_eq!(recorded(&workspace), ResolutionSettings::default());

    write_injected_workspace(&workspace, RECORDING);
    install(&workspace, &["install"]);
    assert_eq!(
        recorded(&workspace),
        ResolutionSettings {
            auto_dedupe: Some(false),
            dedupe_injected_deps: Some(true),
            dedupe_peer_dependents: Some(true),
            link_workspace_packages: Some(Value::Bool(false)),
        },
    );
    drop((root, mock_instance));
}

#[test]
fn a_changed_recorded_setting_re_resolves_the_lockfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_injected_workspace(&workspace, RECORDING);
    install(&workspace, &["install"]);
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    assert!(fs::read_to_string(&lockfile_path).unwrap().contains("link:../b"));

    write_injected_workspace(&workspace, &format!("{RECORDING}dedupeInjectedDeps: false\n"));
    install(&workspace, &["install"]);
    let lockfile = fs::read_to_string(&lockfile_path).unwrap();
    assert!(lockfile.contains("file:packages/b"), "{lockfile}");
    assert_eq!(recorded(&workspace).dedupe_injected_deps, Some(false));
    drop((root, mock_instance));
}

#[test]
fn a_frozen_install_rejects_a_changed_recorded_setting() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_injected_workspace(&workspace, RECORDING);
    install(&workspace, &["install"]);

    write_injected_workspace(&workspace, &format!("{RECORDING}dedupePeerDependents: false\n"));
    let output = pnpm_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout}{stderr}");
    assert!(combined.contains("ERR_PNPM_LOCKFILE_CONFIG_MISMATCH"), "{combined}");
    assert!(combined.contains("settings.dedupePeerDependents"), "{combined}");
    drop((root, mock_instance));
}

/// The verify-deps gate script, run through `node -e` for portability.
fn write_project_with_script(workspace: &Path, version: &str) {
    write_project(workspace, "low", version);
    let path = workspace.join("low/package.json");
    let mut manifest: serde_json::Map<String, Value> =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    manifest.insert(
        "scripts".to_string(),
        serde_json::json!({ "hello": r#"node -e "console.log('script-output')""# }),
    );
    fs::write(path, Value::Object(manifest).to_string()).unwrap();
}

/// A deploy server runs `pnpm install --frozen-lockfile` on a lockfile that
/// records `autoDedupe`. Neither a later script nor a later install
/// re-resolves it ([pnpm/pnpm#16583](https://github.com/pnpm/pnpm/issues/16583)).
#[test]
fn a_frozen_install_of_a_recorded_dedupe_needs_no_further_resolution() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(workspace.join("package.json"), r#"{"name":"root","private":true}"#).unwrap();
    write_settings(
        &workspace,
        format!("packages:\n  - low\n  - high\nautoDedupe: true\n{RECORDING}"),
    )
    .unwrap();
    write_project_with_script(&workspace, "100.0.0");
    write_project(&workspace, "high", "100.1.0");
    install(&workspace, &["install"]);
    write_project_with_script(&workspace, "100.1.0");
    install(&workspace, &["install", "--lockfile-only"]);
    // A deploy server has no local record of the deduplicating resolution.
    fs::remove_dir_all(root.path().join("pacquet-cache/auto-dedupe-baselines")).unwrap();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read(&lockfile_path).unwrap();
    install(&workspace, &["install", "--frozen-lockfile"]);

    let output = pnpm_at(&workspace.join("low"))
        .with_env("PNPM_CONFIG_CI", "false")
        .with_args(["run", "hello"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        r#"$ node -e "console.log('script-output')""#,
    );
    let skipped = |args: &[&str]| {
        let output = install(&workspace, args);
        String::from_utf8_lossy(&output.stdout)
            .contains("Lockfile is up to date, resolution step is skipped")
    };
    assert!(skipped(&["install", "--config.optimistic-repeat-install=false"]));
    // Without the pnpmfile checksum comparison the record cannot be trusted.
    assert!(!skipped(&[
        "install",
        "--config.optimistic-repeat-install=false",
        "--config.ignore-pnpmfile=true",
    ]));
    assert_eq!(fs::read(&lockfile_path).unwrap(), lockfile);
    drop((root, mock_instance));
}

#[test]
fn a_resolving_install_suggests_recording_auto_dedupe() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_settings(&workspace, "packages:\n  - low\nautoDedupe: true\n").unwrap();
    write_project(&workspace, "low", "100.0.0");
    let output = install(&workspace, &["install"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains(NOT_RECORDED), "{output:?}");

    write_settings(&workspace, format!("packages:\n  - low\nautoDedupe: true\n{RECORDING}"))
        .unwrap();
    let output = install(&workspace, &["install"]);
    assert!(!String::from_utf8_lossy(&output.stdout).contains(NOT_RECORDED), "{output:?}");
    drop((root, mock_instance));
}

#[test]
fn recording_resolution_settings_rejects_delegated_resolution() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_settings(&workspace, format!("packages:\n  - low\n{RECORDING}")).unwrap();
    write_project(&workspace, "low", "100.0.0");
    let output = pnpm_at(&workspace)
        .with_args(["install", "--pnpr-server", "http://127.0.0.1:1", "--lockfile-only"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_RESOLUTION_SETTINGS_WITH_PNPR_SERVER"), "{stderr}");
    drop((root, npmrc_info));
}

/// The repeat-install fast path also notices the setting turned on where it
/// watches no file, here through the environment.
#[test]
fn turning_the_setting_on_records_the_settings_on_the_next_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_injected_workspace(&workspace, "");
    install(&workspace, &["install"]);
    let output = pnpm_at(&workspace)
        .with_env("PNPM_CONFIG_LOCKFILE", r#"{"includeResolutionSettings":true}"#)
        .with_args(["install"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(recorded(&workspace).dedupe_injected_deps, Some(true));
    drop((root, mock_instance));
}
