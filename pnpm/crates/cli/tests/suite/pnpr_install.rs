//! End-to-end test for `pacquet install --pnpr-server <url>`.
//!
//! Runs the real `pacquet` binary against a mocked fixtures registry,
//! with an in-process `pnpr` hosting the fast-path endpoints. The pnpr
//! server's own upstream is left at the default; the client sends the
//! registry it wants resolved from (the mock, which the server allowlists
//! as a public route), so a passing test proves resolution used the
//! client-supplied registry. The client then links `node_modules` from the
//! server-produced lockfile.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_crypto_hash::integrity_addressed_tarball_path;
use pnpm_lockfile::{Lockfile, PkgName, ProjectSnapshot, SnapshotEntry};
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::{get_all_files, is_symlink_or_junction},
};
use pnpr::TokenBackend;
use std::{
    fmt::Write as _,
    fs,
    io::Write as _,
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::Path,
    process::Command,
    thread,
    time::Duration,
};
use text_block_macros::text_block_fnl;

const IS_POSITIVE_PATCH: &str = include_str!(
    "../../../../../pnpm11/installing/deps-installer/test/fixtures/patch-pkg/is-positive@1.0.0.patch"
);

/// Start an in-process pnpr with the fast-path endpoints on a detached
/// thread, allowlisting `registry_url` as a public route so the client may
/// resolve against it (off-allowlist registries are rejected at the request
/// boundary); returns its base URL and a pre-seeded bearer token.
fn start_pnpr(registry_url: &str) -> (String, String) {
    let registry_url = registry_url.to_string();
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
                config
                    .route_policy
                    .public
                    .push(pnpr::PublicRoute { registry: Some(registry_url), package: None });
                let listener = tokio::net::TcpListener::from_std(listener).expect("tokio listener");
                let _ = pnpr::serve_listener(config, listener).await;
            });
        })
        .expect("spawn pnpr thread");

    wait_until_ready(addr);
    (format!("http://{addr}/"), token)
}

fn start_pnpr_registry(upstream_url: &str) -> String {
    let upstream_url = upstream_url.to_string();
    let storage = tempfile::tempdir().expect("pnpr storage").keep();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind pnpr");
    listener.set_nonblocking(true).expect("set pnpr listener non-blocking");
    let addr = listener.local_addr().expect("pnpr addr");

    thread::Builder::new()
        .name("pnpr-registry".to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("pnpr runtime");
            runtime.block_on(async move {
                let mut config = pnpr::Config::proxy(addr, storage);
                config.public_url = format!("http://{addr}");
                config.upstreams.get_mut("npmjs").unwrap().url = upstream_url;
                config.registries = pnpr::Registries::new(
                    std::iter::once((
                        "npmjs".to_string(),
                        pnpr::Registry::Upstream { patterns: Vec::new() },
                    ))
                    .collect(),
                    Some("npmjs".to_string()),
                );
                let listener = tokio::net::TcpListener::from_std(listener).expect("tokio listener");
                let _ = pnpr::serve_listener(config, listener).await;
            });
        })
        .expect("spawn pnpr registry thread");

    wait_until_ready(addr);
    format!("http://{addr}")
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
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// Rewrite the `.npmrc` `registry=` line. Registry resolutions derive
/// their tarball URLs from the configured registry at install time, so
/// the swap is transparent to an existing lockfile.
fn point_npmrc_registry_at(npmrc_path: &Path, registry_url: &str) {
    let npmrc = fs::read_to_string(npmrc_path)
        .expect("read .npmrc")
        .lines()
        .map(|line| {
            if line.starts_with("registry=") {
                format!("registry={registry_url}/")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(npmrc_path, npmrc).expect("rewrite .npmrc");
}

fn revision_fixture_tarball() -> Vec<u8> {
    revision_fixture_tarball_with_value("revision")
}

fn revision_fixture_tarball_with_value(value: &str) -> Vec<u8> {
    let manifest = br#"{"name":"revision-pkg","version":"1.0.0","main":"index.js"}"#;
    let source = format!("module.exports = '{value}'\n");
    let mut tar = tar::Builder::new(Vec::new());
    for (path, body) in
        [("package/package.json", manifest.as_slice()), ("package/index.js", source.as_bytes())]
    {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, path, body).expect("append package file");
    }
    let tar = tar.into_inner().expect("finish package tar");
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&tar).expect("compress package tar");
    gzip.finish().expect("finish package tarball")
}

fn revision_packument(
    upstream: &mockito::Server,
    tarball: &[u8],
    revision: u64,
    history: &[(&ssri::Integrity, u64)],
) -> (ssri::Integrity, serde_json::Value) {
    let integrity =
        ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512).chain(tarball).result();
    let revision_path = integrity_addressed_tarball_path(&integrity).unwrap();
    let revisions = history
        .iter()
        .map(|(integrity, revision)| {
            let path = integrity_addressed_tarball_path(integrity).unwrap();
            serde_json::json!({
                "revision": revision,
                "integrity": integrity.to_string(),
                "tarball": format!("{}/{}", upstream.url(), path),
                "manifest": {},
            })
        })
        .chain(std::iter::once(serde_json::json!({
            "revision": revision,
            "integrity": integrity.to_string(),
            "tarball": format!("{}/{}", upstream.url(), revision_path),
            "manifest": {},
        })))
        .collect::<Vec<_>>();
    let packument = serde_json::json!({
        "name": "revision-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": {
            "name": "revision-pkg",
            "version": "1.0.0",
            "dist": {
                "tarball": format!("{}/{}", upstream.url(), revision_path),
                "integrity": integrity.to_string(),
                "revision": revision,
                "revisions": revisions,
            },
        } },
    });
    (integrity, packument)
}

