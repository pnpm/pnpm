use super::{
    BTreeMap, ConfigDepError, EnvLockfile, SilentReporter, TempDir, build_resolver, clean_spec,
    harness, install_config_deps, integrity_of, options, resolve_and_install_config_deps,
    tarball_url_of,
};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_store_dir::{SharedVerifiedFilesCache, StoreDir, StoreIndexWriter};
use std::sync::Arc;

fn temp_store(root: &TempDir) -> &'static StoreDir {
    Box::leak(Box::new(StoreDir::new(root.path().to_path_buf())))
}

/// Index `name@version` in `store` the way a regular install does.
async fn seed_store(store: &'static StoreDir, tarball: &str, integrity: &str, package_id: &str) {
    let integrity: ssri::Integrity = integrity.parse().unwrap();
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let (writer, writer_task) = StoreIndexWriter::spawn(store);
    pnpm_tarball::IngestTarballToStore {
        fetching: pnpm_tarball::ArchiveFetchOptions {
            http_client: &client,
            auth_headers: &auth_headers,
            retry_opts: RetryOpts::default(),
            offline: false,
        },
        package: pnpm_tarball::TarballPackage {
            integrity: Some(&integrity),
            unpacked_size: None,
            file_count: None,
            url: tarball,
            id: package_id,
        },
        store: pnpm_tarball::ArchiveStoreContext {
            dir: store,
            index: None,
            index_writer: Some(Arc::clone(&writer)),
            verify_integrity: true,
            strict_pkg_content_check: true,
            verified_files_cache: SharedVerifiedFilesCache::default(),
            prefetched_cas_paths: None,
            fallback_dir: None,
        },
        requester: "",
        ignore_file_pattern: None,
        progress_reported: None,
        store_projection: pnpm_tarball::ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .unwrap();
    drop(writer);
    writer_task.await.unwrap().unwrap();
}

/// <https://github.com/pnpm/pnpm/issues/3392>
#[tokio::test]
async fn offline_config_dep_install_copies_from_the_fallback_store() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();
    let lockfile_store_root = TempDir::new().unwrap();
    let fallback_root = TempDir::new().unwrap();
    let fallback = temp_store(&fallback_root);

    let mut config_deps = BTreeMap::new();
    config_deps.insert("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0"));
    let mut lockfile_options = options(&harness, root.path(), false);
    lockfile_options.store.dir = temp_store(&lockfile_store_root);
    resolve_and_install_config_deps::<SilentReporter>(&config_deps, &resolver, &lockfile_options)
        .await
        .unwrap();
    std::fs::remove_dir_all(root.path().join("node_modules")).unwrap();
    seed_store(
        fallback,
        &tarball_url_of(&resolver, "@pnpm.e2e/foo", "100.0.0").await,
        &integrity_of(&resolver, "@pnpm.e2e/foo", "100.0.0").await,
        "@pnpm.e2e/foo@100.0.0",
    )
    .await;
    let env_lockfile = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");

    let mut offline = options(&harness, root.path(), true);
    offline.fetching.offline = true;
    let without_fallback = install_config_deps::<SilentReporter>(&env_lockfile, &offline)
        .await
        .expect_err("an empty store cannot serve an offline install");
    assert!(matches!(without_fallback, ConfigDepError::DownloadTarball(_)), "{without_fallback:?}");

    offline.store.fallback_dir = Some(fallback);
    install_config_deps::<SilentReporter>(&env_lockfile, &offline).await
        .expect("the fallback store should stand in for the download");

    let installed = root.path().join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json");
    assert!(installed.is_file(), "config dep must be linked into .pnpm-config");
}
