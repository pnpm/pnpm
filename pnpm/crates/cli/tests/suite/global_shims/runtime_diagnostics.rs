use super::fs;
#[cfg(not(windows))]
use super::{Command, install_shim, isolated_in};
#[cfg(windows)]
use super::{Path, windows_shim_command};

#[test]
fn failed_runtime_download_explains_global_mirror_configuration() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new();
    let checksums = server
        .mock("GET", "/mirrors/release/v24.19.0/SHASUMS256.txt")
        .with_status(404)
        .create();
    let _index = server
        .mock("GET", "/mirrors/release/index.json")
        .with_status(404)
        .create();
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("package.json"),
        r#"{"devEngines":{"runtime":{"name":"node","version":"24.19.0","onFail":"download"}}}"#,
    )
    .unwrap();
    fs::write(
        project.join("pnpm-workspace.yaml"),
        format!("nodeDownloadMirrors:\n  release: {}/mirrors/node/\n", server.url()),
    )
    .unwrap();
    let config_dir = root.path().join("config/pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.yaml"),
        serde_json::json!({
            "tools": { "node": { "mirror": format!("{}/mirrors", server.url()) } },
            "fetchRetries": 0,
            "storeDir": root.path().join("store"),
            "cacheDir": root.path().join("cache"),
        })
        .to_string(),
    )
    .unwrap();
    #[cfg(not(windows))]
    let mut command = {
        let shim = install_shim(&root.path().join("global-bin"), "node", b"pkg:node");
        isolated_in(Command::new(shim), &root, &project)
    };
    #[cfg(windows)]
    let mut command = windows_shim_command(&root, &project, "node", Path::new("pkg:node"));
    let output = command
        .env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"node":"always"}"#)
        .arg("--version")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    checksums.assert();
    for expected in [
        "pnpm: failed to prepare node@runtime:24.19.0",
        "/mirrors/release/",
        "global configuration and environment settings",
        "pnpm-workspace.yaml is ignored",
        "tools.node",
        "nodeDownloadMirrors",
        "PNPM_CONFIG_TOOLS",
        "PNPM_CONFIG_NODE_DOWNLOAD_MIRRORS",
    ] {
        assert!(stderr.contains(expected), "missing {expected:?} in {stderr}");
    }
}