#[test]
fn revision_install_and_frozen_reinstall_work_through_pnpr() {
    let mut upstream = mockito::Server::new();
    let tarball = revision_fixture_tarball();
    let (integrity, packument) = revision_packument(&upstream, &tarball, 2, &[]);
    let revision_path = integrity_addressed_tarball_path(&integrity).unwrap();
    let packument_mock =
        upstream.mock("GET", "/revision-pkg").with_body(packument.to_string()).expect(1).create();
    let tarball_mock = upstream
        .mock("GET", format!("/{revision_path}").as_str())
        .with_body(&tarball)
        .expect(1)
        .create();
    let registry = start_pnpr_registry(&upstream.url());

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, store_dir, mock_instance, .. } = npmrc_info;
    point_npmrc_registry_at(&npmrc_path, &registry);
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "revision-pkg": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();
    assert!(workspace.join("node_modules/revision-pkg/index.js").exists());
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.contains("revision: 2"), "{lockfile}");
    assert!(!lockfile.contains("tarball:"), "{lockfile}");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    fs::remove_dir_all(&store_dir).expect("remove client store");
    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert!(workspace.join("node_modules/revision-pkg/index.js").exists());

    packument_mock.assert();
    tarball_mock.assert();
    drop((root, mock_instance));
}

#[test]
fn update_patches_refreshes_a_pnpr_revision_without_changing_the_version() {
    let first_tarball = revision_fixture_tarball_with_value("revision one");
    let second_tarball = revision_fixture_tarball_with_value("revision two");

    let mut first_upstream = mockito::Server::new();
    let (first_integrity, first_packument) =
        revision_packument(&first_upstream, &first_tarball, 1, &[]);
    let first_path = integrity_addressed_tarball_path(&first_integrity).unwrap();
    first_upstream.mock("GET", "/revision-pkg").with_body(first_packument.to_string()).create();
    first_upstream
        .mock("GET", format!("/{first_path}").as_str())
        .with_body(&first_tarball)
        .expect(1)
        .create();
    let first_registry = start_pnpr_registry(&first_upstream.url());

    let mut second_upstream = mockito::Server::new();
    let (second_integrity, second_packument) =
        revision_packument(&second_upstream, &second_tarball, 2, &[(&first_integrity, 1)]);
    let second_path = integrity_addressed_tarball_path(&second_integrity).unwrap();
    second_upstream.mock("GET", "/revision-pkg").with_body(second_packument.to_string()).create();
    second_upstream
        .mock("GET", format!("/{second_path}").as_str())
        .with_body(&second_tarball)
        .expect(1)
        .create();
    let second_registry = start_pnpr_registry(&second_upstream.url());

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    point_npmrc_registry_at(&npmrc_path, &first_registry);
    let manifest = serde_json::json!({ "dependencies": { "revision-pkg": "^1.0.0" } });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");

    pacquet.with_arg("install").assert().success();
    let initial = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(initial.contains("revision: 1"), "{initial}");

    point_npmrc_registry_at(&npmrc_path, &second_registry);
    pacquet_at(&workspace).with_args(["update", "--patches"]).assert().success();

    let refreshed = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(refreshed.contains("revision: 2"), "{refreshed}");
    assert!(!refreshed.contains("revision: 1"), "{refreshed}");
    assert!(refreshed.contains("revision-pkg@1.0.0"), "{refreshed}");
    let saved_manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    assert_eq!(saved_manifest, manifest);
    assert_eq!(
        fs::read_to_string(workspace.join("node_modules/revision-pkg/index.js"))
            .expect("read installed source"),
        "module.exports = 'revision two'\n",
    );

    drop((root, mock_instance));
}

