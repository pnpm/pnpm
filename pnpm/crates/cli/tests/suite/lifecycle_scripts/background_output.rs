use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};

/// [pnpm/pnpm#5730](https://github.com/pnpm/pnpm/issues/5730)
#[test]
fn install_finishes_while_a_prepare_script_leaves_a_process_holding_its_output() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "name": "project-with-a-background-process",
        "version": "1.0.0",
        "scripts": { "prepare": "echo prepared; sleep 30 & echo $! > background.pid" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    let started = Instant::now();
    let output = pacquet
        .with_arg("install")
        .output()
        .expect("spawn pacquet install");
    let elapsed = started.elapsed();
    let background =
        fs::read_to_string(workspace.join("background.pid")).expect("read background pid");
    Command::new("kill")
        .arg(background.trim())
        .status()
        .expect("kill background process");

    assert!(output.status.success(), "the install must succeed, got: {output:?}");
    assert!(
        elapsed < Duration::from_secs(20),
        "the install must not wait for the background process: {elapsed:?}",
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("prepare: prepared"),
        "the script's output must be reported, got: {output:?}",
    );

    drop((root, mock_instance));
}
