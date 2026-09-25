use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

fn pnpm(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find pnpm").with_current_dir(workspace)
}

fn set_loglevel(workspace: &Path, level: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).expect("read workspace settings");
    yaml.push_str("\nloglevel: ");
    yaml.push_str(level);
    yaml.push('\n');
    fs::write(path, yaml).expect("write workspace settings");
}

fn script_fixture() -> CommandTempCwd<()> {
    let fixture = CommandTempCwd::init();
    fs::write(fixture.workspace.join("pnpm-workspace.yaml"), "").expect("write workspace settings");
    fs::write(
        fixture.workspace.join("package.json"),
        serde_json::json!({
            "name": "loglevel-test",
            "version": "1.0.0",
            "scripts": { "test": r#"node -e "console.log('script-output')""# },
        })
        .to_string(),
    )
    .expect("write package.json");
    fixture
}

#[test]
fn configured_silent_covers_install_fast_path_and_deferred_reporters() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let workspace = &fixture.workspace;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "loglevel-test",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    set_loglevel(workspace, "silent");

    for args in [
        vec!["install"],
        vec!["install"],
        vec!["--reporter=ndjson", "fetch"],
        vec!["--reporter=ndjson", "add", "@pnpm.e2e/bar@100.0.0"],
    ] {
        pnpm(workspace)
            .with_args(args)
            .assert()
            .success()
            .stdout("")
            .stderr("");
    }
    assert!(workspace.join("node_modules/@pnpm.e2e/bar/package.json").exists());

    assert_output_contains(
        pnpm(workspace).with_args(["--loglevel=info", "install"]),
        "Already up to date",
    );
    let CommandTempCwd {
        root,
        npmrc_info: AddMockedRegistry { mock_instance, .. },
        ..
    } = fixture;
    drop((mock_instance, root));
}

#[test]
fn workspace_loglevel_suppresses_script_echo_and_cli_overrides_it() {
    let fixture = script_fixture();
    set_loglevel(&fixture.workspace, "silent");
    pnpm(&fixture.workspace)
        .with_args(["run", "test"])
        .assert()
        .success()
        .stdout("script-output\n")
        .stderr("");
    assert_output_contains(
        pnpm(&fixture.workspace).with_args(["--loglevel=info", "run", "test"]),
        "node -e",
    );
}

#[test]
fn environment_loglevel_overrides_workspace_settings() {
    let fixture = script_fixture();
    set_loglevel(&fixture.workspace, "info");
    pnpm(&fixture.workspace)
        .with_env("PNPM_CONFIG_LOGLEVEL", "silent")
        .with_args(["run", "test"])
        .assert()
        .success()
        .stdout("script-output\n")
        .stderr("");
}

#[test]
fn npmrc_loglevel_is_ignored() {
    let fixture = script_fixture();
    fs::write(fixture.workspace.join(".npmrc"), "loglevel=silent\n").expect("write npmrc");
    assert_output_contains(pnpm(&fixture.workspace).with_args(["run", "test"]), "node -e");
}

fn assert_output_contains(mut command: Command, expected: &str) {
    let assertion = command.assert().success();
    let output = assertion.get_output();
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(printed.contains(expected), "expected {expected:?} in {printed:?}");
}

#[test]
fn configured_error_suppresses_deprecation_warnings() {
    for level in ["warn", "error"] {
        let fixture = CommandTempCwd::init().add_mocked_registry();
        fs::write(
            fixture.workspace.join("package.json"),
            serde_json::json!({
                "dependencies": { "@pnpm.e2e/deprecated": "1.0.0" },
            })
            .to_string(),
        )
        .expect("write package.json");
        set_loglevel(&fixture.workspace, level);
        let assertion = pnpm(&fixture.workspace)
            .with_arg("install")
            .assert()
            .success();
        let output = assertion.get_output();
        let printed = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert_eq!(printed.contains("deprecated"), level == "warn", "output: {printed}");
        let CommandTempCwd {
            root,
            npmrc_info: AddMockedRegistry { mock_instance, .. },
            ..
        } = fixture;
        drop((mock_instance, root));
    }
}