#[test]
fn update_patches_refreshes_a_revision_through_the_pnpr_resolver() {
    let first_tarball = revision_fixture_tarball_with_value("revision one");
    let second_tarball = revision_fixture_tarball_with_value("revision two");
    let mut upstream = mockito::Server::new();
    let (first_integrity, first_packument) = revision_packument(&upstream, &first_tarball, 1, &[]);
    let first_path = integrity_addressed_tarball_path(&first_integrity).unwrap();
    let first_packument_mock = upstream
        .mock("GET", "/revision-pkg")
        .with_body(first_packument.to_string())
        .expect_at_least(1)
        .create();
    let first_tarball_mock = upstream
        .mock("GET", format!("/{first_path}").as_str())
        .with_body(&first_tarball)
        .expect(1)
        .create();
    let (pnpr_url, token) = start_pnpr(&upstream.url());

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);
    let manifest = serde_json::json!({ "dependencies": { "revision-pkg": "^1.0.0" } });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", upstream.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();
    first_packument_mock.assert();
    first_tarball_mock.assert();
    first_packument_mock.remove();

    let (second_integrity, second_packument) =
        revision_packument(&upstream, &second_tarball, 2, &[(&first_integrity, 1)]);
    let second_path = integrity_addressed_tarball_path(&second_integrity).unwrap();
    let second_packument_mock = upstream
        .mock("GET", "/revision-pkg")
        .with_body(second_packument.to_string())
        .expect_at_least(1)
        .create();
    let second_tarball_mock = upstream
        .mock("GET", format!("/{second_path}").as_str())
        .with_body(&second_tarball)
        .expect(1)
        .create();

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", upstream.url())
        .with_args(["update", "--patches", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    second_packument_mock.assert();
    second_tarball_mock.assert();
    let refreshed = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(refreshed.contains("revision: 2"), "{refreshed}");
    assert!(!refreshed.contains("revision: 1"), "{refreshed}");
    assert!(refreshed.contains("revision-pkg@1.0.0"), "{refreshed}");
    let saved_manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    assert_eq!(saved_manifest, manifest);
    assert_eq!(
        fs::read_to_string(workspace.join("node_modules/revision-pkg/index.js"))
            .expect("read installed source"),
        "module.exports = 'revision two'\n",
    );

    drop((root, mock_instance));
}

#[test]
fn install_via_pnpr_links_node_modules() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, store_dir, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
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
fn install_via_pnpr_replaces_a_conflicted_lockfile() {
    const CONFLICTED_LOCKFILE: &str = text_block_fnl! {
        "<<<<<<< HEAD"
        "lockfileVersion: '9.0'"
        "======="
        "lockfileVersion: '9.0'"
        ">>>>>>> branch"
    };

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@foo/no-deps": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    fs::write(workspace.join("pnpm-lock.yaml"), CONFLICTED_LOCKFILE)
        .expect("write conflicted lockfile");

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let lockfile = read_workspace_lockfile(&workspace);
    assert_eq!(workspace_importer_version(&lockfile, ".", "@foo/no-deps"), "1.0.0");
    assert!(is_symlink_or_junction(&workspace.join("node_modules/@foo/no-deps")).unwrap());

    fs::write(workspace.join("pnpm-lock.yaml"), CONFLICTED_LOCKFILE)
        .expect("rewrite conflicted lockfile");
    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--fix-lockfile", "--pnpr-server", &pnpr_url])
        .assert()
        .success();
    let repaired = read_workspace_lockfile(&workspace);
    assert_eq!(workspace_importer_version(&repaired, ".", "@foo/no-deps"), "1.0.0");

    drop((root, mock_instance));
}

#[test]
fn patched_dependencies_resolve_via_pnpr() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(workspace.join("patches/is-positive@1.0.0.patch"), IS_POSITIVE_PATCH)
        .expect("write patch file");
    crate::_utils::append_workspace_yaml_key(
        &workspace,
        "patchedDependencies",
        "{ 'is-positive@1.0.0': patches/is-positive@1.0.0.patch }",
    );

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let installed = fs::read_to_string(workspace.join("node_modules/is-positive/index.js"))
        .expect("read installed package");
    eprintln!("INSTALLED SOURCE:\n{installed}\n");
    assert!(installed.contains("// patched"));
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    eprintln!("LOCKFILE:\n{lockfile}\n");
    assert!(lockfile.contains("patchedDependencies:"));
    assert!(lockfile.contains("patch_hash="));

    drop((root, mock_instance));
}

#[test]
fn package_extensions_resolve_via_pnpr() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    crate::_utils::append_workspace_yaml_key(
        &workspace,
        "packageExtensions",
        "{ 'is-positive@1.0.0': { dependencies: { is-negative: 1.0.0 } } }",
    );

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    eprintln!("LOCKFILE:\n{lockfile}\n");
    assert!(lockfile.contains("packageExtensionsChecksum:"));
    assert!(lockfile.contains("is-negative: 1.0.0"));
    assert!(
        workspace.join("node_modules/.pnpm/is-positive@1.0.0/node_modules/is-negative").exists(),
    );

    drop((root, mock_instance));
}

/// A pnpr-resolved lockfile is rewritten wholesale from the server's
/// answer, so `time:` has to survive the round trip the same way a
/// locally resolved install preserves it.
#[test]
fn install_via_pnpr_preserves_the_lockfiles_time_section() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let manifest_path = workspace.join("package.json");
    let write_manifest = |dependencies: serde_json::Value| {
        fs::write(&manifest_path, serde_json::json!({ "dependencies": dependencies }).to_string())
            .expect("write package.json");
    };
    let install_via_pnpr = || {
        pacquet_at(&workspace)
            .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
            .with_arg("install")
            .with_arg("--pnpr-server")
            .with_arg(&pnpr_url)
            .assert()
            .success();
    };

    write_manifest(serde_json::json!({ "@foo/has-dep-from-same-scope": "1.0.0" }));
    install_via_pnpr();

    // Appended as text: saving would prune the transitive entry before
    // the install under test ever sees it.
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let mut recorded = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    recorded.push_str(
        "\ntime:\n  '@foo/has-dep-from-same-scope@1.0.0': '2024-01-01T00:00:00.000Z'\n  '@foo/no-deps@1.0.0': '2024-01-02T00:00:00.000Z'\n",
    );
    fs::write(&lockfile_path, recorded).expect("record publish dates in pnpm-lock.yaml");

    // A new dependency is what sends the second install back through the
    // server rather than short-circuiting on the lockfile it just wrote.
    write_manifest(
        serde_json::json!({ "@foo/has-dep-from-same-scope": "1.0.0", "is-positive": "1.0.0" }),
    );
    install_via_pnpr();

    let time = read_workspace_lockfile(&workspace).time.expect("`time:` survives a pnpr install");
    assert_eq!(
        time.into_iter().collect::<Vec<_>>(),
        [(
            "@foo/has-dep-from-same-scope@1.0.0".to_string(),
            "2024-01-01T00:00:00.000Z".to_string(),
        )],
    );

    drop((root, mock_instance));
}

