//! End-to-end test for `pacquet install --pnpr-server <url>`.
//!
//! Runs the real `pacquet` binary against a mocked fixtures registry,
//! with an in-process `pnpr` hosting the fast-path endpoints. The pnpr
//! server's own upstream is left at the default; the client sends the
//! registry it wants resolved from (the mock, which the server allowlists
//! as a public route), so a passing test proves resolution used the
//! client-supplied registry. The client then links `node_modules` from the
//! server-produced lockfile.

use crate::cargo_install::crate_archive;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_crypto_hash::integrity_addressed_tarball_path;
use pnpm_lockfile::{Lockfile, PkgName, ProjectSnapshot, SnapshotEntry};
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::{get_all_files, is_symlink_or_junction},
};
use pnpr::{Ecosystem, Registries, Registry, TokenBackend, UpstreamConfig};
use reqwest::header::HeaderMap;
use sha2::{Digest, Sha256};
use std::{
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
    let server = PnprServer::bind("pnpr");
    let tokens_path = server.storage.join("tokens.db");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("token setup runtime");
    let token = runtime.block_on(async {
        let tokens = pnpr::TokenStore::open(tokens_path.clone()).expect("open token store");
        tokens.issue("pacquet-test").await.expect("issue pnpr test token")
    });

    let addr = server.serve(move |config| {
        config.auth.tokens.file = Some(tokens_path);
        config
            .route_policy
            .public
            .push(pnpr::PublicRoute { registry: Some(registry_url), package: None });
    });
    (format!("http://{addr}/"), token)
}

/// Start an in-process pnpr that proxies one upstream registry of
/// `ecosystem`, and return the base URL its clients address.
fn start_pnpr_registry(upstream_url: &str, ecosystem: Ecosystem) -> String {
    let upstream_url = upstream_url.to_string();
    let name = "upstream";
    let addr = PnprServer::bind("pnpr-registry").serve(move |config| {
        config.upstreams.insert(
            name.to_string(),
            UpstreamConfig::with_defaults(upstream_url, HeaderMap::new()),
        );
        config.registries = Registries::new(
            indexmap::IndexMap::from([(
                name.to_string(),
                Registry::Upstream { patterns: Vec::new() },
            )]),
            Some(name.to_string()),
        )
        .with_ecosystem(name, ecosystem);
    });
    // The server's root is its npm alias; every other ecosystem is
    // addressed under its own prefix.
    if ecosystem == Ecosystem::Npm {
        format!("http://{addr}")
    } else {
        format!("http://{addr}/{ecosystem}/")
    }
}

/// A bound port and storage directory waiting for [`Self::serve`] to start
/// pnpr on them. Binding first lets a caller seed storage — a token store,
/// say — with the address the server will answer on already known.
struct PnprServer {
    name: &'static str,
    listener: TcpListener,
    addr: SocketAddr,
    /// Persisted (not cleaned) because the detached server thread outlives
    /// the test that started it.
    storage: std::path::PathBuf,
}

impl PnprServer {
    fn bind(name: &'static str) -> Self {
        let storage = tempfile::tempdir().expect("pnpr storage").keep();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind pnpr");
        // tokio's `from_std` requires the listener to be non-blocking.
        listener.set_nonblocking(true).expect("set pnpr listener non-blocking");
        let addr = listener.local_addr().expect("pnpr addr");
        Self { name, listener, addr, storage }
    }

    /// Run the server on a detached thread until the process exits, with
    /// `configure` applied to the proxy defaults. Returns once it answers.
    fn serve(self, configure: impl FnOnce(&mut pnpr::Config) + Send + 'static) -> SocketAddr {
        let Self { name, listener, addr, storage } = self;
        thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("pnpr runtime");
                runtime.block_on(async move {
                    let mut config = pnpr::Config::proxy(addr, storage);
                    config.public_url = format!("http://{addr}");
                    configure(&mut config);
                    let listener =
                        tokio::net::TcpListener::from_std(listener).expect("tokio listener");
                    let _ = pnpr::serve_listener(config, listener).await;
                });
            })
            .expect("spawn pnpr thread");

        wait_until_ready(addr);
        addr
    }
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

/// Whether a lockfile key names one of the workspace packages the repair
/// test marks and then checks for.
fn is_preserved_key(key: &str) -> bool {
    key.contains(WORKSPACE_PARENT) || key.contains(WORKSPACE_DEP)
}

#[test]
fn cargo_install_uses_a_configured_pnpr_registry_and_accelerator() {
    let mut upstream = mockito::Server::new();
    let archive = crate_archive("demo", "1.0.0");
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let _config_mock = upstream
        .mock("GET", "/config.json")
        .with_body(
            serde_json::json!({
                "dl": format!("{}/dl/{{crate}}/{{version}}", upstream.url()),
                "api": upstream.url(),
            })
            .to_string(),
        )
        .create();
    let index_mock = upstream
        .mock("GET", "/de/mo/demo")
        .with_body(format!(
            "{}\n",
            serde_json::json!({
                "name": "demo",
                "vers": "1.0.0",
                "deps": [],
                "cksum": checksum,
                "features": {},
                "yanked": false,
                "v": 1,
            }),
        ))
        .expect(1)
        .create();
    let download_mock =
        upstream.mock("GET", "/dl/demo/1.0.0").with_body(&archive).expect(1).create();
    let registry_url = start_pnpr_registry(&upstream.url(), Ecosystem::Cargo);
    let (pnpr_url, token) = start_pnpr(&format!("{registry_url}index"));

    let root = tempfile::tempdir().expect("create Cargo project");
    fs::create_dir(root.path().join("src")).expect("create source directory");
    fs::write(root.path().join("src/lib.rs"), "pub use demo::answer;\n").expect("write source");
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ndemo = \"1\"\n",
    )
    .expect("write Cargo manifest");
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        format!(
            "cargo:\n  enabled: true\n  indexUrl: {registry_url}index/\npnprServer: {pnpr_url}\n",
        ),
    )
    .expect("configure pnpm");
    fs::write(
        root.path().join(".npmrc"),
        format!(
            "//{}/:_authToken={token}\n",
            pnpr_url.trim_start_matches("http://").trim_end_matches('/'),
        ),
    )
    .expect("configure pnpr authentication");

    pacquet_at(root.path())
        .with_env("PNPM_CONFIG_CACHE_DIR", root.path().join("cache"))
        .with_env("PNPM_CONFIG_STORE_DIR", root.path().join("store"))
        .with_arg("install")
        .assert()
        .success();

    let lockfile = fs::read_to_string(root.path().join("Cargo.lock")).expect("read Cargo lockfile");
    assert!(lockfile.contains(&format!(r#"source = "sparse+{registry_url}index/""#)), "{lockfile}");
    assert!(root.path().join(".pnpm/crates/crates-io/demo-1.0.0/src/lib.rs").is_file());
    // The accelerator resolved: a local resolve would have walked the sparse
    // index itself and left the entry it read in the client's index cache.
    // Only the registry's config.json, which the download needs either way,
    // is cached here.
    let cached_index_files = get_all_files(&root.path().join("cache/v11/cargo-index"));
    assert!(
        cached_index_files.iter().all(|path| path.ends_with("config.json")),
        "{cached_index_files:?}",
    );
    Command::new("cargo")
        .with_current_dir(root.path())
        .with_args(["check", "--offline"])
        .assert()
        .success();

    index_mock.assert();
    download_mock.assert();
}

mod workspace;

mod revisions;

mod resolution;
