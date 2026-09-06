use crate::ecosystem_install::{EcosystemManifest, EcosystemWorkspaceInventory, InstallContext};
use futures_util::{StreamExt, TryStreamExt, stream};
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_cargo_resolver::CRATES_IO_SPARSE_INDEX;
use pnpm_config::Config;
use pnpm_deps_restorer::{ImportIndexedDirOpts, import_indexed_dir};
use pnpm_install_coordinator::{InstallTask, PreparedInstall};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_pnpr_client::{CargoResolveOptions, PnprClient};
use pnpm_reporter::Reporter;
use pnpm_store_dir::{
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir, StoreIndex, StoreIndexWriter,
};
use pnpm_tarball::{ArchiveStoreProjection, IngestTarballToStore};
use serde::{Deserialize, Serialize};
use ssri::{Algorithm, Integrity};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::{Arc, atomic::AtomicU8},
};

pub(crate) mod add;
mod registry_auth;

#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};

const CRATES_IO_DOWNLOAD_BASE: &str = "https://static.crates.io/crates";
const WORKSPACE_INSTALL_CONCURRENCY: usize = 8;
const MANAGED_START: &str = "# >>> pnpm-managed cargo sources >>>";
const MANAGED_END: &str = "# <<< pnpm-managed cargo sources <<<";
const MANAGED_CONFIG: &str = "# >>> pnpm-managed cargo sources >>>\n[source.crates-io]\nreplace-with = \"pnpm-crates-io\"\n\n[source.pnpm-crates-io]\ndirectory = \".pnpm/crates/crates-io\"\n# <<< pnpm-managed cargo sources <<<";
#[cfg(unix)]
static MANAGED_TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CargoLockfilePolicy {
    UseExisting,
    Resolve,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LockedCrate {
    name: String,
    version: String,
    checksum: String,
}

#[derive(Serialize)]
struct CargoChecksum<'a> {
    files: BTreeMap<String, String>,
    package: &'a str,
}

#[derive(Deserialize)]
struct CargoWorkspaceMetadata {
    workspace_root: PathBuf,
}

struct ManagedDirectory {
    path: PathBuf,
    #[cfg(unix)]
    handle: fs::File,
    #[cfg(windows)]
    _pinned_components: Vec<fs::File>,
}

pub(crate) async fn plan<Reporter: self::Reporter + 'static>(
    context: InstallContext,
    inventory: &EcosystemWorkspaceInventory,
) -> Result<InstallTask<'static>> {
    let roots =
        discover_workspace_roots(inventory.manifests(EcosystemManifest::Cargo).await?).await?;
    let metadata = roots.iter().flat_map(|root| metadata_paths(root)).collect();
    Ok(InstallTask::new(
        metadata,
        prepare::<Reporter>(context, roots, CargoLockfilePolicy::UseExisting),
    ))
}

pub(crate) fn metadata_paths(root: &Path) -> [PathBuf; 2] {
    [root.join("Cargo.lock"), root.join(".cargo/config.toml")]
}

pub(crate) async fn prepare<Reporter: self::Reporter + 'static>(
    context: InstallContext,
    roots: Vec<PathBuf>,
    lockfile_policy: CargoLockfilePolicy,
) -> Result<Vec<Prepared>> {
    let InstallContext { config, http_client, lockfile_only, frozen_lockfile } = context;
    let mut prepared = stream::iter(roots)
        .map(|root| {
            let http_client = Arc::clone(&http_client);
            async move {
                prepare_workspace::<Reporter>(
                    config,
                    &root,
                    lockfile_only,
                    frozen_lockfile,
                    lockfile_policy,
                    http_client,
                )
                .await
            }
        })
        .buffer_unordered(WORKSPACE_INSTALL_CONCURRENCY)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    prepared.sort_by(|left, right| left.root.cmp(&right.root));
    Ok(prepared)
}

pub(crate) struct Prepared {
    root: PathBuf,
    lock: String,
    slots: Option<Vec<(String, PathBuf)>>,
}

impl PreparedInstall for Prepared {
    fn publish(&mut self) -> Result<()> {
        if let Some(slots) = &self.slots {
            link_workspace(&self.root, slots)?;
            write_cargo_config(&self.root)?;
        }
        let path = self.root.join("Cargo.lock");
        let existing = match fs::read_to_string(&path) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).into_diagnostic(),
        };
        if existing.as_deref() != Some(&self.lock) {
            pnpm_fs::write_atomic(&path, self.lock.as_bytes())
                .into_diagnostic()
                .wrap_err_with(|| format!("write {}", path.display()))?;
        }
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        // Versioned source links are additive cache entries. Restoring the
        // declared lockfile and source configuration restores project selection.
        Ok(())
    }

    fn retain(self: Box<Self>) {}
}