#[test]
fn frozen_install_via_pnpr_verifies_the_local_lockfile_without_resolving_or_redownloading() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, cache_dir, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
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
    // The first install recorded its lockfile as verified; wipe that
    // cache so this test still exercises the server-delegated
    // verification path instead of the cache short-circuit.
    fs::remove_dir_all(&cache_dir).expect("wipe the client cache dir");

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
    // every request.
    let mut silent_registry = mockito::Server::new();
    let no_downloads = silent_registry.mock("GET", mockito::Matcher::Any).expect(0).create();
    point_npmrc_registry_at(&npmrc_path, &silent_registry.url());

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

/// The up-to-date verdict is decided purely locally
/// ([pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904)).
#[test]
fn repeat_install_via_pnpr_short_circuits_without_contacting_the_server() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
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

    let mut silent_pnpr = mockito::Server::new();
    let no_pnpr_requests = silent_pnpr.mock("POST", mockito::Matcher::Any).expect(0).create();

    let assert = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(silent_pnpr.url())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        stdout.contains("Already up to date"),
        "expected the up-to-date fast path's output; got:\n{stdout}",
    );
    no_pnpr_requests.assert();

    drop((root, mock_instance));
}

/// Zero exchanges, not one: the verification round trip is covered by
/// the record the previous install left in the local verification cache
/// ([pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904)).
#[test]
fn install_via_pnpr_skips_the_server_when_the_lockfile_satisfies_the_manifest() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
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

    let mut silent_pnpr = mockito::Server::new();
    let no_pnpr_requests = silent_pnpr.mock("POST", mockito::Matcher::Any).expect(0).create();
    // The warm store must serve every tarball; reject any registry fetch.
    let mut silent_registry = mockito::Server::new();
    let no_downloads = silent_registry.mock("GET", mockito::Matcher::Any).expect(0).create();
    point_npmrc_registry_at(&npmrc_path, &silent_registry.url());

    pacquet_at(&workspace)
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(silent_pnpr.url())
        .assert()
        .success();

    no_pnpr_requests.assert();
    no_downloads.assert();
    let symlink_path = workspace.join("node_modules/@foo/no-deps");
    assert!(is_symlink_or_junction(&symlink_path).unwrap(), "direct dep should be symlinked");

    drop((root, mock_instance));
}

/// The satisfaction check skips only the *resolve* exchange on its own
/// ([pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904)).
#[test]
fn satisfied_install_via_pnpr_delegates_verification_when_the_cache_is_cold() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, cache_dir, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
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
    fs::remove_dir_all(&cache_dir).expect("wipe the client cache dir");

    let mut verifier = mockito::Server::new();
    let verify_mock = verifier
        .mock("POST", "/-/pnpr/v0/verify-lockfile")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body("{\"type\":\"done\"}\n")
        .expect(1)
        .create();
    let no_resolve = verifier.mock("POST", "/-/pnpr/v0/resolve").expect(0).create();

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(verifier.url())
        .assert()
        .success();

    verify_mock.assert();
    no_resolve.assert();
    let symlink_path = workspace.join("node_modules/@foo/no-deps");
    assert!(is_symlink_or_junction(&symlink_path).unwrap(), "direct dep should be symlinked");

    drop((root, mock_instance));
}

#[test]
fn install_via_pnpr_lockfile_only_writes_lockfile_without_linking() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, store_dir, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
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

/// `pnpm import` has to control the version each dependency resolves to,
/// which the pnpr protocol cannot yet express, so it resolves locally and
/// says so rather than silently ignoring the server.
#[test]
fn import_ignores_the_pnpr_server_and_resolves_locally() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, store_dir, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    fs::write(
        workspace.join("package-lock.json"),
        serde_json::json!({
            "lockfileVersion": 1,
            "dependencies": {
                "@foo/no-deps": { "version": "1.0.0" },
            },
        })
        .to_string(),
    )
    .expect("write package-lock.json");

    let output = pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("import")
        .with_arg("--pnpr-server")
        .with_arg(&pnpr_url)
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("the pnpr server at {pnpr_url} is not used")),
        "import must say the pnpr server was skipped:\n{stdout}",
    );
    assert!(workspace.join("pnpm-lock.yaml").exists(), "import must write the lockfile");
    assert!(!workspace.join("node_modules").exists(), "import must not link node_modules");
    // The store writer task always creates an empty `v11/index.db`, so the
    // absence of fetched package content is what says nothing was downloaded.
    let cas_blobs: Vec<String> = get_all_files(&store_dir)
        .into_iter()
        .filter(|path| {
            Path::new(path).components().any(|component| component.as_os_str() == "files")
        })
        .collect();
    assert!(cas_blobs.is_empty(), "import must not fetch package content: {cas_blobs:?}");

    drop((root, mock_instance));
}

const WORKSPACE_DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const WORKSPACE_HELLO: &str = "@pnpm.e2e/hello-world-js-bin";
const WORKSPACE_HELLO_PARENT: &str = "@pnpm.e2e/hello-world-js-bin-parent";
const WORKSPACE_PARENT: &str = "@pnpm.e2e/pkg-with-1-dep";
const WORKSPACE_ROOT_DEP: &str = "@foo/no-deps";
const MISSING_PEERS_PARENT: &str = "@pnpm.e2e/abc-parent-with-missing-peers";

fn configure_workspace(workspace: &Path) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(path, yaml).expect("write pnpm-workspace.yaml");
}

