use futures_util::{StreamExt, TryStreamExt, stream};
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_config::Config;
use pnpm_deps_restorer::{ImportIndexedDirOpts, import_indexed_dir};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_reporter::Reporter;
use pnpm_store_dir::{SharedVerifiedFilesCache, StoreDir};
use pnpm_tarball::DownloadTarballToStore;
use serde::Serialize;
use ssri::{Algorithm, Integrity};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, atomic::AtomicU8},
    time::Duration,
};

const CRATES_IO_SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";
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

#[derive(Debug, Default)]
struct LockedPackageBuilder {
    name: Option<String>,
    version: Option<String>,
    source: Option<String>,
    checksum: Option<String>,
}

#[derive(Serialize)]
struct CargoChecksum<'a> {
    files: BTreeMap<String, String>,
    package: &'a str,
}

pub async fn install<Reporter: self::Reporter + 'static>(
    config: &Config,
    root_dir: &Path,
    lockfile_only: bool,
    frozen_lockfile: bool,
    lockfile_policy: CargoLockfilePolicy,
    http_client: Arc<ThrottledClient>,
) -> Result<()> {
    if !config.cargo.enabled {
        return Ok(());
    }

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
    let store_dir = Box::leak(Box::new(config.store_dir.clone()));
    store_dir
        .init()
        .into_diagnostic()
        .wrap_err_with(|| format!("initialize cargo package store at {}", store_dir.display()))?;

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
        .await?;

    link_workspace(root_dir, &slots)?;
    write_cargo_config(root_dir)?;
    Ok(())
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
    let manifest_path = root_dir.join("Cargo.toml");
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
    let slot = store_dir.root().join("crates").join(package.slot_name());
    if slot_is_complete(&slot) {
        return Ok((link_name, slot));
    }

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
        store_index: None,
        store_index_writer: None,
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

fn slot_is_complete(slot: &Path) -> bool {
    slot.join("package.json").is_file()
        && slot.join("Cargo.toml").is_file()
        && slot.join(".cargo-checksum.json").is_file()
}

fn link_workspace(root_dir: &Path, slots: &[(String, PathBuf)]) -> Result<()> {
    let source_dir = root_dir.join(".pnpm").join("crates").join("crates-io");
    fs::create_dir_all(&source_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("create cargo source directory {}", source_dir.display()))?;
    let expected = slots.iter().map(|(name, _)| name.as_str()).collect::<BTreeSet<_>>();
    for entry in fs::read_dir(&source_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("read cargo source directory {}", source_dir.display()))?
    {
        let entry = entry.into_diagnostic().wrap_err("read cargo source entry")?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| miette::miette!("cargo source directory contains a non-UTF-8 entry"))?;
        if !expected.contains(name.as_str()) {
            pnpm_fs::remove_dirent(&entry.path())
                .into_diagnostic()
                .wrap_err_with(|| format!("remove stale cargo source entry {name}"))?;
        }
    }
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
    let config_path = root_dir.join(".cargo").join("config.toml");
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
    let mut packages = Vec::new();
    let mut current = None;
    for (line_index, line) in input.lines().enumerate() {
        let line = line.trim();
        if line == "[[package]]" {
            finish_package(current.take(), &mut packages)?;
            current = Some(LockedPackageBuilder::default());
            continue;
        }
        let Some(package) = current.as_mut() else { continue };
        if let Some((key, value)) = line.split_once(" = ") {
            let target = match key {
                "name" => Some(&mut package.name),
                "version" => Some(&mut package.version),
                "source" => Some(&mut package.source),
                "checksum" => Some(&mut package.checksum),
                _ => None,
            };
            if let Some(target) = target {
                *target = Some(parse_string(value, line_index + 1)?);
            }
        }
    }
    finish_package(current, &mut packages)?;

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

fn finish_package(
    package: Option<LockedPackageBuilder>,
    packages: &mut Vec<LockedCrate>,
) -> Result<()> {
    let Some(package) = package else { return Ok(()) };
    let Some(source) = package.source else { return Ok(()) };
    if source != CRATES_IO_SOURCE {
        return Err(miette::miette!(
            "Cargo source {source:?} is not supported by the crates.io-only proof of concept"
        ));
    }
    let name = package.name.ok_or_else(|| miette::miette!("registry package has no name"))?;
    let version =
        package.version.ok_or_else(|| miette::miette!("registry package {name} has no version"))?;
    let checksum = package
        .checksum
        .ok_or_else(|| miette::miette!("registry package {name} {version} has no checksum"))?;
    validate_package_field("crate name", &name, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
    })?;
    validate_package_field("crate version", &version, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+')
    })?;
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(miette::miette!("invalid checksum for crate {name} {version}"));
    }
    packages.push(LockedCrate { name, version, checksum: checksum.to_ascii_lowercase() });
    Ok(())
}

fn validate_package_field(label: &str, value: &str, allowed: impl Fn(u8) -> bool) -> Result<()> {
    if value.is_empty() || !value.bytes().all(allowed) {
        return Err(miette::miette!("invalid {label} {value:?} in Cargo.lock"));
    }
    Ok(())
}

fn parse_string(value: &str, line: usize) -> Result<String> {
    serde_json::from_str(value)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse Cargo.lock string on line {line}"))
}

impl LockedCrate {
    fn link_name(&self) -> String {
        format!("{}-{}", self.name, self.version)
    }

    fn slot_name(&self) -> String {
        format!("{}-{}-{}", self.name, self.version, self.checksum)
    }
}

#[cfg(test)]
mod tests;
