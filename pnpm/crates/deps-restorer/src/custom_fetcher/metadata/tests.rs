use super::{CustomFetchOutcome, CustomFetcherSession, LockfileResolution};
use pnpm_tarball::{
    ArchiveFetchOptions, ArchiveStoreContext, ArchiveStoreProjection, IngestTarballToStore,
    RetryOpts, TarballPackage,
};
use serde_json::Value;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// A custom fetcher that delegates every package to a local tarball
/// fixture and counts how often the hook ran.
struct CountingDelegateFetcher {
    fetches: AtomicUsize,
    tarball_url: String,
    integrity: String,
}

#[async_trait::async_trait]
impl pnpm_hooks::CustomFetcher for CountingDelegateFetcher {
    async fn can_fetch(
        &self,
        _pkg_id: &str,
        _resolution: Value,
    ) -> Result<bool, pnpm_hooks::HookError> {
        Ok(true)
    }

    async fn fetch(
        &self,
        _pkg_id: &str,
        _resolution: Value,
        _opts: Value,
    ) -> Result<Value, pnpm_hooks::HookError> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({
            "delegate": {
                "tarball": self.tarball_url,
                "integrity": self.integrity,
            },
        }))
    }
}

fn fixture_tarball(name: &str, version: &str) -> Vec<u8> {
    let manifest = serde_json::json!({ "name": name, "version": version }).to_string();
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_path("package/package.json").unwrap();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append(&header, manifest.as_bytes()).unwrap();
    let tar_bytes = builder.into_inner().unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    use std::io::Write as _;
    encoder.write_all(&tar_bytes).unwrap();
    encoder.finish().unwrap()
}

fn leaked_config(store_dir: &Path) -> &'static pnpm_config::Config {
    let mut config = pnpm_config::Config::new();
    config.store_dir = store_dir.to_path_buf().into();
    Box::leak(Box::new(config))
}

fn ingest<'a>(
    config: &'static pnpm_config::Config,
    http_client: &'a pnpm_network::ThrottledClient,
    url: &'a str,
    id: &'a str,
    integrity: Option<&'a ssri::Integrity>,
) -> IngestTarballToStore<'a> {
    IngestTarballToStore {
        fetching: ArchiveFetchOptions {
            http_client,
            auth_headers: config.auth_headers.as_ref(),
            retry_opts: RetryOpts { retries: 0, ..Default::default() },
            offline: true,
        },
        package: TarballPackage { integrity, unpacked_size: None, file_count: None, url, id },
        store: ArchiveStoreContext {
            dir: &config.store_dir,
            index: None,
            index_writer: None,
            verify_integrity: config.verify_store_integrity,
            strict_pkg_content_check: config.strict_store_pkg_content_check,
            verified_files_cache: pnpm_store_dir::SharedVerifiedFilesCache::default(),
            prefetched_cas_paths: None,
        },
        requester: "",
        ignore_file_pattern: None,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
}

/// A pnpmfile `resolvers` hook leaves `ResolvedPackageInfo::name_ver`
/// unset, so the resolve pass files its custom fetch under the tarball
/// URL while the install pass looks the session up under the
/// lockfile-derived `name@version`. The fetch must be filed under the
/// `name@version` read from the fetched manifest too, so the hook runs
/// once per package per install, not twice.
/// <https://github.com/pnpm/pnpm/issues/15025>
#[tokio::test]
async fn resolve_time_custom_fetch_is_reused_by_the_install_pass() {
    let dir = tempfile::tempdir().unwrap();
    let body = fixture_tarball("foo", "1.0.0");
    let tarball_path = dir.path().join("foo.tgz");
    std::fs::write(&tarball_path, &body).unwrap();
    let integrity = ssri::Integrity::from(&body).to_string();

    let fetcher = Arc::new(CountingDelegateFetcher {
        fetches: AtomicUsize::new(0),
        tarball_url: format!("file:{}", tarball_path.display()),
        integrity: integrity.clone(),
    });
    let custom_fetcher = Arc::clone(&fetcher) as Arc<dyn pnpm_hooks::CustomFetcher>;
    let session = CustomFetcherSession::new(vec![custom_fetcher]);

    let config = leaked_config(&dir.path().join("store"));
    let http_client = pnpm_network::ThrottledClient::default();

    // Resolve pass: no `name_ver`, so the session id is the tarball URL.
    let resolve_url = "https://registry.example/foo.tgz";
    let original = LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
        tarball: resolve_url.to_string(),
        integrity: None,
        revision: None,
        git_hosted: None,
        path: None,
    });
    let metadata = session
        .resolve_tarball_metadata::<pnpm_reporter::SilentReporter>(
            ingest(config, &http_client, resolve_url, resolve_url, None),
            &original,
            serde_json::json!({ "lockfileDir": dir.path() }),
            config,
        )
        .await
        .expect("the resolve-time custom fetch succeeds");
    assert_eq!(fetcher.fetches.load(Ordering::SeqCst), 1);
    let manifest = metadata.manifest.expect("the fetched archive has a manifest");
    assert_eq!(manifest["name"], serde_json::json!("foo"));

    // Install pass: the lockfile entry is registry-shaped, so the
    // session is looked up under `name@version`.
    let locked: ssri::Integrity = integrity.parse().unwrap();
    let outcome = session
        .fetch::<pnpm_reporter::SilentReporter>(
            ingest(config, &http_client, resolve_url, "foo@1.0.0", Some(&locked)),
            &metadata.resolution,
            serde_json::json!({}),
        )
        .await
        .expect("the install-time session lookup succeeds");
    assert!(
        matches!(outcome, CustomFetchOutcome::Fetched { .. }),
        "the install pass must reuse the resolve-time fetch instead of running the hook again",
    );
    assert_eq!(
        fetcher.fetches.load(Ordering::SeqCst),
        1,
        "the custom fetcher hook ran twice for one package",
    );
}