fn write_workspace_project(workspace: &Path, dir: &str, name: &str, dependency: (&str, &str)) {
    let project = workspace.join("packages").join(dir);
    fs::create_dir_all(&project).expect("create workspace project");
    fs::write(
        project.join("package.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "name": name,
            "version": "1.0.0",
            "private": true,
            "dependencies": { dependency.0: dependency.1 },
        }))
        .expect("serialize package.json"),
    )
    .expect("write package.json");
}

fn replace_workspace_dependency(workspace: &Path, dir: &str, dependency: (&str, &str)) {
    let path = workspace.join("packages").join(dir).join("package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("read package.json"))
            .expect("parse package.json");
    manifest["dependencies"] = serde_json::json!({ dependency.0: dependency.1 });
    fs::write(path, serde_json::to_string_pretty(&manifest).expect("serialize package.json"))
        .expect("write package.json");
}

fn read_workspace_lockfile(workspace: &Path) -> Lockfile {
    let path = workspace.join("pnpm-lock.yaml");
    let contents = fs::read_to_string(&path).expect("read pnpm-lock.yaml");
    serde_saphyr::from_str(&contents)
        .unwrap_or_else(|error| panic!("parse {}: {error}\n{contents}", path.display()))
}

fn read_workspace_current_lockfile(workspace: &Path) -> Lockfile {
    let path = workspace.join("node_modules/.pnpm/lock.yaml");
    let contents = fs::read_to_string(&path).expect("read current lockfile");
    serde_saphyr::from_str(&contents)
        .unwrap_or_else(|error| panic!("parse {}: {error}\n{contents}", path.display()))
}

fn workspace_importer<'a>(lockfile: &'a Lockfile, id: &str) -> &'a ProjectSnapshot {
    lockfile
        .importers
        .get(id)
        .unwrap_or_else(|| panic!("missing importer {id}: {:?}", lockfile.importers.keys()))
}

fn workspace_importer_version(lockfile: &Lockfile, id: &str, dependency: &str) -> String {
    let name: PkgName = dependency.parse().expect("parse package name");
    workspace_importer(lockfile, id)
        .dependencies
        .as_ref()
        .and_then(|dependencies| dependencies.get(&name))
        .unwrap_or_else(|| panic!("missing {dependency} from importer {id}"))
        .version
        .to_string()
}

fn workspace_snapshot_entries(lockfile: &Lockfile, name: &str) -> Vec<(String, SnapshotEntry)> {
    lockfile
        .snapshots
        .as_ref()
        .into_iter()
        .flatten()
        .filter(|(key, _)| key.to_string().starts_with(&format!("{name}@")))
        .map(|(key, entry)| (key.to_string(), entry.clone()))
        .collect()
}

fn workspace_has_link(workspace: &Path, project: &str, dependency: &str) -> bool {
    is_symlink_or_junction(
        &workspace.join("packages").join(project).join("node_modules").join(dependency),
    )
    .unwrap_or(false)
}

fn workspace_slot(workspace: &Path, dependency: &str, version: &str) -> std::path::PathBuf {
    workspace.join("node_modules/.pnpm").join(format!("{}@{version}", dependency.replace('/', "+")))
}

fn assert_standard_workspace_pnpr_from(project: Option<&str>) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    write_workspace_project(&workspace, "app", "app", (WORKSPACE_HELLO, "1.0.0"));
    write_workspace_project(&workspace, "lib", "lib", (WORKSPACE_PARENT, "100.0.0"));
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let cwd = project.map_or_else(|| workspace.clone(), |project| workspace.join(project));
    pacquet_at(&cwd)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let wanted = read_workspace_lockfile(&workspace);
    assert_eq!(
        wanted.importers.keys().cloned().collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["packages/app".to_string(), "packages/lib".to_string(),]),
    );
    assert!(workspace_has_link(&workspace, "app", WORKSPACE_HELLO));
    assert!(workspace_has_link(&workspace, "lib", WORKSPACE_PARENT));

    drop((root, mock_instance));
}

/// The workspace the server reconstructs from a resolve request has no
/// catalog sections of its own, so an unsent catalog leaves every
/// `catalog:` specifier unresolvable
/// ([pnpm/pnpm#13232](https://github.com/pnpm/pnpm/issues/13232)).
#[test]
fn workspace_install_via_pnpr_resolves_catalog_references() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
    writeln!(yaml, "catalog:\n  '{WORKSPACE_HELLO}': 1.0.0").expect("append the catalog");
    fs::write(&path, yaml).expect("write pnpm-workspace.yaml");
    write_workspace_project(&workspace, "app", "app", (WORKSPACE_HELLO, "catalog:"));
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let wanted = read_workspace_lockfile(&workspace);
    assert_eq!(workspace_importer_version(&wanted, "packages/app", WORKSPACE_HELLO), "1.0.0");
    assert!(workspace_has_link(&workspace, "app", WORKSPACE_HELLO));

    drop((root, mock_instance));
}

/// The importer ids the server request carries, and the ones the
/// filtered-lockfile merge keys on, are relative to the lockfile — which
/// `lockfileDir` can pin outside the workspace. Deriving them from the
/// workspace root instead leaves the two sides naming different projects.
#[test]
fn workspace_install_via_pnpr_names_importers_relative_to_a_pinned_lockfile_dir() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    crate::_utils::append_workspace_yaml_key(&workspace, "lockfileDir", "..");
    write_workspace_project(&workspace, "app", "app", (WORKSPACE_HELLO, "1.0.0"));
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let wanted = read_workspace_lockfile(root.path());
    assert_eq!(
        wanted.importers.keys().cloned().collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["workspace/packages/app".to_string()]),
    );
    assert!(workspace_has_link(&workspace, "app", WORKSPACE_HELLO));

    drop(mock_instance);
}

