use super::{
    ArchiveStoreProjection, Config, LockedCrate, MANAGED_CONFIG, MaterializeOptions,
    add_cargo_checksum, download_auth_headers, fetch_sparse_index_file, managed_config,
    materialize, parse_lockfile, resolve_via_pnpr, sparse_index_path, update_managed_config,
    workspace_root,
};
use cargo_util_schemas::index::RegistryConfig;
use pnpm_cargo_resolver::CRATES_IO_SPARSE_INDEX;
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::{
    CafsFileInfo, PackageFilesIndex, SharedVerifiedFilesCache, StoreDir, StoreIndex,
    StoreIndexWriter,
};
use ssri::{Algorithm, Integrity};
use std::{
    collections::HashMap,
    fs,
    sync::{Arc, atomic::AtomicU8},
    time::Duration,
};

#[cfg(unix)]
use super::{
    ensure_workspace_directory, link_workspace, link_workspace_in, write_cargo_config,
    write_cargo_config_in,
};

#[cfg(unix)]
use std::os::unix::fs::symlink;

#[cfg(windows)]
use super::ensure_workspace_directory_windows;

#[test]
fn parses_crates_io_packages_and_ignores_workspace_packages() {
    let lockfile = r#"
version = 4

[[package]]
name = "workspace-member"
version = "0.1.0"

[[package]]
name = "serde"
version = "1.0.228"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"
"#;

    assert_eq!(
        parse_lockfile(lockfile, CRATES_IO_SPARSE_INDEX).unwrap(),
        vec![LockedCrate {
            name: "serde".to_string(),
            version: "1.0.228".to_string(),
            checksum: "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"
                .to_string(),
        }],
    );
}

#[test]
fn rejects_non_crates_io_sources() {
    let lockfile = r#"
[[package]]
name = "private"
version = "1.0.0"
source = "registry+https://registry.example/index"
checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
"#;

    let error = parse_lockfile(lockfile, CRATES_IO_SPARSE_INDEX).unwrap_err().to_string();
    assert!(error.contains("does not match the configured Cargo registry"), "{error}");
}

#[test]
fn rejects_a_crates_io_source_under_a_configured_registry() {
    let lockfile = r#"
[[package]]
name = "serde"
version = "1.0.228"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"
"#;

    let error =
        parse_lockfile(lockfile, "https://registry.example.test/index/").unwrap_err().to_string();

    assert!(error.contains("does not match the configured Cargo registry"), "{error}");
}

#[test]
fn accepts_the_configured_registry_source() {
    let lockfile = r#"
[[package]]
name = "serde"
version = "1.0.228"
source = "sparse+https://registry.example.test/index/"
checksum = "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"
"#;

    assert_eq!(
        parse_lockfile(lockfile, "https://registry.example.test/index/").unwrap(),
        vec![LockedCrate {
            name: "serde".to_string(),
            version: "1.0.228".to_string(),
            checksum: "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"
                .to_string(),
        }],
    );
}

#[test]
fn accepts_the_sparse_spelling_of_crates_io() {
    let lockfile = r#"
[[package]]
name = "serde"
version = "1.0.228"
source = "sparse+https://index.crates.io/"
checksum = "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"
"#;

    assert_eq!(parse_lockfile(lockfile, CRATES_IO_SPARSE_INDEX).unwrap().len(), 1);
}

#[test]
fn ignores_fields_from_non_package_lockfile_tables() {
    let lockfile = r#"
version = 4

[[package]]
name = "serde"
version = "1.0.228"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"

[[patch.unused]]
name = "unselected"
version = "2.0.0"
source = "registry+https://registry.example/index"
"#;

    assert_eq!(
        parse_lockfile(lockfile, CRATES_IO_SPARSE_INDEX).unwrap(),
        vec![LockedCrate {
            name: "serde".to_string(),
            version: "1.0.228".to_string(),
            checksum: "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"
                .to_string(),
        }],
    );
}

#[test]
fn crate_store_slots_are_grouped_by_name_version_and_content() {
    let package = LockedCrate {
        name: "serde".to_string(),
        version: "1.0.228".to_string(),
        checksum: "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e".to_string(),
    };

    assert_eq!(
        package.store_slot(std::path::Path::new("store/v11")),
        std::path::Path::new("store/v11")
            .join("crates")
            .join("serde")
            .join("1.0.228")
            .join("9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"),
    );
}