async fn prepare_workspace<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    root_dir: &Path,
    lockfile_only: bool,
    frozen_lockfile: bool,
    lockfile_policy: CargoLockfilePolicy,
    http_client: Arc<ThrottledClient>,
) -> Result<Prepared> {
    let cargo_lock_path = root_dir.join("Cargo.lock");
    let cargo_lock = read_or_resolve_lockfile(
        config,
        root_dir,
        &cargo_lock_path,
        frozen_lockfile,
        lockfile_policy,
        &http_client,
    )
    .await?;
    if lockfile_only {
        return Ok(Prepared { root: root_dir.to_path_buf(), lock: cargo_lock, slots: None });
    }
    let packages = parse_lockfile(&cargo_lock)
        .wrap_err_with(|| format!("parse {}", cargo_lock_path.display()))?;
    let store_dir = &config.store_dir;
    store_dir
        .init()
        .into_diagnostic()
        .wrap_err_with(|| format!("initialize cargo package store at {}", store_dir.display()))?;
    let store_index = StoreIndex::shared_for(store_dir, config.frozen_store);
    let (store_index_writer, writer_task) =
        StoreIndexWriter::spawn_for(store_dir, config.frozen_store);

    let auth_headers = Arc::clone(&config.auth_headers);
    let verified_files_cache = SharedVerifiedFilesCache::default();
    let logged_methods = Arc::new(AtomicU8::new(0));
    let retry_opts = config.retry_opts();
    let requester = format!("cargo workspace at {}", root_dir.display());
    let concurrency = config.network_concurrency.clamp(1, 16);

    let slots = stream::iter(packages)
        .map(|package| {
            materialize::<Reporter>(MaterializeOptions {
                package,
                store_dir,
                store_index: store_index.clone(),
                store_index_writer: Arc::clone(&store_index_writer),
                http_client: Arc::clone(&http_client),
                auth_headers: Arc::clone(&auth_headers),
                verified_files_cache: Arc::clone(&verified_files_cache),
                logged_methods: Arc::clone(&logged_methods),
                package_import_method: config.package_import_method,
                retry_opts,
                verify_store_integrity: config.verify_store_integrity,
                strict_store_pkg_content_check: config.strict_store_pkg_content_check,
                offline: config.offline,
                requester: requester.clone(),
            })
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>();
    drop(store_index_writer);
    StoreIndexWriter::drain(writer_task, "; some Cargo rows may not be persisted").await;
    let slots = slots?;

    Ok(Prepared { root: root_dir.to_path_buf(), lock: cargo_lock, slots: Some(slots) })
}

pub(crate) async fn workspace_root(manifest_path: &Path) -> Result<PathBuf> {
    let metadata = read_cargo_metadata_for_manifest(manifest_path).await?;
    serde_json::from_str::<CargoWorkspaceMetadata>(&metadata)
        .into_diagnostic()
        .wrap_err("read Cargo workspace root from metadata")
        .map(|metadata| metadata.workspace_root)
}

async fn discover_workspace_roots(manifests: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let roots = stream::iter(manifests.iter().cloned())
        .map(|manifest| async move { workspace_root(&manifest).await })
        .buffer_unordered(8)
        .try_collect::<BTreeSet<_>>()
        .await?;
    Ok(roots.into_iter().collect())
}

async fn read_or_resolve_lockfile(
    config: &Config,
    root_dir: &Path,
    cargo_lock_path: &Path,
    frozen_lockfile: bool,
    lockfile_policy: CargoLockfilePolicy,
    http_client: &Arc<ThrottledClient>,
) -> Result<String> {
    if lockfile_policy == CargoLockfilePolicy::UseExisting {
        match fs::read_to_string(cargo_lock_path) {
            Ok(lockfile) => return Ok(lockfile),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("read {}", cargo_lock_path.display()));
            }
        }
    }
    if frozen_lockfile {
        return Err(miette::miette!(
            "Cargo.lock is absent, but --frozen-lockfile forbids generating it"
        ));
    }

    let metadata = read_cargo_metadata(root_dir).await?;
    if let Some(lockfile) = resolve_via_pnpr(config, &metadata).await? {
        return Ok(lockfile);
    }
    let index_files = fetch_sparse_index(config, &metadata, http_client).await?;
    let lockfile = pnpm_cargo_resolver::resolve_lockfile(&metadata, &index_files)
        .wrap_err("resolve Cargo dependencies")?;
    Ok(lockfile)
}

