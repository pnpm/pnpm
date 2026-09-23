use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use std::{fs, path::Path, process::Command};

fn pnpm(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

fn script_fixture(workspace_yaml: &str) -> CommandTempCwd<()> {
    let fixture = CommandTempCwd::init();
    fs::write(fixture.workspace.join("pnpm-workspace.yaml"), workspace_yaml)
        .expect("write workspace settings");
    fs::write(
        fixture.workspace.join("package.json"),
        serde_json::json!({
            "name": "reporter-test",
            "version": "1.0.0",
            "scripts": { "test": r#"node -e "console.log('script-output')""# },
        })
        .to_string(),
    )
    .expect("write package.json");
    fixture
}

fn printed_output(mut command: Command) -> String {
    let assertion = command.assert().success();
    let output = assertion.get_output();
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn workspace_reporter_silences_script_echo_and_cli_overrides_it() {
    let fixture = script_fixture("reporter: silent\n");
    pnpm(&fixture.workspace)
        .with_args(["run", "test"])
        .assert()
        .success()
        .stdout("script-output\n")
        .stderr("");

    let printed =
        printed_output(pnpm(&fixture.workspace).with_args(["--reporter=default", "run", "test"]));
    assert!(printed.contains("node -e"), "output: {printed}");

    pnpm(&fixture.workspace)
        .with_args(["config", "get", "reporter"])
        .assert()
        .success()
        .stdout("silent\n");
}

#[test]
fn environment_reporter_overrides_workspace_setting() {
    let fixture = script_fixture("reporter: default\n");
    pnpm(&fixture.workspace)
        .with_env("PNPM_CONFIG_REPORTER", "silent")
        .with_args(["run", "test"])
        .assert()
        .success()
        .stdout("script-output\n")
        .stderr("");
}

#[test]
fn global_config_reporter_applies() {
    let fixture = script_fixture("");
    let config_home = fixture.root.path().join("xdg-config");
    fs::create_dir_all(config_home.join("pnpm")).expect("create global config directory");
    fs::write(config_home.join("pnpm/config.yaml"), "reporter: silent\n")
        .expect("write global config");
    pnpm(&fixture.workspace)
        .with_env("XDG_CONFIG_HOME", &config_home)
        .with_args(["run", "test"])
        .assert()
        .success()
        .stdout("script-output\n")
        .stderr("");
}

#[test]
fn workspace_reporter_silences_pre_command_pin_warnings() {
    let pins = [
        (
            serde_json::json!({
                "packageManager": { "name": "pnpm", "version": "0.0.1", "onFail": "warn" },
            }),
            "configured to use 0.0.1 of pnpm",
        ),
        (
            serde_json::json!({
                "runtime": { "name": "node", "version": "99999.0.0", "onFail": "warn" },
            }),
            "99999.0.0",
        ),
    ];
    for (dev_engines, warning) in pins {
        let fixture = script_fixture("reporter: silent\n");
        fs::write(
            fixture.workspace.join("package.json"),
            serde_json::json!({
                "name": "reporter-test",
                "version": "1.0.0",
                "scripts": { "test": r#"node -e "console.log('script-output')""# },
                "devEngines": dev_engines,
            })
            .to_string(),
        )
        .expect("write package.json");
        pnpm(&fixture.workspace)
            .with_env("PNPM_SHIM_BYPASS", "1")
            .with_args(["run", "test"])
            .assert()
            .success()
            .stdout("script-output\n")
            .stderr("");

        let printed = printed_output(
            pnpm(&fixture.workspace)
                .with_env("PNPM_SHIM_BYPASS", "1")
                .with_args(["--reporter=default", "run", "test"]),
        );
        assert!(printed.contains(warning), "output: {printed}");
    }
}

#[test]
fn workspace_reporter_silences_fresh_and_repeat_installs() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let workspace = &fixture.workspace;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "reporter-test",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read workspace settings");
    yaml.push_str("\nreporter: silent\n");
    fs::write(&yaml_path, yaml).expect("write workspace settings");

    for _ in 0..2 {
        pnpm(workspace)
            .with_arg("install")
            .assert()
            .success()
            .stdout("")
            .stderr("");
    }
    assert!(workspace.join("node_modules/@pnpm.e2e/foo/package.json").exists());

    let printed = printed_output(pnpm(workspace).with_args(["--reporter=default", "install"]));
    assert!(printed.contains("Already up to date"), "output: {printed}");

    let CommandTempCwd {
        root,
        npmrc_info: AddMockedRegistry { mock_instance, .. },
        ..
    } = fixture;
    drop((mock_instance, root));
}
