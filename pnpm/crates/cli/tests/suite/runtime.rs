use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path};

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
fn setting_a_project_runtime_suggests_the_explicit_global_shim() {
    let CommandTempCwd { mut pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let version = "24.0.0-rc.4";
    let _mocks = crate::install_runtimes::mock_node_release(&mut server, version);
    let pnpm_home = root.path().join("pnpm-home");
    configure_node_runtime(&root, &workspace, &server);

    let output = pacquet
        .env("PNPM_HOME", &pnpm_home)
        .env("XDG_STATE_HOME", root.path().join("state"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .args(["runtime", "set", "node", version])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("pnpm shim add node"), "stdout was:\n{stdout}");
    assert!(!pnpm_home.join("bin").join(format!("node{}", std::env::consts::EXE_SUFFIX)).exists());
}

fn configure_node_runtime(root: &tempfile::TempDir, workspace: &Path, server: &mockito::Server) {
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
}