/// Resolve through the configured pnpr server, which walks the sparse
/// index server-side instead of making this client fetch one index file
/// per crate in the graph.
///
/// `None` when there is no server to ask, or when the one configured
/// resolves npm alone: a server that does not serve Cargo resolution
/// means a local resolve, not a failed install.
async fn resolve_via_pnpr(config: &Config, metadata: &str) -> Result<Option<String>> {
    let Some(pnpr_server) = config.pnpr_server.as_deref().filter(|_| !config.offline) else {
        return Ok(None);
    };
    let client = PnprClient::new(pnpr_server);
    if !crate::pnpr_ecosystems::server_resolves(
        &client,
        pnpr_server,
        pnpm_pnpr_client::CARGO_ECOSYSTEM,
    )
    .await?
    {
        return Ok(None);
    }
    // Only the dependency graph leaves the machine: the rest of a
    // `cargo metadata` document describes local paths the server has no
    // use for.
    let metadata = pnpm_cargo_resolver::resolve_inputs(metadata)
        .wrap_err("reduce cargo metadata for the pnpr server")?;
    client
        .resolve_cargo(CargoResolveOptions {
            metadata,
            registry: CRATES_IO_SPARSE_INDEX.to_string(),
            authorization: config.auth_headers.for_url(pnpr_server),
        })
        .await
        .into_diagnostic()
        .wrap_err("resolve Cargo dependencies through the pnpr server")
        .map(Some)
}

pub(crate) async fn latest_version(
    config: &Config,
    auth_headers: &AuthHeaders,
    name: &str,
    http_client: &ThrottledClient,
) -> Result<String> {
    let cache_dir = config.cache_dir.join("v11").join("cargo-index").join("crates-io");
    let index_file = fetch_sparse_index_file(
        name,
        CRATES_IO_SPARSE_INDEX,
        &cache_dir,
        http_client,
        auth_headers,
        config.offline,
        config.retry_opts(),
    )
    .await?;
    pnpm_cargo_resolver::latest_version(name, &index_file)
        .wrap_err_with(|| format!("select the latest version of crate {name}"))
}

pub(crate) fn crates_io_auth_headers(config: &Config) -> Result<Arc<AuthHeaders>> {
    registry_auth::crates_io::<pnpm_config::Host>(&config.auth_headers, config.offline)
}

async fn read_cargo_metadata(root_dir: &Path) -> Result<String> {
    read_cargo_metadata_for_manifest(&root_dir.join("Cargo.toml")).await
}

async fn read_cargo_metadata_for_manifest(manifest_path: &Path) -> Result<String> {
    let manifest_path = manifest_path.to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        Command::new("cargo")
            .args(["metadata", "--no-deps", "--format-version", "1", "--manifest-path"])
            .arg(&manifest_path)
            .output()
            .map(|output| (manifest_path, output))
    })
    .await
    .into_diagnostic()
    .wrap_err("join Cargo manifest discovery task")?
    .into_diagnostic()
    .wrap_err("run cargo metadata for manifest discovery")?;
    let (manifest_path, output) = output;
    if !output.status.success() {
        let manifest_path = manifest_path.display();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        return Err(miette::miette!("cargo metadata failed for {}: {}", manifest_path, stderr,));
    }
    String::from_utf8(output.stdout).into_diagnostic().wrap_err("decode cargo metadata output")
}

async fn fetch_sparse_index(
    config: &Config,
    metadata: &str,
    http_client: &Arc<ThrottledClient>,
) -> Result<BTreeMap<String, String>> {
    let auth_headers = crates_io_auth_headers(config)?;
    let cache_dir = config.cache_dir.join("v11").join("cargo-index").join("crates-io");
    let mut index_files = BTreeMap::new();

    loop {
        let missing = pnpm_cargo_resolver::missing_index_names(metadata, &index_files)
            .wrap_err("discover Cargo sparse-index files")?;
        if missing.is_empty() {
            return Ok(index_files);
        }
        let fetched = stream::iter(missing)
            .map(|name| {
                let http_client = Arc::clone(http_client);
                let auth_headers = Arc::clone(&auth_headers);
                let cache_dir = cache_dir.clone();
                async move {
                    let contents = fetch_sparse_index_file(
                        &name,
                        CRATES_IO_SPARSE_INDEX,
                        &cache_dir,
                        &http_client,
                        &auth_headers,
                        config.offline,
                        config.retry_opts(),
                    )
                    .await?;
                    Ok::<_, miette::Report>((name, contents))
                }
            })
            .buffer_unordered(config.network_concurrency.clamp(1, 16))
            .try_collect::<Vec<_>>()
            .await?;
        index_files.extend(fetched);
    }
}

