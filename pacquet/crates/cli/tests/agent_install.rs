//! End-to-end test for `pacquet install --agent <url>`.
//!
//! Runs the real `pacquet` binary against a mocked fixtures registry,
//! with an in-process `pnpr` agent (resolving from that registry)
//! hosting the `/v1/install` + `/v1/files` endpoints. Proves the CLI
//! routes through the agent client and then links `node_modules` from
//! the agent-produced lockfile.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{
    fs,
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    thread,
    time::Duration,
};

/// Start an in-process pnpr with the agent endpoints, resolving from
/// `upstream`. The server runs on a detached thread for the rest of the
/// process; returns its base URL.
fn start_agent(upstream: &str) -> String {
    // Persisted (not cleaned) because the detached server thread outlives
    // this function; a test process leaves it in the OS temp dir.
    let storage = tempfile::tempdir().expect("agent storage").keep();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind agent");
    // tokio's `from_std` requires the listener to be non-blocking.
    listener.set_nonblocking(true).expect("set agent listener non-blocking");
    let addr = listener.local_addr().expect("agent addr");
    let upstream = upstream.trim_end_matches('/').to_string();

    thread::Builder::new()
        .name("pnpr-agent".to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("agent runtime");
            runtime.block_on(async move {
                let mut config = pnpr::Config::proxy(addr, storage);
                // The agent derives its resolve registry from the first uplink.
                config.uplinks.get_mut("npmjs").expect("npmjs uplink").url = upstream;
                config.public_url = format!("http://{addr}");
                let listener = tokio::net::TcpListener::from_std(listener).expect("tokio listener");
                let _ = pnpr::serve_listener(config, listener).await;
            });
        })
        .expect("spawn agent thread");

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
    panic!("agent server never became ready at {addr}");
}

#[test]
fn install_via_agent_links_node_modules() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    let agent_url = start_agent(&mock_instance.url());

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    pacquet.with_arg("install").with_arg("--agent").with_arg(&agent_url).assert().success();

    let symlink_path = workspace.join("node_modules/@foo/no-deps");
    assert!(is_symlink_or_junction(&symlink_path).unwrap(), "direct dep should be symlinked");
    let virtual_path = workspace.join("node_modules/.pnpm/@foo+no-deps@1.0.0");
    assert!(virtual_path.exists(), "virtual store should hold the package");
    assert!(workspace.join("pnpm-lock.yaml").exists(), "agent should write the lockfile");
    // The client store was populated by the agent's `/v1/files` downloads.
    assert!(store_dir.join("v11/index.db").exists(), "client store index should exist");

    drop((root, mock_instance));
}