#[test]
fn appends_the_managed_config_without_changing_user_settings() {
    let existing = "[alias]\ncodecov = \"llvm-cov\"\n";
    let updated = update_managed_config(existing, CRATES_IO_SPARSE_INDEX).unwrap();

    assert_eq!(updated, format!("{existing}\n{MANAGED_CONFIG}\n"));
}

#[test]
fn replaces_only_the_existing_managed_config() {
    let existing = "before\n# >>> pnpm-managed cargo sources >>>\nstale\n# <<< pnpm-managed cargo sources <<<\nafter\n";
    let updated = update_managed_config(existing, CRATES_IO_SPARSE_INDEX).unwrap();

    assert_eq!(updated, format!("before\n{MANAGED_CONFIG}\nafter\n"));
}

#[test]
fn configures_the_selected_sparse_registry_as_the_vendored_source() {
    let config = managed_config("https://registry.example.test/index/");

    assert!(config.contains("[source.crates-io]\nreplace-with = \"pnpm-registry\""));
    assert!(config.contains("[source.pnpm-registry]"));
    assert!(config.contains(r#"registry = "sparse+https://registry.example.test/index/""#));
    assert!(config.contains(r#"replace-with = "pnpm-registry-directory""#));
}

#[test]
fn escapes_a_registry_url_that_is_not_a_bare_toml_string() {
    let config = managed_config("https://registry.example.test/o'brien/index");

    let parsed: toml::Table = toml::from_str(&config).expect("managed config is valid TOML");
    assert_eq!(
        parsed["source"]["pnpm-registry"]["registry"].as_str(),
        Some("sparse+https://registry.example.test/o'brien/index/"),
    );
}

#[test]
fn rejects_an_incomplete_managed_config() {
    let error =
        update_managed_config("# >>> pnpm-managed cargo sources >>>\n", CRATES_IO_SPARSE_INDEX)
            .unwrap_err()
            .to_string();

    assert!(error.contains("incomplete"), "{error}");
}

#[test]
fn creates_the_cargo_checksum_manifest_from_cas_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let store_dir = StoreDir::from(temp_dir.path().join("store"));
    let (cargo_toml, _) = store_dir.write_cas_file(b"[package]\nname = \"demo\"\n", false).unwrap();
    let (source, _) = store_dir.write_cas_file(b"fn main() {}\n", false).unwrap();
    let mut cas_paths = HashMap::from([
        ("Cargo.toml".to_string(), cargo_toml),
        ("src/main.rs".to_string(), source),
    ]);

    add_cargo_checksum(
        &store_dir,
        &mut cas_paths,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap();

    let manifest_path = cas_paths.get(".cargo-checksum.json").unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest_path).unwrap()).unwrap();
    assert_eq!(
        manifest["package"],
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    assert_eq!(
        manifest["files"]["Cargo.toml"],
        "5f55e5180ed66d818f61920fd7b0205a164b782a105610f293acd5ec68d0eacb",
    );
    assert_eq!(
        manifest["files"]["src/main.rs"],
        "536e506bb90914c243a12b397b9a998f85ae2cbd9ba02dfd03a9e155ca5ca0f4",
    );
}

#[tokio::test]
async fn repairs_a_preseeded_slot_from_verified_store_metadata() {
    let temp_dir = tempfile::tempdir().unwrap();
    let store_dir = Box::leak(Box::new(StoreDir::from(temp_dir.path().join("store"))));
    store_dir.init().unwrap();
    let cargo_toml = b"[package]\nname = \"demo\"\nversion = \"1.0.0\"\n";
    let source = b"pub fn trusted() {}\n";
    let (cargo_toml_path, cargo_toml_hash) = store_dir.write_cas_file(cargo_toml, false).unwrap();
    let (source_path, source_hash) = store_dir.write_cas_file(source, false).unwrap();
    let files = HashMap::from([
        (
            "Cargo.toml".to_string(),
            CafsFileInfo {
                digest: format!("{cargo_toml_hash:x}"),
                mode: 0o644,
                size: cargo_toml.len() as u64,
                checked_at: None,
            },
        ),
        (
            "src/lib.rs".to_string(),
            CafsFileInfo {
                digest: format!("{source_hash:x}"),
                mode: 0o644,
                size: source.len() as u64,
                checked_at: None,
            },
        ),
    ]);
    let checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let integrity = Integrity::from_hex(checksum, Algorithm::Sha256).unwrap();
    let package_id = "crate:demo@1.0.0";
    StoreIndex::open_in(store_dir)
        .unwrap()
        .set(
            &ArchiveStoreProjection::RawArchive.store_index_key(&integrity.to_string(), package_id),
            &PackageFilesIndex {
                manifest: None,
                requires_build: Some(false),
                requires_prepare: None,
                algo: "sha512".to_string(),
                files,
                side_effects: None,
                remote_side_effects_quarantine: None,
            },
        )
        .unwrap();
    let package = LockedCrate {
        name: "demo".to_string(),
        version: "1.0.0".to_string(),
        checksum: checksum.to_string(),
    };
    let slot = package.store_slot(store_dir.root());
    fs::create_dir_all(slot.join("src")).unwrap();
    fs::write(slot.join("package.json"), "{}").unwrap();
    fs::write(slot.join("Cargo.toml"), "attacker controlled").unwrap();
    fs::write(slot.join("src/lib.rs"), "pub fn substituted() {}\n").unwrap();
    fs::write(slot.join(".cargo-checksum.json"), "{}").unwrap();
    let (store_index_writer, writer_task) = StoreIndexWriter::spawn(store_dir);

    materialize::<SilentReporter>(MaterializeOptions {
        package,
        store_dir,
        store_index: StoreIndex::shared_readonly_in(store_dir),
        store_index_writer: Arc::clone(&store_index_writer),
        http_client: Arc::new(ThrottledClient::default()),
        auth_headers: Arc::new(AuthHeaders::default()),
        download_template: "https://static.crates.io/crates".to_string(),
        verified_files_cache: SharedVerifiedFilesCache::default(),
        logged_methods: Arc::new(AtomicU8::new(0)),
        package_import_method: pnpm_config::PackageImportMethod::default(),
        retry_opts: RetryOpts {
            retries: 0,
            factor: 1,
            min_timeout: Duration::ZERO,
            max_timeout: Duration::ZERO,
        },
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        offline: true,
        requester: "test".to_string(),
    })
    .await
    .unwrap();
    drop(store_index_writer);
    StoreIndexWriter::drain(writer_task, "").await;

    assert_eq!(fs::read(slot.join("Cargo.toml")).unwrap(), cargo_toml);
    assert_eq!(fs::read(slot.join("src/lib.rs")).unwrap(), source);
    assert!(slot.join(".cargo-checksum.json").is_file());
    assert!(cargo_toml_path.is_file());
    assert!(source_path.is_file());
}

#[test]
fn maps_crate_names_to_sparse_index_paths() {
    assert_eq!(sparse_index_path("a").unwrap(), "1/a");
    assert_eq!(sparse_index_path("ab").unwrap(), "2/ab");
    assert_eq!(sparse_index_path("abc").unwrap(), "3/a/abc");
    assert_eq!(sparse_index_path("Serde_JSON").unwrap(), "se/rd/serde_json");
}

fn config_with_cargo_credentials(index_url: &str) -> Config {
    let mut config = Config::new();
    config.cargo.index_url = index_url.to_string();
    config.auth_headers = Arc::new(AuthHeaders::from_creds_map([
        ("//registry.example.test/".to_string(), "Bearer crate-token".to_string()),
        ("//cdn.example.test/".to_string(), "Bearer unrelated-token".to_string()),
        ("//127.0.0.1:4873/".to_string(), "Bearer local-token".to_string()),
    ]));
    config
}

fn registry_config(dl: &str, auth_required: bool) -> RegistryConfig {
    RegistryConfig { dl: dl.to_string(), api: None, auth_required }
}

#[test]
fn crate_downloads_keep_credentials_off_plaintext_hosts() {
    let config = config_with_cargo_credentials("https://registry.example.test/index/");
    let auth_headers =
        download_auth_headers(&config, &registry_config("https://registry.example.test/dl", false));

    assert_eq!(
        auth_headers.for_url_with_package("https://registry.example.test/dl/demo/1.0.0", None),
        Some("Bearer crate-token".to_string()),
    );
    assert_eq!(
        auth_headers.for_url_with_package("http://registry.example.test/dl/demo/1.0.0", None),
        None,
    );
}

#[test]
fn a_registry_on_loopback_still_authenticates_its_downloads() {
    let config = config_with_cargo_credentials("http://127.0.0.1:4873/index/");
    let auth_headers =
        download_auth_headers(&config, &registry_config("http://127.0.0.1:4873/dl", false));

    assert_eq!(
        auth_headers.for_url_with_package("http://127.0.0.1:4873/dl/demo/1.0.0", None),
        Some("Bearer local-token".to_string()),
    );
}

#[test]
fn an_unauthenticated_archive_host_carries_no_credential() {
    let config = config_with_cargo_credentials("https://registry.example.test/index/");
    let auth_headers =
        download_auth_headers(&config, &registry_config("https://cdn.example.test/{crate}", false));

    assert_eq!(auth_headers.for_url_with_package("https://cdn.example.test/demo", None), None);
    assert!(auth_headers.allows_fetch("https://cdn.example.test/demo"));
}

#[test]
fn an_authenticated_archive_host_carries_the_credential_of_the_registry() {
    let config = config_with_cargo_credentials("https://registry.example.test/index/");
    let auth_headers =
        download_auth_headers(&config, &registry_config("https://cdn.example.test/{crate}", true));

    // The registry's credential, not the one configured for the host it named.
    assert_eq!(
        auth_headers.for_url_with_package("https://cdn.example.test/demo", None),
        Some("Bearer crate-token".to_string()),
    );
}

#[tokio::test]
async fn sparse_index_fetch_uses_configured_request_auth() {
    let mut server = mockito::Server::new_async().await;
    let response = r#"{"name":"demo","vers":"1.0.0","deps":[],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}"#;
    let request = server
        .mock("GET", "/de/mo/demo")
        .match_header("authorization", "Bearer cargo-read-token")
        .with_status(200)
        .with_body(response)
        .create_async()
        .await;
    let auth_headers = AuthHeaders::from_creds_map([(
        pnpm_network::nerf_dart(&server.url()),
        "Bearer cargo-read-token".to_string(),
    )]);
    let cache = tempfile::tempdir().unwrap();

    let contents = fetch_sparse_index_file(
        "demo",
        &server.url(),
        cache.path(),
        &ThrottledClient::default(),
        &auth_headers,
        false,
        RetryOpts { retries: 0, ..RetryOpts::default() },
    )
    .await
    .unwrap();

    assert_eq!(contents, response);
    request.assert_async().await;
}

#[tokio::test]
async fn asks_cargo_for_the_workspace_root_of_a_member() {
    let repository = tempfile::tempdir().unwrap();
    let cargo_root = repository.path().join("rust");
    let member = cargo_root.join("member");
    fs::create_dir_all(member.join("src")).unwrap();
    fs::write(
        cargo_root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(member.join("src/lib.rs"), "").unwrap();

    assert_eq!(workspace_root(&member.join("Cargo.toml")).await.unwrap(), cargo_root);
}

#[cfg(unix)]
#[test]
fn rejects_a_symlinked_cargo_source_parent() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("keep"), "unchanged").unwrap();
    symlink(outside.path(), workspace.path().join(".pnpm")).unwrap();

    let error = link_workspace(workspace.path(), &[]).unwrap_err().to_string();

    assert!(error.contains("must be a real directory"), "{error}");
    assert_eq!(fs::read_to_string(outside.path().join("keep")).unwrap(), "unchanged");
}

#[cfg(unix)]
#[test]
fn rejects_a_symlinked_cargo_config_parent() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let external_config = outside.path().join("config.toml");
    fs::write(&external_config, "unchanged\n").unwrap();
    symlink(outside.path(), workspace.path().join(".cargo")).unwrap();

    let error =
        write_cargo_config(workspace.path(), CRATES_IO_SPARSE_INDEX).unwrap_err().to_string();

    assert!(error.contains("must be a real directory"), "{error}");
    assert_eq!(fs::read_to_string(external_config).unwrap(), "unchanged\n");
}

#[cfg(unix)]
#[test]
fn config_write_stays_in_the_directory_pinned_before_a_parent_swap() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let cargo_dir = ensure_workspace_directory(workspace.path(), &[".cargo"]).unwrap();
    let pinned_path = workspace.path().join(".cargo-pinned");
    fs::rename(workspace.path().join(".cargo"), &pinned_path).unwrap();
    fs::write(outside.path().join("config.toml"), "unchanged\n").unwrap();
    symlink(outside.path(), workspace.path().join(".cargo")).unwrap();

    write_cargo_config_in(&cargo_dir, CRATES_IO_SPARSE_INDEX).unwrap();

    assert_eq!(fs::read_to_string(outside.path().join("config.toml")).unwrap(), "unchanged\n");
    assert!(fs::read_to_string(pinned_path.join("config.toml")).unwrap().contains(MANAGED_CONFIG));
}

