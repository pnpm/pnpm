//! End-to-end cache-gc, startup orphan pruning, and TTL/LRU eviction tests for
//! Cargo, Python, and OCI upstream caches.

use super::create_file_with_age;
use crate::ecosystem::{
    body_bytes,
    sha256_hex,
};
use axum::{
    Router,
    body::Body,
    http::{
        Request,
        StatusCode,
    },
};
use pnpr::{
    AuthState,
    Command,
    Config,
    cache_maintenance::{
        CacheGcEffect,
        CacheGcOptions,
        CacheGcScope,
        active_cache_namespaces,
        collect_cache,
        prune_orphaned_namespaces_startup,
    },
    router_with_auth,
    run_command,
};
use serde_json::json;
use std::{
    net::{
        Ipv4Addr,
        SocketAddr,
        SocketAddrV4,
    },
    path::Path,
    time::Duration,
};
use tempfile::TempDir;
use tower::ServiceExt;

fn oci_manifest_bytes(config_payload: &[u8], layer_payload: &[u8]) -> Vec<u8> {
    let config_digest = format!("sha256:{}", sha256_hex(config_payload));
    let layer_digest = format!("sha256:{}", sha256_hex(layer_payload));
    serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": { "digest": config_digest, "size": config_payload.len() },
        "layers": [{ "digest": layer_digest, "size": layer_payload.len() }],
    }))
    .unwrap()
}

fn write_ecosystem_config(
    tmp: &TempDir,
    config_name: &str,
    upstream_url: Option<&str>,
) -> (Config, Config) {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 7677));
    let public_url = "http://pnpr.test".to_string();
    let storage = tmp.path().join("storage");
    let cache = tmp.path().join("cache");
    let registries_block = match upstream_url {
        Some(url) => format!(
            r"registries:
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
  crates-up:
    type: upstream
    ecosystem: cargo
    url: {url}/
    public: true
  pypi-up:
    type: upstream
    ecosystem: pypi
    url: {url}/
    public: true
  oci-up:
    type: upstream
    ecosystem: oci
    url: {url}/
    public: true
defaultRegistry:
  npm: npmjs
  cargo: crates-up
  pypi: pypi-up
  oci: oci-up
",
        ),
        None => r"registries:
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
defaultRegistry:
  npm: npmjs
"
        .to_string(),
    };
    let yaml = format!(
        r"storage: {storage}
cache: {cache}
secret: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
packumentTtl: 1h
routes:
  allowedPrivateNetworks:
    - 127.0.0.0/8
    - ::1/128
{registries_block}",
        storage = storage.display(),
        cache = cache.display(),
    );
    let config_path = tmp.path().join(config_name);
    std::fs::write(&config_path, yaml).unwrap();
    let seeded = pnpr_fixtures::load_seeded_config(&config_path, listen, Some(public_url.clone()));
    let cli_config = Config::from_yaml(&config_path, listen, Some(public_url)).unwrap();
    (seeded, cli_config)
}

