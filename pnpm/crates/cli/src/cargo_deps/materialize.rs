use super::{
    Algorithm, Arc, ArchiveStoreProjection, AtomicU8, AuthHeaders, BTreeMap, CafsFileInfo, Config,
    HashMap, ImportIndexedDirOpts, IngestTarballToStore, Integrity, IntoDiagnostic, LockedCrate,
    PathBuf, Reporter, Result, RetryOpts, Serialize, SharedReadonlyStoreIndex,
    SharedVerifiedFilesCache, StoreDir, StoreIndex, StoreIndexWriter, StreamExt, ThrottledClient,
    checksum_cache, import_indexed_dir, join_all_buffered, registry_download_config, stream,
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
    let concurrency = config.network_concurrency.clamp(1, 16);

    let materializations = stream::iter(options.packages).map(|package| {
        materialize::<Reporter>(MaterializeOptions {
            package,
            store_dir,
            store_index: store_index.as_ref().map(Arc::clone),
            store_index_writer: Arc::clone(&store_index_writer),
            http_client: Arc::clone(&options.http_client),
            auth_headers: Arc::clone(&auth_headers),
            download_template: registry_config.dl.clone(),
            verified_files_cache: Arc::clone(&verified_files_cache),
            logged_methods: Arc::clone(&options.logged_methods),
            package_import_method: config.package_import_method,
            retry_opts: config.retry_opts(),
            verify_store_integrity: config.verify_store_integrity,
            strict_store_pkg_content_check: config.strict_store_pkg_content_check,
            offline: config.offline,
            requester: options.requester.clone(),
        })
    });
    let slots = join_all_buffered(materializations, concurrency).await;
    drop(store_index_writer);
    StoreIndexWriter::drain(writer_task, "; some Cargo rows may not be persisted").await;
    slots
}

pub(super) struct MaterializeOptions {
    pub(super) package: LockedCrate,
    pub(super) store_dir: &'static StoreDir,
    pub(super) store_index: Option<SharedReadonlyStoreIndex>,
    pub(super) store_index_writer: Arc<StoreIndexWriter>,
    pub(super) http_client: Arc<ThrottledClient>,
    pub(super) auth_headers: Arc<AuthHeaders>,
    pub(super) download_template: String,
    pub(super) verified_files_cache: SharedVerifiedFilesCache,
    pub(super) logged_methods: Arc<AtomicU8>,
    pub(super) package_import_method: pnpm_config::PackageImportMethod,
    pub(super) retry_opts: RetryOpts,
    pub(super) verify_store_integrity: bool,
    pub(super) strict_store_pkg_content_check: bool,
    pub(super) offline: bool,
    pub(super) requester: String,
}

pub(super) async fn materialize<Reporter: self::Reporter + 'static>(
    options: MaterializeOptions,
) -> Result<(String, PathBuf)> {
    let link_name = options.package.link_name();
    let slot = options.package.store_slot(options.store_dir.root());
    let mut cas_paths = ingest_crate::<Reporter>(&options).await?;

    let slot_for_import = slot.clone();
    tokio::task::spawn_blocking(move || {
        checksum_cache::ChecksumCache {
            store_dir: options.store_dir,
            index: options.store_index.as_ref(),
            writer: &options.store_index_writer,
            verified_files: &options.verified_files_cache,
        }
        .add(&mut cas_paths, &options.package.checksum)?;
        import_indexed_dir::<Reporter>(
            &options.logged_methods,
            options.package_import_method,
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
        &options.download_template,
        &options.package.name,
        &options.package.version,
        &options.package.checksum,
    );
    let package_id = format!("crate:{}@{}", options.package.name, options.package.version);
    let integrity = Integrity::from_hex(&options.package.checksum, Algorithm::Sha256)
        .into_diagnostic()
        .wrap_err_with(|| format!("decode checksum for {package_id}"))?;
    let cas_paths = IngestTarballToStore {
        http_client: &options.http_client,
        store_dir: options.store_dir,
        store_index: options.store_index.as_ref().map(Arc::clone),
        store_index_writer: Some(Arc::clone(&options.store_index_writer)),
        verify_store_integrity: options.verify_store_integrity,
        strict_store_pkg_content_check: options.strict_store_pkg_content_check,
        verified_files_cache: Arc::clone(&options.verified_files_cache),
        package_integrity: Some(&integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &package_url,
        package_id: &package_id,
        auth_headers: &options.auth_headers,
        requester: &options.requester,
        prefetched_cas_paths: None,
        retry_opts: options.retry_opts,
        ignore_file_pattern: None,
        offline: options.offline,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::RawArchive,
    }
    .run_without_mem_cache::<Reporter>()
    .await
    .into_diagnostic()
    .wrap_err_with(|| format!("download {package_id}"))?;

    Ok(cas_paths)
}