#[cfg(unix)]
#[test]
fn crate_link_stays_in_the_directory_pinned_before_a_parent_swap() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let slot = tempfile::tempdir().unwrap();
    let source_dir =
        ensure_workspace_directory(workspace.path(), &[".pnpm", "crates", "crates-io"]).unwrap();
    let source_path = workspace.path().join(".pnpm/crates/crates-io");
    let pinned_path = workspace.path().join(".pnpm/crates/crates-io-pinned");
    fs::rename(&source_path, &pinned_path).unwrap();
    symlink(outside.path(), &source_path).unwrap();

    link_workspace_in(&source_dir, &[("example-1.0.0".to_string(), slot.path().to_path_buf())])
        .unwrap();

    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    assert_eq!(
        fs::read_link(pinned_path.join("example-1.0.0")).unwrap(),
        pnpm_fs::relative_path(&source_path, slot.path()),
    );
}

#[cfg(unix)]
#[test]
fn crate_link_does_not_overwrite_a_nonempty_stale_backup() {
    let workspace = tempfile::tempdir().unwrap();
    let slot = tempfile::tempdir().unwrap();
    let source_path = workspace.path().join(".pnpm/crates/crates-io");
    let stale_backup = source_path.join(".ignored_example-1.0.0");
    fs::create_dir_all(source_path.join("example-1.0.0")).unwrap();
    fs::create_dir(&stale_backup).unwrap();
    fs::write(stale_backup.join("keep"), "unchanged").unwrap();

    link_workspace(workspace.path(), &[("example-1.0.0".to_string(), slot.path().to_path_buf())])
        .unwrap();

    assert_eq!(fs::read_to_string(stale_backup.join("keep")).unwrap(), "unchanged");
    assert_eq!(
        fs::read_link(source_path.join("example-1.0.0")).unwrap(),
        pnpm_fs::relative_path(&source_path, slot.path()),
    );
}