async fn fetch_sparse_index_file(
    name: &str,
    sparse_index: &str,
    cache_dir: &Path,
    http_client: &ThrottledClient,
    auth_headers: &AuthHeaders,
    offline: bool,
    retry_opts: RetryOpts,
) -> Result<String> {
    let relative_path = sparse_index_path(name)?;
    let cache_path = cache_dir.join(&relative_path);
    if offline {
        return fs::read_to_string(&cache_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("read cached sparse index entry for {name}"));
    }

    let url = format!("{}/{relative_path}", sparse_index.trim_end_matches('/'));
    let response = http_client
        .get_bytes_with_secure_auth_and_retry(&url, auth_headers, None, retry_opts)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("fetch sparse index entry for {name}"))?;
    if !response.status.is_success() {
        return Err(miette::miette!(
            "fetch sparse index entry for {name} returned HTTP {}",
            response.status,
        ));
    }
    let contents = String::from_utf8(response.body)
        .into_diagnostic()
        .wrap_err_with(|| format!("decode sparse index entry for {name}"))?;
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)
            .into_diagnostic()
            .wrap_err_with(|| format!("create Cargo sparse-index cache at {}", parent.display()))?;
    }
    pnpm_fs::write_atomic(&cache_path, contents.as_bytes())
        .into_diagnostic()
        .wrap_err_with(|| format!("cache sparse index entry for {name}"))?;
    Ok(contents)
}

fn sparse_index_path(name: &str) -> Result<String> {
    let name = name.to_ascii_lowercase();
    validate_package_field("crate name", &name, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
    })?;
    Ok(match name.len() {
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[..1]),
        _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
    })
}

struct MaterializeOptions {
    package: LockedCrate,
    store_dir: &'static StoreDir,
    store_index: Option<SharedReadonlyStoreIndex>,
    store_index_writer: Arc<StoreIndexWriter>,
    http_client: Arc<ThrottledClient>,
    auth_headers: Arc<AuthHeaders>,
    verified_files_cache: SharedVerifiedFilesCache,
    logged_methods: Arc<AtomicU8>,
    package_import_method: pnpm_config::PackageImportMethod,
    retry_opts: RetryOpts,
    verify_store_integrity: bool,
    strict_store_pkg_content_check: bool,
    offline: bool,
    requester: String,
}