async fn get_ok_bytes(app: &Router, path: &str) -> Vec<u8> {
    let response = app
        .clone()
        .oneshot(
            Request::get(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = body_bytes(response.into_body()).await;
    assert_eq!(status, StatusCode::OK, "GET {path} failed with {status}");
    bytes
}

#[tokio::test]
async fn cache_gc_and_startup_retain_active_cargo_pypi_and_oci_upstreams_then_prune_when_removed() {
    let mut upstream = mockito::Server::new_async().await;
    let crate_bytes = b"fake-crate-archive-bytes";
    let wheel_bytes = b"fake-wheel-archive-bytes";
    let metadata_bytes = b"Metadata-Version: 2.1\nName: requests\nVersion: 1.0.0\n";
    let oci_layer = b"fake-oci-layer-bytes";
    let oci_layer_digest = format!("sha256:{}", sha256_hex(oci_layer));
    let oci_manifest = oci_manifest_bytes(b"{}", oci_layer);

    let cargo_config = upstream
        .mock("GET", "/config.json")
        .with_body(
            json!({
                "dl": format!("{}/api/v1/crates", upstream.url()),
                "api": upstream.url(),
            })
            .to_string(),
        )
        .create_async()
        .await;
    let cargo_index = upstream
        .mock("GET", "/my/cr/mycrate")
        .with_body(format!(
            "{}\n",
            json!({
                "name": "mycrate",
                "vers": "1.0.0",
                "deps": [],
                "cksum": sha256_hex(crate_bytes),
                "features": {},
                "yanked": false,
            }),
        ))
        .expect(1)
        .create_async()
        .await;
    let cargo_download = upstream
        .mock("GET", "/api/v1/crates/mycrate/1.0.0/download")
        .with_body(crate_bytes.as_slice())
        .expect(1)
        .create_async()
        .await;

    let wheel_filename = "requests-1.0.0-py3-none-any.whl";
    let pypi_page = upstream
        .mock("GET", "/requests/")
        .with_body(
            json!({
                "meta": { "api-version": "1.0" },
                "name": "requests",
                "files": [{
                    "filename": wheel_filename,
                    "url": format!("{}/packages/{wheel_filename}", upstream.url()),
                    "hashes": { "sha256": sha256_hex(wheel_bytes) },
                    "core-metadata": { "sha256": sha256_hex(metadata_bytes) },
                }],
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let pypi_wheel = upstream
        .mock("GET", format!("/packages/{wheel_filename}").as_str())
        .with_body(wheel_bytes.as_slice())
        .expect(1)
        .create_async()
        .await;
    let pypi_metadata = upstream
        .mock("GET", format!("/packages/{wheel_filename}.metadata").as_str())
        .with_body(metadata_bytes.as_slice())
        .expect(1)
        .create_async()
        .await;

    let oci_manifest_mock = upstream
        .mock("GET", "/v2/library/alpine/manifests/1.0")
        .with_header("content-type", "application/vnd.oci.image.manifest.v1+json")
        .with_body(oci_manifest.clone())
        .expect(1)
        .create_async()
        .await;
    let oci_blob_mock = upstream
        .mock("GET", format!("/v2/library/alpine/blobs/{oci_layer_digest}").as_str())
        .with_body(oci_layer.as_slice())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let (config, cli_config) = write_ecosystem_config(&tmp, "pnpr.yaml", Some(&upstream.url()));
    let app = router_with_auth(config.clone(), AuthState::in_memory());

    let cargo_url = "/cargo/~crates-up/api/v1/crates/mycrate/1.0.0/download";
    let wheel_url = format!("/pypi/~pypi-up/files/requests/{wheel_filename}");
    let metadata_url = format!("/pypi/~pypi-up/files/requests/{wheel_filename}.metadata");
    let manifest_url = "/v2/library/alpine/manifests/1.0";
    let blob_url = format!("/v2/library/alpine/blobs/{oci_layer_digest}");

    assert_eq!(get_ok_bytes(&app, cargo_url).await, crate_bytes);
    assert_eq!(get_ok_bytes(&app, &wheel_url).await, wheel_bytes);
    assert_eq!(get_ok_bytes(&app, &metadata_url).await, metadata_bytes);
    assert_eq!(get_ok_bytes(&app, manifest_url).await, oci_manifest);
    assert_eq!(get_ok_bytes(&app, &blob_url).await, oci_layer);

    cargo_index.assert_async().await;
    cargo_download.assert_async().await;
    pypi_page.assert_async().await;
    pypi_wheel.assert_async().await;
    pypi_metadata.assert_async().await;
    oci_manifest_mock.assert_async().await;
    oci_blob_mock.assert_async().await;

    for mock in [
        cargo_config,
        cargo_index,
        cargo_download,
        pypi_page,
        pypi_wheel,
        pypi_metadata,
        oci_manifest_mock,
        oci_blob_mock,
    ] {
        mock.remove_async().await;
    }
    drop(upstream);

    let public_dirs_count = count_subdirs(&config.storage.cache_dir.join("~public"));
    assert_eq!(
        public_dirs_count, 3,
        "each populated public upstream owns one directory under ~public",
    );

    run_command(
        &cli_config,
        Command::CacheGc { dry_run: false, max_size: None, max_age: None, orphans_only: true },
    )
    .await
    .unwrap();

    let startup_report = prune_orphaned_namespaces_startup(&config).await.unwrap();
    assert_eq!(startup_report.namespaces_removed, 0);
    assert_eq!(startup_report.files_evicted, 0);

    let periodic_report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: Some(10 * 1024 * 1024),
            max_age: Some(Duration::from_hours(24)),
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    assert_eq!(periodic_report.namespaces_removed, 0);
    assert_eq!(periodic_report.files_evicted, 0);

    assert_eq!(get_ok_bytes(&app, cargo_url).await, crate_bytes);
    assert_eq!(get_ok_bytes(&app, &wheel_url).await, wheel_bytes);
    assert_eq!(get_ok_bytes(&app, &metadata_url).await, metadata_bytes);
    assert_eq!(get_ok_bytes(&app, manifest_url).await, oci_manifest);
    assert_eq!(get_ok_bytes(&app, &blob_url).await, oci_layer);

    let (reduced_config, reduced_cli_config) = write_ecosystem_config(&tmp, "reduced.yaml", None);
    let dry_prune = collect_cache(
        &reduced_config,
        &CacheGcOptions {
            effect: CacheGcEffect::DryRun,
            max_size: None,
            max_age: None,
            scope: CacheGcScope::OrphansOnly,
        },
    )
    .await
    .unwrap();
    assert_eq!(dry_prune.namespaces_removed, 3);
    assert_eq!(dry_prune.files_evicted, 8);

    run_command(
        &reduced_cli_config,
        Command::CacheGc { dry_run: false, max_size: None, max_age: None, orphans_only: true },
    )
    .await
    .unwrap();
    assert_eq!(count_subdirs(&reduced_config.storage.cache_dir.join("~public")), 0);
}

fn count_subdirs(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .count()
}

#[tokio::test]
async fn ttl_and_lru_eviction_cover_nested_ecosystem_cache_subtrees() {
    let tmp = TempDir::new().unwrap();
    let (config, _) = write_ecosystem_config(&tmp, "pnpr.yaml", Some("http://127.0.0.1:9"));
    let cache_dir = &config.storage.cache_dir;
    let active_ns = active_cache_namespaces(&config)
        .into_iter()
        .find(|ns| ns.starts_with("~public/"))
        .unwrap();

    let expired_artifact =
        cache_dir.join(&active_ns).join("artifacts/deadbeef/packages/@/serde/blobs/1.0.0.crate");
    let older_metadata =
        cache_dir.join(&active_ns).join("metadata/cafebabe/packages/@/requests/document.json");
    let newer_oci_blob =
        cache_dir.join(&active_ns).join("oci-blobs/packages/@library/alpine/blobs/sha256-1234");
    let fresh_manifest = cache_dir
        .join(&active_ns)
        .join("oci-manifests/01234567/packages/@library/alpine/document.json");

    create_file_with_age(&expired_artifact, &[0xAA; 500], Duration::from_hours(48));
    create_file_with_age(&older_metadata, &[0xBB; 500], Duration::from_mins(20));
    create_file_with_age(&newer_oci_blob, &[0xCC; 500], Duration::from_mins(5));
    create_file_with_age(&fresh_manifest, &[0xDD; 500], Duration::from_mins(1));

    let marker = cache_dir.join(pnpr_config::CACHE_SECRET_MARKER);
    std::fs::create_dir_all(cache_dir).unwrap();
    std::fs::write(&marker, pnpr_config::cache_secret_fingerprint(&config.cache_secret)).unwrap();
    let marker_len = std::fs::metadata(&marker).unwrap().len();

    let report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: Some(marker_len + 1100),
            max_age: Some(Duration::from_hours(24)),
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    assert_eq!(report.namespaces_removed, 0);
    assert_eq!(report.files_evicted, 2);
    let artifacts_dir = cache_dir.join(&active_ns).join("artifacts");
    let metadata_dir = cache_dir.join(&active_ns).join("metadata");
    for evicted_path in [&expired_artifact, &artifacts_dir, &older_metadata, &metadata_dir] {
        eprintln!("EVICTED PATH: {}", evicted_path.display());
        assert!(!evicted_path.exists());
    }
    for retained_path in [&newer_oci_blob, &fresh_manifest] {
        eprintln!("RETAINED PATH: {}", retained_path.display());
        assert!(retained_path.exists());
    }
}
