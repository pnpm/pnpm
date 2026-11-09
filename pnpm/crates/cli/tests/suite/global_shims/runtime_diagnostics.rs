use super::{Command, fs, install_shim, isolated_in};

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
    let shim = install_shim(&root.path().join("global-bin"), "node", b"pkg:node");
    let output = isolated_in(Command::new(shim), &root, &project)
        .arg("--version")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    checksums.assert();
    let stderr = String::from_utf8_lossy(&output.stderr);
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