async fn materialize<Reporter: self::Reporter + 'static>(
    options: MaterializeOptions,
) -> Result<(String, PathBuf)> {
    let MaterializeOptions {
        package,
        store_dir,
        store_index,
        store_index_writer,
        http_client,
        auth_headers,
        verified_files_cache,
        logged_methods,
        package_import_method,
        retry_opts,
        verify_store_integrity,
        strict_store_pkg_content_check,
        offline,
        requester,
    } = options;
    let link_name = package.link_name();
    let slot = package.store_slot(store_dir.root());
    let package_url = format!(
        "{CRATES_IO_DOWNLOAD_BASE}/{}/{}-{}.crate",
        package.name, package.name, package.version,
    );
    let package_id = format!("crate:{}@{}", package.name, package.version);
    let integrity = Integrity::from_hex(&package.checksum, Algorithm::Sha256)
        .into_diagnostic()
        .wrap_err_with(|| format!("decode checksum for {package_id}"))?;
    let mut cas_paths = IngestTarballToStore {
        http_client: &http_client,
        store_dir,
        store_index,
        store_index_writer: Some(store_index_writer),
        verify_store_integrity,
        strict_store_pkg_content_check,
        verified_files_cache,
        package_integrity: Some(&integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &package_url,
        package_id: &package_id,
        auth_headers: &auth_headers,
        requester: &requester,
        prefetched_cas_paths: None,
        retry_opts,
        ignore_file_pattern: None,
        offline,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::RawArchive,
    }
    .run_without_mem_cache::<Reporter>()
    .await
    .into_diagnostic()
    .wrap_err_with(|| format!("download {package_id}"))?;

    let checksum = package.checksum;
    let slot_for_import = slot.clone();
    tokio::task::spawn_blocking(move || {
        add_cargo_checksum(store_dir, &mut cas_paths, &checksum)?;
        import_indexed_dir::<Reporter>(
            &logged_methods,
            package_import_method,
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

fn add_cargo_checksum(
    store_dir: &StoreDir,
    cas_paths: &mut HashMap<String, PathBuf>,
    package_checksum: &str,
) -> Result<()> {
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
    let (cas_path, _) = store_dir
        .write_cas_file(&checksum, false)
        .into_diagnostic()
        .wrap_err("store .cargo-checksum.json")?;
    cas_paths.insert(".cargo-checksum.json".to_string(), cas_path);
    Ok(())
}

fn link_workspace(root_dir: &Path, slots: &[(String, PathBuf)]) -> Result<()> {
    let source_dir = ensure_workspace_directory(root_dir, &[".pnpm", "crates", "crates-io"])?;
    link_workspace_in(&source_dir, slots)
}

fn link_workspace_in(source_dir: &ManagedDirectory, slots: &[(String, PathBuf)]) -> Result<()> {
    for (name, slot) in slots {
        let outcome = force_workspace_symlink(source_dir, slot, name)
            .into_diagnostic()
            .wrap_err_with(|| format!("link cargo package {name}"))?;
        if let Some(warning) = outcome.warning {
            tracing::warn!(target: "pacquet::cargo", ?warning, "cargo package link warning");
        }
    }
    Ok(())
}

fn write_cargo_config(root_dir: &Path) -> Result<()> {
    let cargo_dir = ensure_workspace_directory(root_dir, &[".cargo"])?;
    write_cargo_config_in(&cargo_dir)
}

fn write_cargo_config_in(cargo_dir: &ManagedDirectory) -> Result<()> {
    let config_path = cargo_dir.path.join("config.toml");
    let (existing, mode) = match read_workspace_file(cargo_dir, "config.toml") {
        Ok(existing) => existing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => (String::new(), None),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", config_path.display()));
        }
    };
    let updated = update_managed_config(&existing)?;
    if updated != existing {
        write_workspace_file(cargo_dir, "config.toml", updated.as_bytes(), mode)
            .into_diagnostic()
            .wrap_err_with(|| format!("write {}", config_path.display()))?;
    }
    Ok(())
}

fn ensure_workspace_directory(root_dir: &Path, components: &[&str]) -> Result<ManagedDirectory> {
    #[cfg(unix)]
    {
        ensure_workspace_directory_unix(root_dir.to_path_buf(), components)
    }
    #[cfg(windows)]
    {
        let root = fs::canonicalize(root_dir).into_diagnostic().wrap_err_with(|| {
            format!("resolve Cargo workspace directory {}", root_dir.display())
        })?;
        ensure_workspace_directory_windows(root, components)
    }
}

#[cfg(unix)]
fn ensure_workspace_directory_unix(root: PathBuf, components: &[&str]) -> Result<ManagedDirectory> {
    use std::os::unix::fs::OpenOptionsExt as _;

    let mut options = fs::OpenOptions::new();
    options.read(true).custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY);
    let mut handle = options
        .open(&root)
        .into_diagnostic()
        .wrap_err_with(|| format!("open Cargo workspace directory {}", root.display()))?;
    let mut path = root;
    for component in components {
        path.push(component);
        handle = loop {
            match open_directory_at(&handle, component) {
                Ok(handle) => break handle,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    match create_directory_at(&handle, component) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(error) => {
                            return Err(error).into_diagnostic().wrap_err_with(|| {
                                format!("create Cargo directory {}", path.display())
                            });
                        }
                    }
                }
                Err(error)
                    if error.kind() == io::ErrorKind::NotADirectory
                        || error.raw_os_error() == Some(libc::ELOOP) =>
                {
                    let path = path.display();
                    return Err(miette::miette!(
                        "managed Cargo directory {} must be a real directory",
                        path,
                    ));
                }
                Err(error) => {
                    return Err(error)
                        .into_diagnostic()
                        .wrap_err_with(|| format!("inspect Cargo directory {}", path.display()));
                }
            }
        };
    }
    Ok(ManagedDirectory { path, handle })
}

#[cfg(unix)]
fn open_directory_at(parent: &fs::File, name: &str) -> io::Result<fs::File> {
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStrExt as _};

    let name = std::ffi::CString::new(std::ffi::OsStr::new(name).as_bytes())?;
    // SAFETY: `name` is NUL-terminated, `parent` stays open for the call, and
    // the returned descriptor is owned immediately on success.
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    file_from_descriptor(descriptor)
}

