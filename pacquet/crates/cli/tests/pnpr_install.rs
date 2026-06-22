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
use pnpr::TokenBackend;
use std::{
    fs,
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::Path,
    process::Command,
    thread,
    time::Duration,
};

/// Start an in-process pnpr with the fast-path endpoints on a detached
/// thread; returns its base URL and a pre-seeded bearer token.
fn start_pnpr() -> (String, String) {
    // Persisted (not cleaned) because the detached server thread outlives
    // this function.
    let storage = tempfile::tempdir().expect("pnpr storage").keep();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind pnpr");
    // tokio's `from_std` requires the listener to be non-blocking.
    listener.set_nonblocking(true).expect("set pnpr listener non-blocking");
    let addr = listener.local_addr().expect("pnpr addr");
    let tokens_path = storage.join("tokens.db");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("token setup runtime");
    let token = runtime.block_on(async {
        let tokens = pnpr::TokenStore::open(tokens_path.clone()).expect("open token store");
        tokens.issue("pacquet-test").await.expect("issue pnpr test token")
    });

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
                config.auth.tokens.file = Some(tokens_path);
                let listener = tokio::net::TcpListener::from_std(listener).expect("tokio listener");
                let _ = pnpr::serve_listener(config, listener).await;
            });
        })
        .expect("spawn pnpr thread");

    wait_until_ready(addr);
    (format!("http://{addr}/"), token)
}

fn configure_pnpr_auth(npmrc_path: &std::path::Path, pnpr_url: &str, token: &str) {
    let authority =
        pnpr_url.strip_prefix("http://").expect("test pnpr URL uses http").trim_end_matches('/');
    let current = fs::read_to_string(npmrc_path).expect("read .npmrc");
    let separator = if current.ends_with('\n') { "" } else { "\n" };
    fs::write(npmrc_path, format!("{current}{separator}//{authority}/:_authToken={token}\n"))
        .expect("write pnpr auth to .npmrc");
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
    let AddMockedRegistry { npmrc_path, store_dir, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr();
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(&pnpr_url)
        .assert()
        .success();

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
fn frozen_install_via_pnpr_verifies_the_local_lockfile_without_resolving_or_redownloading() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr();
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(&pnpr_url)
        .assert()
        .success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    let mut verifier = mockito::Server::new();
    let verify_mock = verifier
        .mock("POST", "/-/pnpr/v0/verify-lockfile")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body("{\"type\":\"done\"}\n")
        .expect(1)
        .create();

    // The first install warmed the store, so the frozen restore must not
    // fetch a single tarball: point the registry at a server that rejects
    // every request. Registry resolutions derive their tarball URLs from
    // the configured registry at install time, so the swap is transparent
    // to the lockfile.
    let mut silent_registry = mockito::Server::new();
    let no_downloads = silent_registry.mock("GET", mockito::Matcher::Any).expect(0).create();
    let npmrc = fs::read_to_string(&npmrc_path)
        .expect("read .npmrc")
        .lines()
        .map(|line| {
            if line.starts_with("registry=") {
                format!("registry={}/", silent_registry.url())
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, npmrc).expect("rewrite .npmrc");

    pacquet_at(&workspace)
        .with_arg("install")
        .with_arg("--frozen-lockfile")
        .with_arg("--pnpr-server")
        .with_arg(verifier.url())
        .assert()
        .success();

    verify_mock.assert();
    no_downloads.assert();
    let symlink_path = workspace.join("node_modules/@foo/no-deps");
    assert!(is_symlink_or_junction(&symlink_path).unwrap(), "direct dep should be symlinked");

    drop((root, mock_instance));
}

#[test]
fn install_via_pnpr_lockfile_only_writes_lockfile_without_linking() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, store_dir, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr();
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(&pnpr_url)
        .with_arg("--lockfile-only")
        .assert()
        .success();

    assert!(workspace.join("pnpm-lock.yaml").exists(), "pnpr should write the lockfile");
    assert!(!workspace.join("node_modules").exists(), "lockfile-only must not link node_modules");
    assert!(
        !store_dir.join("v11/index.db").exists(),
        "lockfile-only must not populate the client store",
    );

    drop((root, mock_instance));
}
