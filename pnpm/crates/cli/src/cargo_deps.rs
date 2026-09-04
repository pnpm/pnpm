use futures_util::{StreamExt, TryStreamExt, stream};
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_config::Config;
use pnpm_deps_restorer::{ImportIndexedDirOpts, import_indexed_dir};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_reporter::Reporter;
use pnpm_store_dir::{
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir, StoreIndex, StoreIndexWriter,
};
use pnpm_tarball::DownloadTarballToStore;
use serde::{Deserialize, Serialize};
use ssri::{Algorithm, Integrity};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::{Arc, atomic::AtomicU8},
    time::Duration,
};

const CRATES_IO_DOWNLOAD_BASE: &str = "https://static.crates.io/crates";
const CRATES_IO_SPARSE_INDEX: &str = "https://index.crates.io";
const MANAGED_START: &str = "# >>> pnpm-managed cargo sources >>>";
const MANAGED_END: &str = "# <<< pnpm-managed cargo sources <<<";
const MANAGED_CONFIG: &str = "# >>> pnpm-managed cargo sources >>>\n[source.crates-io]\nreplace-with = \"pnpm-crates-io\"\n\n[source.pnpm-crates-io]\ndirectory = \".pnpm/crates/crates-io\"\n# <<< pnpm-managed cargo sources <<<";

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

pub async fn install<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    root_dir: &Path,
    discover_projects: bool,
    lockfile_only: bool,
    frozen_lockfile: bool,
    lockfile_policy: CargoLockfilePolicy,
    http_client: Arc<ThrottledClient>,
) -> Result<()> {
    if !config.cargo.enabled {
        return Ok(());
    }

    let roots = if discover_projects {
        discover_workspace_roots(root_dir).await?
    } else {
        vec![root_dir.to_path_buf()]
    };
    futures_util::future::try_join_all(roots.iter().map(|root| {
        install_workspace::<Reporter>(
            config,
            root,
            lockfile_only,
            frozen_lockfile,
            lockfile_policy,
            Arc::clone(&http_client),
        )
    }))
    .await?;
    Ok(())
}

async fn install_workspace<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    root_dir: &Path,
    lockfile_only: bool,
    frozen_lockfile: bool,
    lockfile_policy: CargoLockfilePolicy,
    http_client: Arc<ThrottledClient>,
) -> Result<()> {
    let cargo_lock_path = root_dir.join("Cargo.lock");
    let cargo_lock = ensure_lockfile(
        config,
        root_dir,
        &cargo_lock_path,
        frozen_lockfile,
        lockfile_policy,
        &http_client,
    )
    .await?;
    if lockfile_only {
        return Ok(());
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

    let auth_headers = Arc::new(AuthHeaders::default());
    let verified_files_cache = SharedVerifiedFilesCache::default();
    let logged_methods = Arc::new(AtomicU8::new(0));
    let retry_opts = RetryOpts {
        retries: config.fetch_retries,
        factor: config.fetch_retry_factor,
        min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
        max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
    };
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
        .try_collect::<Vec<_>>()
        .await;
    drop(store_index_writer);
    StoreIndexWriter::drain(writer_task, "; some Cargo rows may not be persisted").await;
    let slots = slots?;

    link_workspace(root_dir, &slots)?;
    write_cargo_config(root_dir)?;
    Ok(())
}

pub(crate) async fn workspace_root(manifest_path: &Path) -> Result<PathBuf> {
    let metadata = read_cargo_metadata_for_manifest(manifest_path).await?;
    serde_json::from_str::<CargoWorkspaceMetadata>(&metadata)
        .into_diagnostic()
        .wrap_err("read Cargo workspace root from metadata")
        .map(|metadata| metadata.workspace_root)
}

async fn discover_workspace_roots(search_root: &Path) -> Result<Vec<PathBuf>> {
    let search_root = search_root.to_path_buf();
    let manifests = tokio::task::spawn_blocking(move || discover_manifests(&search_root))
        .await
        .into_diagnostic()
        .wrap_err("join Cargo project discovery task")??;
    let roots = stream::iter(manifests)
        .map(|manifest| async move { workspace_root(&manifest).await })
        .buffer_unordered(8)
        .try_collect::<BTreeSet<_>>()
        .await?;
    Ok(roots.into_iter().collect())
}

fn discover_manifests(search_root: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![search_root.to_path_buf()];
    let mut manifests = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if directory != search_root && is_ignorable_discovery_error(&error) => {
                continue;
            }
            Err(error) => {
                return Err(error).into_diagnostic().wrap_err_with(|| {
                    format!(
                        "read directory while discovering Cargo projects at {}",
                        directory.display(),
                    )
                });
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if is_ignorable_discovery_error(&error) => continue,
                Err(error) => {
                    return Err(error).into_diagnostic().wrap_err_with(|| {
                        format!(
                            "read entry while discovering Cargo projects at {}",
                            directory.display(),
                        )
                    });
                }
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) if is_ignorable_discovery_error(&error) => continue,
                Err(error) => {
                    return Err(error).into_diagnostic().wrap_err_with(|| {
                        format!("inspect Cargo project candidate {}", entry.path().display())
                    });
                }
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | ".pnpm" | "node_modules" | "target"),
                ) {
                    pending.push(entry.path());
                }
            } else if file_type.is_file() && entry.file_name() == "Cargo.toml" {
                manifests.push(entry.path());
            }
        }
    }
    Ok(manifests)
}