#[cfg(unix)]
fn create_directory_at(parent: &fs::File, name: &str) -> io::Result<()> {
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStrExt as _};

    let name = std::ffi::CString::new(std::ffi::OsStr::new(name).as_bytes())?;
    // SAFETY: `name` is NUL-terminated and `parent` stays open for the call.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o777) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn file_from_descriptor(descriptor: libc::c_int) -> io::Result<fs::File> {
    use std::os::fd::{FromRawFd as _, OwnedFd};

    if descriptor == -1 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: a successful `openat` returns a new descriptor owned by the caller.
        let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
        Ok(fs::File::from(descriptor))
    }
}

#[cfg(windows)]
fn ensure_workspace_directory_windows(
    root: PathBuf,
    components: &[&str],
) -> Result<ManagedDirectory> {
    let root_handle = open_pinned_windows_directory(&root)
        .into_diagnostic()
        .wrap_err_with(|| format!("open Cargo workspace directory {}", root.display()))?;
    let root_metadata = root_handle
        .metadata()
        .into_diagnostic()
        .wrap_err_with(|| format!("inspect Cargo workspace directory {}", root.display()))?;
    if !root_metadata.is_dir() || is_windows_reparse_point(&root_metadata) {
        let root = root.display();
        return Err(miette::miette!(
            "managed Cargo workspace directory {} must be a real directory",
            root,
        ));
    }
    let mut handles = vec![root_handle];
    let mut path = root;
    for component in components {
        path.push(component);
        let handle = loop {
            match open_pinned_windows_directory(&path) {
                Ok(handle) => break handle,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    match fs::create_dir(&path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(error) => {
                            return Err(error).into_diagnostic().wrap_err_with(|| {
                                format!("create Cargo directory {}", path.display())
                            });
                        }
                    }
                }
                Err(error) => {
                    return Err(error)
                        .into_diagnostic()
                        .wrap_err_with(|| format!("inspect Cargo directory {}", path.display()));
                }
            }
        };
        let metadata = handle
            .metadata()
            .into_diagnostic()
            .wrap_err_with(|| format!("inspect Cargo directory {}", path.display()))?;
        if !metadata.is_dir() || is_windows_reparse_point(&metadata) {
            let path = path.display();
            return Err(miette::miette!(
                "managed Cargo directory {} must be a real directory",
                path,
            ));
        }
        handles.push(handle);
    }
    // Windows lacks the descriptor-relative operations used on Unix. Keeping
    // every component open without FILE_SHARE_DELETE prevents a checked parent
    // from being renamed or replaced while the path-based helpers run.
    Ok(ManagedDirectory { path, _pinned_components: handles })
}