#[test]
fn standard_workspace_install_via_pnpr_from_root_resolves_every_real_importer() {
    assert_standard_workspace_pnpr_from(None);
}

#[test]
fn standard_workspace_install_via_pnpr_from_member_resolves_every_real_importer() {
    assert_standard_workspace_pnpr_from(Some("packages/app"));
}

#[test]
fn workspace_pnpr_install_uses_current_resolver_settings_and_frozen_replays_them_from_member() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    write_workspace_project(&workspace, "app", "app", (WORKSPACE_HELLO, "1.0.0"));
    write_workspace_project(&workspace, "lib", "lib", (WORKSPACE_PARENT, "100.0.0"));
    let resolver_settings = |lockfile: &Lockfile| {
        let settings = lockfile.settings.as_ref().expect("lockfile settings");
        (settings.auto_install_peers, settings.dedupe_peers, settings.exclude_links_from_lockfile)
    };
    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_env("PNPM_CONFIG_AUTO_INSTALL_PEERS", "true")
        .with_env("PNPM_CONFIG_DEDUPE_PEERS", "false")
        .with_env("PNPM_CONFIG_EXCLUDE_LINKS_FROM_LOCKFILE", "true")
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let stale = read_workspace_lockfile(&workspace);
    assert_eq!(resolver_settings(&stale), (true, None, true));
    replace_workspace_dependency(&workspace, "app", (MISSING_PEERS_PARENT, "1.0.0"));
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_env("PNPM_CONFIG_AUTO_INSTALL_PEERS", "false")
        .with_env("PNPM_CONFIG_DEDUPE_PEERS", "true")
        .with_env("PNPM_CONFIG_EXCLUDE_LINKS_FROM_LOCKFILE", "false")
        .with_args([
            "install",
            "--no-prefer-frozen-lockfile",
            "--lockfile-only",
            "--pnpr-server",
            &pnpr_url,
        ])
        .assert()
        .success();
    let updated = read_workspace_lockfile(&workspace);
    assert_eq!(resolver_settings(&updated), (false, Some(true), false));
    for peer in ["@pnpm.e2e/peer-a", "@pnpm.e2e/peer-b", "@pnpm.e2e/peer-c"] {
        let snapshots = workspace_snapshot_entries(&updated, peer);
        assert!(snapshots.is_empty(), "autoInstallPeers=false must omit {peer}, got {snapshots:?}");
    }
    assert_eq!(workspace_importer_version(&updated, "packages/app", MISSING_PEERS_PARENT), "1.0.0");
    let before_frozen = fs::read(workspace.join("pnpm-lock.yaml")).expect("read updated lockfile");

    pacquet_at(&workspace.join("packages/app"))
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_env("PNPM_CONFIG_AUTO_INSTALL_PEERS", "false")
        .with_env("PNPM_CONFIG_DEDUPE_PEERS", "true")
        .with_env("PNPM_CONFIG_EXCLUDE_LINKS_FROM_LOCKFILE", "false")
        .with_args(["install", "--frozen-lockfile", "--lockfile-only", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    assert_eq!(
        fs::read(workspace.join("pnpm-lock.yaml")).expect("read frozen lockfile"),
        before_frozen,
    );
    assert!(!workspace.join("node_modules").exists());
    assert!(!workspace.join("packages/app/node_modules").exists());
    assert!(!workspace.join("packages/lib/node_modules").exists());

    drop((root, mock_instance));
}

fn assert_filtered_workspace_pnpr(lockfile_only: bool) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, store_dir, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "workspace-root",
            "version": "1.0.0",
            "private": true,
        })
        .to_string(),
    )
    .expect("write workspace root manifest");
    write_workspace_project(&workspace, "selected", "selected", (WORKSPACE_HELLO, "0.0.0"));
    write_workspace_project(&workspace, "unselected", "unselected", (WORKSPACE_PARENT, "100.0.0"));
    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let before = read_workspace_lockfile(&workspace);
    let prior_unselected = workspace_importer(&before, "packages/unselected").clone();
    let prior_parent = workspace_snapshot_entries(&before, WORKSPACE_PARENT);
    let prior_child = workspace_snapshot_entries(&before, WORKSPACE_DEP);
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "workspace-root",
            "version": "1.0.0",
            "private": true,
            "dependencies": { WORKSPACE_ROOT_DEP: "1.0.0" },
        })
        .to_string(),
    )
    .expect("add workspace root dependency");
    let root_manifest = fs::read(workspace.join("package.json")).expect("read root manifest");
    replace_workspace_dependency(&workspace, "selected", (WORKSPACE_HELLO, "1.0.0"));
    replace_workspace_dependency(&workspace, "unselected", (WORKSPACE_HELLO_PARENT, "1.0.0"));
    let unselected_manifest =
        fs::read(workspace.join("packages/unselected/package.json")).expect("read manifest");
    if lockfile_only {
        fs::remove_dir_all(&store_dir).expect("remove baseline client store");
    }
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);
    let mut args = vec!["--filter", "selected", "install", "--pnpr-server", &pnpr_url];
    if lockfile_only {
        args.push("--lockfile-only");
    }
    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(args)
        .assert()
        .success();
    let after = read_workspace_lockfile(&workspace);

    assert_eq!(
        fs::read(workspace.join("packages/unselected/package.json")).expect("read manifest"),
        unselected_manifest,
    );
    assert_eq!(workspace_importer(&after, "packages/unselected"), &prior_unselected);
    assert_eq!(workspace_snapshot_entries(&after, WORKSPACE_PARENT), prior_parent);
    assert_eq!(workspace_snapshot_entries(&after, WORKSPACE_DEP), prior_child);
    assert!(workspace_snapshot_entries(&after, WORKSPACE_HELLO_PARENT).is_empty());
    assert_eq!(workspace_importer_version(&after, "packages/selected", WORKSPACE_HELLO), "1.0.0");
    assert_eq!(workspace_importer_version(&after, ".", WORKSPACE_ROOT_DEP), "1.0.0");
    assert_eq!(
        fs::read(workspace.join("package.json")).expect("read root manifest"),
        root_manifest,
    );

    if lockfile_only {
        assert!(!workspace.join("node_modules").exists());
        assert!(!store_dir.join("v11/index.db").exists());
    } else {
        assert!(
            is_symlink_or_junction(&workspace.join("node_modules").join(WORKSPACE_ROOT_DEP))
                .unwrap_or(false),
            "workspace root dependency must be linked",
        );
        assert!(workspace_has_link(&workspace, "selected", WORKSPACE_HELLO));
        assert!(!workspace.join("packages/unselected/node_modules").exists());
        assert!(workspace_slot(&workspace, WORKSPACE_HELLO, "1.0.0").exists());
        assert!(!workspace_slot(&workspace, WORKSPACE_HELLO, "0.0.0").exists());
        assert!(!workspace_slot(&workspace, WORKSPACE_PARENT, "100.0.0").exists());
        assert!(!workspace_slot(&workspace, WORKSPACE_DEP, "100.1.0").exists());
        let current = read_workspace_current_lockfile(&workspace);
        assert_eq!(
            current.importers.keys().cloned().collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from([".".to_string(), "packages/selected".to_string()]),
        );
    }

    drop((root, mock_instance));
}