fn is_ignorable_discovery_error(error: &std::io::Error) -> bool {
    matches!(error.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied,)
}

async fn ensure_lockfile(
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
    let index_files = fetch_sparse_index(config, &metadata, http_client).await?;
    let lockfile = pnpm_cargo_resolver::resolve_lockfile(&metadata, &index_files)
        .wrap_err("resolve Cargo dependencies")?;
    pnpm_fs::write_atomic(cargo_lock_path, lockfile.as_bytes())
        .into_diagnostic()
        .wrap_err_with(|| format!("write {}", cargo_lock_path.display()))?;
    Ok(lockfile)
}

pub(crate) async fn latest_version(
    config: &Config,
    name: &str,
    http_client: &ThrottledClient,
) -> Result<String> {
    let auth_headers = AuthHeaders::default();
    let cache_dir = config.cache_dir.join("v11").join("cargo-index").join("crates-io");
    let index_file =
        fetch_sparse_index_file(name, &cache_dir, http_client, &auth_headers, config.offline)
            .await?;
    pnpm_cargo_resolver::latest_version(name, &index_file)
        .wrap_err_with(|| format!("select the latest version of crate {name}"))
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
    let auth_headers = Arc::new(AuthHeaders::default());
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
                        &cache_dir,
                        &http_client,
                        &auth_headers,
                        config.offline,
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
    cache_dir: &Path,
    http_client: &ThrottledClient,
    auth_headers: &AuthHeaders,
    offline: bool,
) -> Result<String> {
    let relative_path = sparse_index_path(name)?;
    let cache_path = cache_dir.join(&relative_path);
    if offline {
        return fs::read_to_string(&cache_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("read cached sparse index entry for {name}"));
    }

    let url = format!("{CRATES_IO_SPARSE_INDEX}/{relative_path}");
    let response = http_client
        .get_bytes_with_secure_auth_headers(&url, auth_headers)
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
    let mut cas_paths = DownloadTarballToStore {
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
        append_manifest: None,
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
    for (name, slot) in slots {
        let outcome = pnpm_fs::force_symlink_dir(slot, &source_dir.join(name))
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
    let config_path = cargo_dir.join("config.toml");
    let existing = match fs::read_to_string(&config_path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", config_path.display()));
        }
    };
    let updated = update_managed_config(&existing)?;
    if updated != existing {
        pnpm_fs::write_atomic(&config_path, updated.as_bytes())
            .into_diagnostic()
            .wrap_err_with(|| format!("write {}", config_path.display()))?;
    }
    Ok(())
}

fn ensure_workspace_directory(root_dir: &Path, components: &[&str]) -> Result<PathBuf> {
    let mut directory = root_dir.to_path_buf();
    for component in components {
        directory.push(component);
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => {
                let directory = directory.display();
                return Err(miette::miette!(
                    "managed Cargo directory {} must be a real directory",
                    directory,
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&directory)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("create Cargo directory {}", directory.display()))?;
            }
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("inspect Cargo directory {}", directory.display()));
            }
        }
    }
    Ok(directory)
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
    let packages = lockfile
        .packages
        .into_iter()
        .filter_map(|package| package.source.clone().map(|source| (package, source)))
        .map(|(package, source)| {
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
            Ok(LockedCrate { name, version, checksum: checksum.to_ascii_lowercase() })
        })
        .collect::<Result<Vec<_>>>()?;

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