#[cfg(windows)]
#[test]
fn rejects_a_reparse_point_swapped_into_the_workspace_root() {
    let parent = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let swapped_root = parent.path().join("workspace");
    pnpm_fs::symlink_dir(outside.path(), &swapped_root).unwrap();

    let error = ensure_workspace_directory_windows(swapped_root, &[])
        .err()
        .expect("a reparse-point workspace root must be rejected")
        .to_string();

    assert!(error.contains("must be a real directory"), "{error}");
}

fn handshake_body(ecosystems: &[&str]) -> String {
    serde_json::json!({
        "pnpr": { "versions": [0], "artifacts": [], "fixLockfile": [0], "ecosystems": ecosystems },
    })
    .to_string()
}

fn config_for_pnpr(server: &str) -> Config {
    let mut config = Config::new();
    config.pnpr_server = Some(server.to_string());
    config
}

#[tokio::test]
async fn cargo_resolution_is_offloaded_to_the_pnpr_server() {
    const METADATA: &str = r#"{
      "packages": [{
        "id": "path+file:///home/dev/private-workspace#app@0.1.0",
        "name": "app",
        "version": "0.1.0",
        "manifest_path": "/home/dev/private-workspace/Cargo.toml",
        "dependencies": []
      }],
      "workspace_members": ["path+file:///home/dev/private-workspace#app@0.1.0"]
    }"#;
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "cargo"]))
        .create_async()
        .await;
    let sent_metadata = pnpm_cargo_resolver::resolve_inputs(METADATA).unwrap();
    assert!(!sent_metadata.contains("private-workspace"), "{sent_metadata}");
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "ecosystem": "cargo",
            "metadata": sent_metadata,
            "registry": CRATES_IO_SPARSE_INDEX,
        })))
        .with_header("content-type", "application/x-ndjson")
        .with_body("{\"type\":\"done\",\"lockfile\":\"version = 4\\n\"}\n")
        .expect(1)
        .create_async()
        .await;

    let lockfile = resolve_via_pnpr(&config_for_pnpr(&server.url()), METADATA).await.unwrap();

    assert_eq!(lockfile.as_deref(), Some("version = 4\n"));
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn configured_cargo_registry_is_sent_to_the_pnpr_server() {
    let mut server = mockito::Server::new_async().await;
    let handshake =
        server.mock("GET", "/-/pnpr").with_body(handshake_body(&["cargo"])).create_async().await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "ecosystem": "cargo",
            "registry": "https://registry.example.test/index/",
        })))
        .with_header("content-type", "application/x-ndjson")
        .with_body("{\"type\":\"done\",\"lockfile\":\"version = 4\\n\"}\n")
        .create_async()
        .await;
    let mut config = config_for_pnpr(&server.url());
    config.cargo.index_url = "https://registry.example.test/index/".to_string();

    let lockfile =
        resolve_via_pnpr(&config, r#"{"packages":[],"workspace_members":[]}"#).await.unwrap();

    assert_eq!(lockfile.as_deref(), Some("version = 4\n"));
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn a_server_without_cargo_support_leaves_resolution_local() {
    let mut server = mockito::Server::new_async().await;
    let handshake =
        server.mock("GET", "/-/pnpr").with_body(handshake_body(&["npm"])).create_async().await;
    let resolve = server.mock("POST", "/-/pnpr/v0/resolve").expect(0).create_async().await;

    let lockfile = resolve_via_pnpr(&config_for_pnpr(&server.url()), "{}").await.unwrap();

    assert_eq!(lockfile, None);
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn an_unterminated_pnpr_response_does_not_grow_without_bound() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "cargo"]))
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_header("content-type", "application/x-ndjson")
        .with_body("x".repeat(33 * 1024 * 1024))
        .create_async()
        .await;

    let metadata = r#"{"packages":[],"workspace_members":[]}"#;
    let error = resolve_via_pnpr(&config_for_pnpr(&server.url()), metadata).await.unwrap_err();

    assert!(
        error.chain().any(|cause| cause.to_string().contains("exceeds the")),
        "the oversized body is refused by its size, not by parsing: {error:?}",
    );
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn a_second_terminal_frame_fails_the_resolve() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "cargo"]))
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_header("content-type", "application/x-ndjson")
        .with_body(concat!(
            "{\"type\":\"done\",\"lockfile\":\"version = 4\\n\"}\n",
            "{\"type\":\"error\",\"message\":\"resolution failed\"}\n",
        ))
        .create_async()
        .await;

    let metadata = r#"{"packages":[],"workspace_members":[]}"#;
    let error = resolve_via_pnpr(&config_for_pnpr(&server.url()), metadata).await.unwrap_err();

    assert!(
        error.chain().any(|cause| cause.to_string().contains("more than one terminal frame")),
        "a response that also reports a failure is not a lockfile to write: {error:?}",
    );
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn concurrent_roots_share_one_handshake() {
    const METADATA: &str = r#"{"packages":[],"workspace_members":[]}"#;
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "cargo"]))
        .expect(1)
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_header("content-type", "application/x-ndjson")
        .with_body("{\"type\":\"done\",\"lockfile\":\"version = 4\\n\"}\n")
        .expect(4)
        .create_async()
        .await;
    let config = config_for_pnpr(&server.url());

    let roots = (0..4).map(|_| resolve_via_pnpr(&config, METADATA));
    for resolved in futures_util::future::join_all(roots).await {
        resolved.unwrap().expect("the server resolves Cargo");
    }

    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn concurrent_roots_share_one_failed_handshake() {
    const METADATA: &str = r#"{"packages":[],"workspace_members":[]}"#;
    let mut server = mockito::Server::new_async().await;
    let handshake = server.mock("GET", "/-/pnpr").with_status(500).expect(1).create_async().await;
    let config = config_for_pnpr(&server.url());

    let roots = (0..4).map(|_| resolve_via_pnpr(&config, METADATA));
    for resolved in futures_util::future::join_all(roots).await {
        let error = resolved.unwrap_err();
        assert!(
            error.to_string().contains("whether it resolves cargo"),
            "every root reports the same refusal: {error:?}",
        );
    }

    handshake.assert_async().await;
}

#[tokio::test]
async fn the_handshake_is_asked_once_per_server() {
    const METADATA: &str = r#"{"packages":[],"workspace_members":[]}"#;
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "cargo"]))
        .expect(1)
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_header("content-type", "application/x-ndjson")
        .with_body("{\"type\":\"done\",\"lockfile\":\"version = 4\\n\"}\n")
        .expect(2)
        .create_async()
        .await;
    let config = config_for_pnpr(&server.url());

    for _ in 0..2 {
        resolve_via_pnpr(&config, METADATA).await.unwrap().expect("the server resolves Cargo");
    }

    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn an_offline_install_does_not_reach_the_pnpr_server() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server.mock("GET", "/-/pnpr").expect(0).create_async().await;
    let mut config = config_for_pnpr(&server.url());
    config.offline = true;

    let lockfile = resolve_via_pnpr(&config, "{}").await.unwrap();

    assert_eq!(lockfile, None);
    handshake.assert_async().await;
}
