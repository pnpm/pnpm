//! End-to-end test for `pacquet install --pnpr-server <url>`.
//!
//! Runs the real `pacquet` binary against a mocked fixtures registry,
//! with an in-process `pnpr` hosting the fast-path endpoints. The pnpr
//! server's own uplink is left at the default — the client sends the
//! registry it wants resolved from (the mock), so a passing test proves
//! resolution used the client-supplied registry. The client then links
//! `node_modules` from the server-produced lockfile.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{
    fs,
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::Path,
    process::Command,
    thread,
    time::Duration,
};

/// Start an in-process pnpr with the fast-path endpoints on a detached
/// thread; returns its base URL.
fn start_pnpr() -> String {
    // Persisted (not cleaned) because the detached server thread outlives
    // this function.
    let storage = tempfile::tempdir().expect("pnpr storage").keep();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind pnpr");
    // tokio's `from_std` requires the listener to be non-blocking.
    listener.set_nonblocking(true).expect("set pnpr listener non-blocking");
    let addr = listener.local_addr().expect("pnpr addr");

    thread::Builder::new()
        .name("pnpr".to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("pnpr runtime");
            runtime.block_on(async move {
                let mut config = pnpr::Config::proxy(addr, storage);
                config.public_url = format!("http://{addr}");
                let listener = tokio::net::TcpListener::from_std(listener).expect("tokio listener");
                let _ = pnpr::serve_listener(config, listener).await;
            });
        })
        .expect("spawn pnpr thread");

    wait_until_ready(addr);
    format!("http://{addr}/")
}

fn wait_until_ready(addr: SocketAddr) {
    for _ in 0..200 {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(50)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("pnpr server never became ready at {addr}");
}

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

#[test]
fn install_via_pnpr_links_node_modules() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    let pnpr_url = start_pnpr();

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    pacquet.with_arg("install").with_arg("--pnpr-server").with_arg(&pnpr_url).assert().success();

    let symlink_path = workspace.join("node_modules/@foo/no-deps");
    assert!(is_symlink_or_junction(&symlink_path).unwrap(), "direct dep should be symlinked");
    let virtual_path = workspace.join("node_modules/.pnpm/@foo+no-deps@1.0.0");
    assert!(virtual_path.exists(), "virtual store should hold the package");
    assert!(workspace.join("pnpm-lock.yaml").exists(), "pnpr should write the lockfile");
    // The client store was populated by the frozen install fetching tarballs
    // directly from the registry after pnpr returned the lockfile.
    assert!(store_dir.join("v11/index.db").exists(), "client store index should exist");

    drop((root, mock_instance));
}

#[test]
fn frozen_install_via_pnpr_verifies_the_local_lockfile_without_resolving() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let pnpr_url = start_pnpr();

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    pacquet.with_arg("install").with_arg("--pnpr-server").with_arg(&pnpr_url).assert().success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    let mut verifier = mockito::Server::new();
    let verify_mock = verifier
        .mock("POST", "/v1/verify-lockfile")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body("{\"type\":\"done\"}\n")
        .expect(1)
        .create();

    pacquet_at(&workspace)
        .with_arg("install")
        .with_arg("--frozen-lockfile")
        .with_arg("--pnpr-server")
        .with_arg(verifier.url())
        .assert()
        .success();

    verify_mock.assert();
    let symlink_path = workspace.join("node_modules/@foo/no-deps");
    assert!(is_symlink_or_junction(&symlink_path).unwrap(), "direct dep should be symlinked");

    drop((root, mock_instance));
}

#[test]
fn install_via_pnpr_lockfile_only_writes_lockfile_without_linking() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    let pnpr_url = start_pnpr();

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    pacquet
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(&pnpr_url)
        .with_arg("--lockfile-only")
        .assert()
        .success();

    // `--lockfile-only` resolves and writes the lockfile but fetches
    // nothing and links nothing.
    assert!(workspace.join("pnpm-lock.yaml").exists(), "pnpr should write the lockfile");
    assert!(!workspace.join("node_modules").exists(), "lockfile-only must not link node_modules");
    assert!(
        !store_dir.join("v11/index.db").exists(),
        "lockfile-only must not populate the client store",
    );

    drop((root, mock_instance));
}