#[cfg(windows)]
fn open_pinned_windows_directory(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(windows)]
fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(unix)]
fn read_workspace_file(
    directory: &ManagedDirectory,
    name: &str,
) -> io::Result<(String, Option<u32>)> {
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStrExt as _, unix::fs::PermissionsExt as _};

    let name = std::ffi::CString::new(std::ffi::OsStr::new(name).as_bytes())?;
    // SAFETY: `name` is NUL-terminated, and the directory descriptor remains
    // valid for the call. `O_NOFOLLOW` prevents a file-level symlink redirect.
    let descriptor = unsafe {
        libc::openat(
            directory.handle.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    let file = file_from_descriptor(descriptor)?;
    let mode = file.metadata()?.permissions().mode();
    let contents = io::read_to_string(file)?;
    Ok((contents, Some(mode)))
}

#[cfg(windows)]
fn read_workspace_file(
    directory: &ManagedDirectory,
    name: &str,
) -> io::Result<(String, Option<u32>)> {
    fs::read_to_string(directory.path.join(name)).map(|contents| (contents, None))
}

#[cfg(unix)]
fn write_workspace_file(
    directory: &ManagedDirectory,
    name: &str,
    bytes: &[u8],
    mode: Option<u32>,
) -> io::Result<()> {
    use std::io::Write as _;
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStrExt as _, unix::fs::PermissionsExt as _};

    let destination = std::ffi::CString::new(std::ffi::OsStr::new(name).as_bytes())?;
    let (temporary, mut file) = loop {
        let temporary_name = format!(
            ".{name}.pnpm-{}-{}",
            std::process::id(),
            MANAGED_TEMP_ID.fetch_add(1, Ordering::Relaxed),
        );
        let temporary = std::ffi::CString::new(temporary_name.as_bytes())?;
        // SAFETY: the name is NUL-terminated, the directory descriptor remains
        // valid, and a successful call returns a new descriptor owned by this function.
        let descriptor = unsafe {
            libc::openat(
                directory.handle.as_raw_fd(),
                temporary.as_ptr(),
                libc::O_WRONLY | libc::O_CLOEXEC | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
                0o600,
            )
        };
        match file_from_descriptor(descriptor) {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        if let Some(mode) = mode {
            file.set_permissions(fs::Permissions::from_mode(mode))?;
        }
        file.sync_all()?;
        // SAFETY: both names are valid C strings and both directory descriptors
        // refer to the same live, pinned directory.
        if unsafe {
            libc::renameat(
                directory.handle.as_raw_fd(),
                temporary.as_ptr(),
                directory.handle.as_raw_fd(),
                destination.as_ptr(),
            )
        } == 0
        {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    })();
    drop(file);
    if result.is_err() {
        // SAFETY: `temporary` is NUL-terminated and the directory handle is valid.
        unsafe {
            libc::unlinkat(directory.handle.as_raw_fd(), temporary.as_ptr(), 0);
        }
    }
    result
}

#[cfg(windows)]
fn write_workspace_file(
    directory: &ManagedDirectory,
    name: &str,
    bytes: &[u8],
    _mode: Option<u32>,
) -> io::Result<()> {
    pnpm_fs::write_atomic(&directory.path.join(name), bytes)
}

#[cfg(unix)]
fn force_workspace_symlink(
    directory: &ManagedDirectory,
    target: &Path,
    name: &str,
) -> io::Result<pnpm_fs::ForceSymlinkOutcome> {
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStrExt as _};

    let wanted = pnpm_fs::relative_path(&directory.path, target);
    let wanted_c = std::ffi::CString::new(wanted.as_os_str().as_bytes())?;
    let name_c = std::ffi::CString::new(std::ffi::OsStr::new(name).as_bytes())?;
    let mut warning = None;
    loop {
        // SAFETY: both paths are NUL-terminated and the directory handle is valid.
        if unsafe {
            libc::symlinkat(wanted_c.as_ptr(), directory.handle.as_raw_fd(), name_c.as_ptr())
        } == 0
        {
            return Ok(pnpm_fs::ForceSymlinkOutcome { reused: false, warning });
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error);
        }
        match read_link_at(&directory.handle, &name_c) {
            Ok(existing) if existing == wanted => {
                return Ok(pnpm_fs::ForceSymlinkOutcome { reused: true, warning });
            }
            Ok(_) => {
                // SAFETY: `name_c` is NUL-terminated and the directory handle is valid.
                if unsafe { libc::unlinkat(directory.handle.as_raw_fd(), name_c.as_ptr(), 0) } != 0
                {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::NotFound {
                        return Err(error);
                    }
                }
            }
            Err(error) if error.raw_os_error() == Some(libc::EINVAL) => {
                let ignored_name = format!(
                    ".ignored_{name}-{}-{}",
                    std::process::id(),
                    MANAGED_TEMP_ID.fetch_add(1, Ordering::Relaxed),
                );
                let ignored = std::ffi::CString::new(ignored_name.as_bytes())?;
                // SAFETY: both names are NUL-terminated and both descriptors refer
                // to the same valid directory.
                if unsafe {
                    libc::renameat(
                        directory.handle.as_raw_fd(),
                        name_c.as_ptr(),
                        directory.handle.as_raw_fd(),
                        ignored.as_ptr(),
                    )
                } != 0
                {
                    return Err(io::Error::last_os_error());
                }
                warning = Some(format!(
                    "Symlink wanted name was occupied by directory or file. Old entity moved: {:?}{}{} => {ignored_name}",
                    directory.path,
                    std::path::MAIN_SEPARATOR,
                    name,
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn read_link_at(directory: &fs::File, name: &std::ffi::CStr) -> io::Result<PathBuf> {
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStringExt as _};

    let mut capacity = 256;
    loop {
        let mut contents = Vec::<u8>::with_capacity(capacity);
        // SAFETY: the name and directory descriptor are valid, and the buffer has
        // `capacity` writable bytes. `readlinkat` initializes the returned prefix.
        let length = unsafe {
            libc::readlinkat(
                directory.as_raw_fd(),
                name.as_ptr(),
                contents.as_mut_ptr().cast(),
                contents.capacity(),
            )
        };
        if length == -1 {
            return Err(io::Error::last_os_error());
        }
        let length = usize::try_from(length).expect("readlinkat returned a nonnegative length");
        if length < contents.capacity() {
            // SAFETY: `readlinkat` initialized exactly `length` bytes on success.
            unsafe {
                contents.set_len(length);
            }
            return Ok(std::ffi::OsString::from_vec(contents).into());
        }
        capacity *= 2;
    }
}

#[cfg(windows)]
fn force_workspace_symlink(
    directory: &ManagedDirectory,
    target: &Path,
    name: &str,
) -> io::Result<pnpm_fs::ForceSymlinkOutcome> {
    pnpm_fs::force_symlink_dir(target, &directory.path.join(name))
}

fn update_managed_config(existing: &str) -> Result<String> {
    match (existing.find(MANAGED_START), existing.find(MANAGED_END)) {
        (None, None) => {
            let separator = if existing.is_empty() || existing.ends_with("\n\n") {
                ""
            } else if existing.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            Ok(format!("{existing}{separator}{MANAGED_CONFIG}\n"))
        }
        (Some(start), Some(end)) if start <= end => {
            let after = end + MANAGED_END.len();
            Ok(format!("{}{}{}", &existing[..start], MANAGED_CONFIG, &existing[after..]))
        }
        _ => Err(miette::miette!(
            ".cargo/config.toml contains an incomplete pnpm-managed Cargo source block"
        )),
    }
}

fn parse_lockfile(input: &str) -> Result<Vec<LockedCrate>> {
    let lockfile =
        cargo_lock::Lockfile::from_str(input).into_diagnostic().wrap_err("parse Cargo.lock")?;
    let mut packages = Vec::new();
    for package in lockfile.packages {
        if let Some(package) = locked_crate_from_package(package)? {
            packages.push(package);
        }
    }

    let mut names = BTreeSet::new();
    for package in &packages {
        let link_name = package.link_name();
        if !names.insert(link_name.clone()) {
            return Err(miette::miette!(
                "Cargo.lock contains duplicate crates.io package {}",
                link_name,
            ));
        }
    }
    Ok(packages)
}

fn locked_crate_from_package(package: cargo_lock::Package) -> Result<Option<LockedCrate>> {
    let Some(source) = package.source.as_ref() else {
        return Ok(None);
    };
    if !source.is_default_registry() {
        return Err(miette::miette!(
            "Cargo source {source:?} is not supported by the crates.io-only proof of concept"
        ));
    }
    let name = package.name.to_string();
    let version = package.version.to_string();
    let checksum = package
        .checksum
        .ok_or_else(|| miette::miette!("registry package {name} {version} has no checksum"))?
        .to_string();
    validate_package_field("crate name", &name, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
    })?;
    validate_package_field("crate version", &version, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+')
    })?;
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(miette::miette!("invalid checksum for crate {name} {version}"));
    }
    Ok(Some(LockedCrate { name, version, checksum: checksum.to_ascii_lowercase() }))
}

fn validate_package_field(label: &str, value: &str, allowed: impl Fn(u8) -> bool) -> Result<()> {
    if value.is_empty() || !value.bytes().all(allowed) {
        return Err(miette::miette!("invalid {label} {value:?} in Cargo.lock"));
    }
    Ok(())
}

impl LockedCrate {
    fn link_name(&self) -> String {
        format!("{}-{}", self.name, self.version)
    }

    /// The shared slot contains immutable crate source, not a dependency view.
    /// Unlike an npm GVS slot, it has no package-local dependency links, so its
    /// final identity component is the registry checksum rather than a graph
    /// hash. Cargo's workspace directory source supplies the graph-specific
    /// view, and Cargo writes compilation artifacts outside this slot.
    fn store_slot(&self, store_root: &Path) -> PathBuf {
        store_root.join("crates").join(&self.name).join(&self.version).join(&self.checksum)
    }
}

#[cfg(test)]
mod tests;
