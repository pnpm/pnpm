use super::{
    Algorithm, Arc, ArchiveStoreProjection, AtomicU8, AuthHeaders, BTreeMap, CafsFileInfo, Config,
    HashMap, ImportIndexedDirOpts, IngestTarballToStore, Integrity, IntoDiagnostic, LockedCrate,
    PathBuf, Reporter, Result, RetryOpts, Serialize, SharedReadonlyStoreIndex,
    SharedVerifiedFilesCache, StoreDir, StoreIndex, StoreIndexWriter, StreamExt, ThrottledClient,
    checksum_cache, import_indexed_dir, registry_download_config, stream,
};
use miette::WrapErr;

#[derive(Serialize)]
struct CargoChecksum<'a> {
    files: BTreeMap<String, String>,
    /// The registry checksum of the `.crate` archive the files came from.
    /// Null for a package vendored from a git checkout, which has none —
    /// the same value `cargo vendor` writes for one.
    package: Option<&'a str>,
}

pub(super) struct DownloadOptions {
    pub(super) config: &'static Config,
    pub(super) packages: Vec<LockedCrate>,
    pub(super) http_client: Arc<ThrottledClient>,
    pub(super) logged_methods: Arc<AtomicU8>,
    pub(super) requester: String,
}

/// Download every registry crate the lockfile pins into its store slot.
/// Returns before the registry's configuration is fetched when the lockfile
/// pins none, so a workspace that takes nothing from the registry never
/// asks it for anything.
pub(super) async fn download_crates<Reporter: self::Reporter + 'static>(
    options: DownloadOptions,
) -> Result<Vec<(String, PathBuf)>> {
    if options.packages.is_empty() {
        return Ok(Vec::new());
    }
    let config = options.config;
    let store_dir = &config.store_dir;
    let store_index = StoreIndex::shared_for(store_dir, config.frozen_store);
    let (store_index_writer, writer_task) =
        StoreIndexWriter::spawn_for(store_dir, config.frozen_store);

    let (registry_config, auth_headers) =
        registry_download_config(config, &options.http_client).await?;
    let verified_files_cache = SharedVerifiedFilesCache::default();

    let fetching = CrateDownload::new(&options, registry_config.dl, auth_headers);
    let store = CrateStore {
        dir: store_dir,
        index: store_index,
        index_writer: Arc::clone(&store_index_writer),
        verified_files_cache,
        logged_methods: options.logged_methods,
        import_method: config.package_import_method,
        verify_integrity: config.verify_store_integrity,
        strict_pkg_content_check: config.strict_store_pkg_content_check,
    };
    let slots = stream::iter(options.packages)
        .map(|package| {
            materialize::<Reporter>(MaterializeOptions {
                package,
                fetching: fetching.clone(),
                store: store.clone(),
            })
        })
        .buffer_unordered(config.network_concurrency.clamp(1, 16))
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>();
    drop(store);
    drop(store_index_writer);
    StoreIndexWriter::drain(writer_task, "; some Cargo rows may not be persisted").await;
    slots
}

pub(super) struct MaterializeOptions {
    pub(super) package: LockedCrate,
    pub(super) fetching: CrateDownload,
    pub(super) store: CrateStore,
}

#[derive(Clone)]
pub(crate) struct CrateDownload {
    pub(super) http_client: Arc<ThrottledClient>,
    pub(super) auth_headers: Arc<AuthHeaders>,
    pub(super) download_template: String,
    pub(super) retry_opts: RetryOpts,
    pub(super) offline: bool,
    pub(super) requester: String,
}

#[derive(Clone)]
pub(crate) struct CrateStore {
    pub(super) dir: &'static StoreDir,
    pub(super) index: Option<SharedReadonlyStoreIndex>,
    pub(super) index_writer: Arc<StoreIndexWriter>,
    pub(super) verified_files_cache: SharedVerifiedFilesCache,
    pub(super) logged_methods: Arc<AtomicU8>,
    pub(super) import_method: pnpm_config::PackageImportMethod,
    pub(super) verify_integrity: bool,
    pub(super) strict_pkg_content_check: bool,
}