fn seed_filtered_repair_workspace(workspace: &Path, registry_url: &str) {
    configure_workspace(workspace);
    write_workspace_project(workspace, "selected", "selected", (WORKSPACE_HELLO, "0.0.0"));
    write_workspace_project(workspace, "unselected", "unselected", (WORKSPACE_PARENT, "100.0.0"));
    pacquet_at(workspace)
        .with_env("PNPM_CONFIG_REGISTRY", registry_url)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
}

fn selected_only_pnpr_lockfile(mut lockfile: Lockfile) -> Lockfile {
    lockfile.importers.retain(|id, _| id == "packages/selected");
    if let Some(packages) = lockfile.packages.as_mut() {
        packages.retain(|key, _| key.to_string().contains(WORKSPACE_HELLO));
    }
    if let Some(snapshots) = lockfile.snapshots.as_mut() {
        snapshots.retain(|key, _| key.to_string().contains(WORKSPACE_HELLO));
    }
    lockfile
}

fn mock_filtered_repair_response(
    server: &mut mockito::Server,
    lockfile: &Lockfile,
) -> (mockito::Mock, mockito::Mock) {
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"pnpr":{"versions":[0],"fixLockfile":[0]}}"#)
        .expect(1)
        .create();
    let response = serde_json::json!({
        "type": "done",
        "lockfile": lockfile,
        "stats": { "totalPackages": 0 },
    });
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(format!("{response}\n"))
        .expect(1)
        .create();
    (handshake, resolve)
}

#[test]
fn filtered_workspace_install_via_pnpr_materializes_the_root_and_selected_closure() {
    assert_filtered_workspace_pnpr(false);
}

#[test]
fn filtered_workspace_pnpr_lockfile_only_merges_the_root_and_selected_importers() {
    assert_filtered_workspace_pnpr(true);
}

