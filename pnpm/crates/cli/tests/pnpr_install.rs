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
use pacquet_lockfile::{Lockfile, PkgName, ProjectSnapshot, SnapshotEntry};
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
fn frozen_install_via_pnpr_verifies_the_local_lockfile_without_resolving_or_redownloading() {
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

#[test]
fn import_via_pnpr_server_writes_lockfile_without_linking() {
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
        .with_arg("import")
        .with_arg("--pnpr-server")
        .with_arg(&pnpr_url)
        .assert()
        .success();

    assert!(workspace.join("pnpm-lock.yaml").exists(), "pnpr should write the lockfile");
    assert!(!workspace.join("node_modules").exists(), "import must not link node_modules");
    assert!(!store_dir.join("v11/index.db").exists(), "import must not populate the client store");

    drop((root, mock_instance));
}

const WORKSPACE_DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const WORKSPACE_HELLO: &str = "@pnpm.e2e/hello-world-js-bin";
const WORKSPACE_HELLO_PARENT: &str = "@pnpm.e2e/hello-world-js-bin-parent";
const WORKSPACE_PARENT: &str = "@pnpm.e2e/pkg-with-1-dep";

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

#[test]
fn standard_workspace_install_via_pnpr_from_root_resolves_every_real_importer() {
    assert_standard_workspace_pnpr_from(None);
}

#[test]
fn standard_workspace_install_via_pnpr_from_member_resolves_every_real_importer() {
    assert_standard_workspace_pnpr_from(Some("packages/app"));
}

#[test]
fn frozen_lockfile_only_workspace_install_via_pnpr_from_member_uses_every_real_importer() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    write_workspace_project(&workspace, "app", "app", (WORKSPACE_HELLO, "1.0.0"));
    write_workspace_project(&workspace, "lib", "lib", (WORKSPACE_PARENT, "100.0.0"));
    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let before = read_workspace_lockfile(&workspace);
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    pacquet_at(&workspace.join("packages/app"))
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--frozen-lockfile", "--lockfile-only", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    assert_eq!(read_workspace_lockfile(&workspace), before);
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
    assert_eq!(workspace_importer_version(&after, "packages/selected", WORKSPACE_HELLO), "1.0.0",);
    assert!(!after.importers.contains_key("."));

    if lockfile_only {
        assert!(!workspace.join("node_modules").exists());
        assert!(!store_dir.join("v11/index.db").exists());
    } else {
        assert!(workspace_has_link(&workspace, "selected", WORKSPACE_HELLO));
        assert!(!workspace.join("packages/unselected/node_modules").exists());
        assert!(workspace_slot(&workspace, WORKSPACE_HELLO, "1.0.0").exists());
        assert!(!workspace_slot(&workspace, WORKSPACE_HELLO, "0.0.0").exists());
        assert!(!workspace_slot(&workspace, WORKSPACE_PARENT, "100.0.0").exists());
        assert!(!workspace_slot(&workspace, WORKSPACE_DEP, "100.1.0").exists());
        let current = read_workspace_current_lockfile(&workspace);
        assert_eq!(
            current.importers.keys().cloned().collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from(["packages/selected".to_string()]),
        );
    }

    drop((root, mock_instance));
}

#[test]
fn filtered_workspace_install_via_pnpr_materializes_only_selected_closure() {
    assert_filtered_workspace_pnpr(false);
}

#[test]
fn filtered_workspace_pnpr_lockfile_only_merges_prior_wanted_without_root_importer() {
    assert_filtered_workspace_pnpr(true);
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
        "the malformed response must not panic; got:\n{stderr}"
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
    assert!(workspace_has_link(&workspace, "lib", WORKSPACE_HELLO));
    assert!(workspace_slot(&workspace, WORKSPACE_HELLO, "1.0.0").exists());

    drop((root, mock_instance));
}