pub(super) async fn materialize<Reporter: self::Reporter + 'static>(
    options: MaterializeOptions,
) -> Result<(String, PathBuf)> {
    let link_name = options.package.link_name();
    let slot = options.package.store_slot(options.store.dir.root());
    let mut cas_paths = ingest_crate::<Reporter>(&options).await?;

    let slot_for_import = slot.clone();
    tokio::task::spawn_blocking(move || {
        checksum_cache::ChecksumCache {
            store_dir: options.store.dir,
            index: options.store.index.as_ref(),
            writer: &options.store.index_writer,
            verified_files: &options.store.verified_files_cache,
        }
        .add(&mut cas_paths, &options.package.checksum)?;
        import_indexed_dir::<Reporter>(
            &options.store.logged_methods,
            options.store.import_method,
            &slot_for_import,
            &cas_paths,
            ImportIndexedDirOpts {
                force: true,
                safe_to_skip: true,
                ..ImportIndexedDirOpts::default()
            },
        )
        .into_diagnostic()
        .wrap_err_with(|| format!("materialize cargo package at {}", slot_for_import.display()))
    })
    .await
    .into_diagnostic()
    .wrap_err("join cargo package materialization task")??;
    Ok((link_name, slot))
}

pub(super) fn add_cargo_checksum(
    store_dir: &StoreDir,
    cas_paths: &mut HashMap<String, PathBuf>,
    package_checksum: Option<&str>,
) -> Result<CafsFileInfo> {
    cas_paths.remove(".cargo-checksum.json");
    let files = cas_paths
        .iter()
        .map(|(path, cas_path)| {
            pnpm_crypto_hash::create_hex_hash_from_file(cas_path)
                .into_diagnostic()
                .wrap_err_with(|| format!("hash cargo package file {path}"))
                .map(|checksum| (path.clone(), checksum))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let checksum = serde_json::to_vec(&CargoChecksum { files, package: package_checksum })
        .into_diagnostic()
        .wrap_err("serialize .cargo-checksum.json")?;
    let (cas_path, hash) = store_dir
        .write_cas_file(&checksum, false)
        .into_diagnostic()
        .wrap_err("store .cargo-checksum.json")?;
    cas_paths.insert(".cargo-checksum.json".to_string(), cas_path);
    Ok(CafsFileInfo {
        digest: format!("{hash:x}"),
        mode: 0o644,
        size: checksum.len() as u64,
        checked_at: None,
    })
}

async fn ingest_crate<Reporter: self::Reporter + 'static>(
    options: &MaterializeOptions,
) -> Result<HashMap<String, PathBuf>> {
    let package_url = pnpm_cargo_resolver::download_url(
        &options.fetching.download_template,
        &options.package.name,
        &options.package.version,
        &options.package.checksum,
    );
    let package_id = format!("crate:{}@{}", options.package.name, options.package.version);
    let integrity = Integrity::from_hex(&options.package.checksum, Algorithm::Sha256)
        .into_diagnostic()
        .wrap_err_with(|| format!("decode checksum for {package_id}"))?;
    let cas_paths = IngestTarballToStore {
        fetching: options.fetching.archive_options(),
        package: pnpm_tarball::TarballPackage {
            integrity: Some(&integrity),
            unpacked_size: None,
            file_count: None,
            url: &package_url,
            id: &package_id,
        },
        store: options.store.archive_context(),

        requester: &options.fetching.requester,

        ignore_file_pattern: None,

        progress_reported: None,
        store_projection: ArchiveStoreProjection::RawArchive,
    }
    .run_without_mem_cache::<Reporter>()
    .await
    .into_diagnostic()
    .wrap_err_with(|| format!("download {package_id}"))?;

    Ok(cas_paths)
}

impl CrateDownload {
    fn new(
        options: &DownloadOptions,
        download_template: String,
        auth_headers: Arc<AuthHeaders>,
    ) -> Self {
        Self {
            http_client: Arc::clone(&options.http_client),
            auth_headers,
            download_template,
            retry_opts: options.config.retry_opts(),
            offline: options.config.offline,
            requester: options.requester.clone(),
        }
    }

    fn archive_options(&self) -> pnpm_tarball::ArchiveFetchOptions<'_> {
        pnpm_tarball::ArchiveFetchOptions {
            http_client: &self.http_client,
            auth_headers: &self.auth_headers,
            retry_opts: self.retry_opts,
            offline: self.offline,
        }
    }
}

impl CrateStore {
    fn archive_context(&self) -> pnpm_tarball::ArchiveStoreContext<'_> {
        pnpm_tarball::ArchiveStoreContext {
            dir: self.dir,
            index: self.index.as_ref().map(Arc::clone),
            index_writer: Some(Arc::clone(&self.index_writer)),
            verify_integrity: self.verify_integrity,
            strict_pkg_content_check: self.strict_pkg_content_check,
            verified_files_cache: Arc::clone(&self.verified_files_cache),
            prefetched_cas_paths: None,
            fallback_dir: None,
        }
    }
}