#[test]
fn filtered_pnpr_repair_preserves_unselected_metadata() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    seed_filtered_repair_workspace(&workspace, &mock_instance.url());
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let mut previous = read_workspace_lockfile(&workspace);
    let previous_unselected = workspace_importer(&previous, "packages/unselected").clone();
    let mut preserved_package_count = 0;
    for (key, metadata) in previous.packages.as_mut().expect("packages") {
        let key = key.to_string();
        if key.contains(WORKSPACE_PARENT) || key.contains(WORKSPACE_DEP) {
            metadata.deprecated = Some("preserve this metadata".to_string());
            preserved_package_count += 1;
        }
    }
    assert!(preserved_package_count > 0);
    let mut preserved_snapshot_count = 0;
    for (key, snapshot) in previous.snapshots.as_mut().expect("snapshots") {
        let key = key.to_string();
        if key.contains(WORKSPACE_PARENT) || key.contains(WORKSPACE_DEP) {
            snapshot.optional = true;
            snapshot.transitive_peer_dependencies = Some(vec!["preserved-peer".to_string()]);
            preserved_snapshot_count += 1;
        }
    }
    assert!(preserved_snapshot_count > 0);
    let fresh = selected_only_pnpr_lockfile(previous.clone());
    let mut broken: serde_json::Value =
        serde_saphyr::from_str(&serde_saphyr::to_string(&previous).expect("serialize lockfile"))
            .expect("parse lockfile value");
    broken["time"] = serde_json::json!("invalid");
    fs::write(&lockfile_path, serde_saphyr::to_string(&broken).expect("serialize lockfile"))
        .expect("write broken lockfile");
    let mut server = mockito::Server::new();
    let (handshake_mock, resolve_mock) = mock_filtered_repair_response(&mut server, &fresh);

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args([
            "--filter",
            "selected",
            "install",
            "--fix-lockfile",
            "--lockfile-only",
            "--pnpr-server",
            &server.url(),
        ])
        .assert()
        .success();

    let repaired = read_workspace_lockfile(&workspace);
    assert_eq!(workspace_importer(&repaired, "packages/unselected"), &previous_unselected);
    let preserved_packages = repaired
        .packages
        .as_ref()
        .expect("repaired packages")
        .iter()
        .filter(|(key, _)| {
            let key = key.to_string();
            key.contains(WORKSPACE_PARENT) || key.contains(WORKSPACE_DEP)
        })
        .collect::<Vec<_>>();
    assert_eq!(preserved_packages.len(), preserved_package_count);
    assert!(
        preserved_packages.iter().all(|(_, metadata)| {
            metadata.deprecated.as_deref() == Some("preserve this metadata")
        }),
    );
    let preserved_snapshots = repaired
        .snapshots
        .as_ref()
        .expect("repaired snapshots")
        .iter()
        .filter(|(key, _)| {
            let key = key.to_string();
            key.contains(WORKSPACE_PARENT) || key.contains(WORKSPACE_DEP)
        })
        .collect::<Vec<_>>();
    assert_eq!(preserved_snapshots.len(), preserved_snapshot_count);
    assert!(preserved_snapshots.iter().all(|(_, snapshot)| {
        snapshot.optional
            && snapshot
                .transitive_peer_dependencies
                .as_ref()
                .is_some_and(|peers| peers.len() == 1 && peers[0] == "preserved-peer")
    }));
    handshake_mock.assert();
    resolve_mock.assert();

    drop((root, mock_instance));
}

#[test]
fn filtered_pnpr_repair_verifies_the_merged_lockfile_before_writing() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    seed_filtered_repair_workspace(&workspace, &mock_instance.url());
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let previous = read_workspace_lockfile(&workspace);
    let fresh = selected_only_pnpr_lockfile(previous.clone());
    let mut broken: serde_json::Value =
        serde_saphyr::from_str(&serde_saphyr::to_string(&previous).expect("serialize lockfile"))
            .expect("parse lockfile value");
    broken["time"] = serde_json::json!("invalid");
    broken["importers"]["packages/unselected"]["dependencies"]["../../../escape"] =
        serde_json::json!({ "specifier": "link:local", "version": "link:local" });
    let before = serde_saphyr::to_string(&broken).expect("serialize lockfile");
    fs::write(&lockfile_path, &before).expect("write broken lockfile");
    let mut server = mockito::Server::new();
    let (handshake_mock, resolve_mock) = mock_filtered_repair_response(&mut server, &fresh);

    let output = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args([
            "--filter",
            "selected",
            "install",
            "--fix-lockfile",
            "--lockfile-only",
            "--trust-lockfile",
            "--pnpr-server",
            &server.url(),
        ])
        .output()
        .expect("run filtered repair");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_INVALID_DEPENDENCY_NAME"),
        "merged repair must reject the traversal alias; got:\n{stderr}",
    );
    assert_eq!(fs::read_to_string(&lockfile_path).expect("read lockfile after failure"), before);
    handshake_mock.assert();
    resolve_mock.assert();

    drop((root, mock_instance));
}

#[test]
fn filtered_workspace_pnpr_reports_a_missing_selected_importer_without_panicking() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    write_workspace_project(&workspace, "selected", "selected", (WORKSPACE_HELLO, "1.0.0"));
    write_workspace_project(&workspace, "unselected", "unselected", (WORKSPACE_PARENT, "1.0.0"));

    let mut server = mockito::Server::new();
    let response = serde_json::json!({
        "type": "done",
        "lockfile": { "lockfileVersion": "9.0" },
        "stats": { "totalPackages": 0 },
    });
    let resolve_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(format!("{response}\n"))
        .expect(1)
        .create();

    let output = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["--filter", "selected", "install", "--pnpr-server", &server.url()])
        .output()
        .expect("run filtered install against a malformed pnpr response");

    assert!(!output.status.success(), "a malformed pnpr lockfile must fail the install");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("fresh lockfile is missing importer packages/selected"),
        "stderr must identify the missing selected importer; got:\n{stderr}",
    );
    assert!(
        !stderr.contains("panicked at"),
        "the malformed response must not panic; got:\n{stderr}",
    );
    resolve_mock.assert();
    drop((root, mock_instance));
}

#[test]
fn filtered_workspace_pnpr_resolves_workspace_protocol_from_project_identity() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    write_workspace_project(&workspace, "app", "app", ("lib", "workspace:*"));
    write_workspace_project(&workspace, "lib", "lib", (WORKSPACE_HELLO, "1.0.0"));
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["--filter", "app", "install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let wanted = read_workspace_lockfile(&workspace);
    assert_eq!(workspace_importer_version(&wanted, "packages/app", "lib"), "link:../lib");
    assert!(workspace_has_link(&workspace, "app", "lib"));
    assert!(!workspace_has_link(&workspace, "lib", WORKSPACE_HELLO));
    assert!(!workspace_slot(&workspace, WORKSPACE_HELLO, "1.0.0").exists());

    drop((root, mock_instance));
}
