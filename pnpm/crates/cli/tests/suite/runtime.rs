use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, process::Command};

#[test]
fn runtime_unknown_subcommand_runs_with_default_ndjson_and_silent_reporters() {
    for reporter in [None, Some("--reporter=ndjson"), Some("--reporter=silent")] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        fs::write(workspace.join("package.json"), "{}").expect("write package.json");

        let mut command = pacquet;
        if let Some(reporter) = reporter {
            command.arg(reporter);
        }
        command.arg("runtime").arg("unknown");
        let output = command.output().expect("spawn pacquet runtime");
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success(), "unknown runtime subcommand must fail");
        assert!(stderr.contains("ERR_PNPM_RUNTIME_UNKNOWN_SUBCOMMAND"), "stderr: {stderr}");

        drop(root);
    }
}

#[test]
fn setting_a_project_runtime_creates_a_project_aware_global_bin() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let version = "24.0.0-rc.4";
    let _mocks = crate::install_runtimes::mock_node_release(&mut server, version);
    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let config_dir = root.path().join("config").join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.yaml"),
        format!(
            "storeDir: {}\ncacheDir: {}\nnodeDownloadMirrors:\n  rc: '{}/'\n",
            root.path().join("store").display(),
            root.path().join("cache").display(),
            server.url(),
        ),
    )
    .unwrap();
    fs::write(workspace.join("package.json"), "{}").unwrap();

    pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .with_args(["runtime", "set", "node", version])
        .assert()
        .success();

    let shim = fs::read_to_string(global_bin.join("node")).expect("read the Node.js shim");
    assert!(shim.contains("cmd-shim-target=pkg:node"), "shim was:\n{shim}");
    assert!(global_bin.join(".pnpm-shim-v1").is_file());

    #[cfg(unix)]
    Command::new(global_bin.join("node"))
        .with_current_dir(&workspace)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .with_env("PNPM_AUTO_APPROVE_PROJECT_BINS_FOR_TESTS", "1")
        .assert()
        .success();
}
