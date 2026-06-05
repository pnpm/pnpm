//! Server-side dependency resolution backed by pacquet.
//!
//! Writes a throwaway project, resolves it lockfile-only (so
//! `node_modules` is never linked), reads the produced lockfile back,
//! then fetches into the shared store only the packages that aren't
//! cached yet. The store index that results is the source of truth the
//! [`super::diff`] pass reads.

use std::{
    collections::HashSet,
    sync::{Arc, atomic::AtomicU8},
};

use dashmap::DashMap;
use pacquet_config::{Config, NodeLinker};
use pacquet_lockfile::{
    Lockfile, LockfileResolution, check_lockfile_settings, satisfies_package_manifest,
};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_package_manager::{Install, ResolvedPackages};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::SilentReporter;
use pacquet_store_dir::{SharedVerifiedFilesCache, StoreIndex, StoreIndexWriter, store_index_key};
use pacquet_tarball::{DownloadTarballToStore, MemCache, RetryOpts};

use super::protocol::InstallRequest;

/// A resolved package distilled from the lockfile, carrying everything
/// needed both to fetch it (`tarball_url`) and to diff it (`integrity`,
/// `pkg_id`).
pub struct ResolvedPkg {
    pub pkg_id: String,
    pub integrity: String,
    pub tarball_url: String,
}

#[derive(Debug)]
pub enum ResolveError {
    Io(std::io::Error),
    Manifest(String),
    Install(String),
    NoLockfile,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::Io(err) => write!(f, "{err}"),
            ResolveError::Manifest(msg) => write!(f, "failed to read manifest: {msg}"),
            ResolveError::Install(msg) => write!(f, "resolution failed: {msg}"),
            ResolveError::NoLockfile => write!(f, "resolution produced no lockfile"),
        }
    }
}

impl From<std::io::Error> for ResolveError {
    fn from(err: std::io::Error) -> Self {
        ResolveError::Io(err)
    }
}

/// Resolve a request lockfile-only and return the produced lockfile.
/// The store is intentionally left untouched here (no tarball is
/// fetched); [`fetch_uncached`] populates it afterward.
///
/// A single-project request resolves one root (`.`) importer. A
/// multi-project request is reconstructed as a real workspace in the
/// temp dir — root manifest, `pnpm-workspace.yaml` listing the member
/// dirs, and a `package.json` per member — so pacquet's install path
/// discovers and resolves every importer in one pass, producing a
/// lockfile keyed by the same POSIX importer dirs the client sent.
pub async fn resolve(
    config: &'static Config,
    client: &Arc<ThrottledClient>,
    request: &InstallRequest,
    auth_headers: &Arc<AuthHeaders>,
) -> Result<Lockfile, ResolveError> {
    let projects = request.projects_normalized();

    let temp = tempfile::Builder::new().prefix("pnpr-resolve-").tempdir()?;
    let dir = temp.path();

    let mut member_dirs: Vec<&str> = Vec::new();
    let mut seen_dirs: HashSet<&str> = HashSet::new();
    let mut wrote_root = false;
    for project in &projects {
        let rel = sanitized_importer_dir(&project.dir)?;
        // Reject duplicate importer dirs (including several that normalize
        // to `.`): writing the same `package.json` twice would silently
        // drop the earlier project's dependency map.
        if !seen_dirs.insert(rel) {
            return Err(ResolveError::Install(format!("duplicate importer dir: {rel:?}")));
        }
        let project_dir = if rel == "." {
            wrote_root = true;
            dir.to_path_buf()
        } else {
            member_dirs.push(rel);
            dir.join(rel)
        };
        tokio::fs::create_dir_all(&project_dir).await?;
        let manifest_json = serde_json::json!({
            "name": importer_manifest_name(rel),
            "version": "0.0.0",
            "dependencies": project.dependencies,
            "devDependencies": project.dev_dependencies,
            "optionalDependencies": project.optional_dependencies,
        });
        let manifest_bytes = serde_json::to_vec(&manifest_json)
            .map_err(|err| ResolveError::Install(err.to_string()))?;
        tokio::fs::write(project_dir.join("package.json"), manifest_bytes).await?;
    }

    // A workspace needs a root manifest at its root even when the client
    // didn't send a `.` importer (e.g. a member-only filtered install).
    if !wrote_root {
        let root_json = serde_json::json!({ "name": "pnpr-resolve", "version": "0.0.0" });
        let root_bytes =
            serde_json::to_vec(&root_json).map_err(|err| ResolveError::Install(err.to_string()))?;
        tokio::fs::write(dir.join("package.json"), root_bytes).await?;
    }

    // Only declare a workspace when there are members; a lone root
    // importer resolves as a plain single project (no workspace file).
    if !member_dirs.is_empty() {
        let mut yaml = String::from("packages:\n");
        for member in &member_dirs {
            // Emit each dir as a double-quoted scalar (JSON strings are
            // valid YAML) so a dir with YAML-significant characters
            // (`:`, `#`, leading `-`, ...) stays a plain string instead of
            // being reparsed as a mapping or breaking the document.
            let quoted = serde_json::to_string(member)
                .map_err(|err| ResolveError::Install(err.to_string()))?;
            yaml.push_str("  - ");
            yaml.push_str(&quoted);
            yaml.push('\n');
        }
        tokio::fs::write(dir.join("pnpm-workspace.yaml"), yaml).await?;
    }

    let manifest_path = dir.join("package.json");
    let manifest = PackageManifest::from_path(manifest_path)
        .map_err(|err| ResolveError::Manifest(err.to_string()))?;

    // Seed resolution from the client's lockfile when present, matching
    // pnpm's resolution-reuse: frozen → use it as-is (already verified
    // by the caller before this point); non-frozen → reuse its pins for
    // unchanged entries and resolve only what's new/changed
    // (`preferFrozenLockfile` + `update: false`). With no lockfile it's a
    // fresh resolve. `frozen_lockfile` is passed through unchanged so a
    // `--frozen-lockfile` request with no lockfile surfaces pacquet's
    // frozen-lockfile error rather than silently synthesizing one.
    let input_lockfile = request.lockfile.as_ref();
    let lockfile_path = dir.join(Lockfile::FILE_NAME);
    if let Some(lockfile) = input_lockfile {
        lockfile
            .save_to_path(&lockfile_path)
            .map_err(|err| ResolveError::Install(err.to_string()))?;
    }
    let frozen_lockfile = request.frozen_lockfile;

    let resolved_packages: ResolvedPackages = DashMap::new();
    let tarball_mem_cache: Arc<MemCache> = Arc::new(MemCache::default());
    let _logged = AtomicU8::new(0);

    Install {
        tarball_mem_cache,
        resolved_packages: &resolved_packages,
        http_client: client,
        http_client_arc: Arc::clone(client),
        config,
        manifest: &manifest,
        lockfile: input_lockfile,
        lockfile_path: input_lockfile.map(|_| lockfile_path.as_path()),
        dependency_groups: vec![
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
        ],
        frozen_lockfile,
        // Default to reuse so unchanged entries keep their pins; the
        // client's `--no-prefer-frozen-lockfile` (`Some(false)`) forces
        // a fresh re-resolve.
        prefer_frozen_lockfile: request.prefer_frozen_lockfile.or(Some(true)),
        ignore_manifest_check: request.ignore_manifest_check,
        skip_runtimes: false,
        // The lockfile was already verified under the client's policy
        // (in `handle_install`) before we get here, so the install path
        // must not re-verify it.
        trust_lockfile: true,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: NodeLinker::Isolated,
        lockfile_only: true,
        update_seed_policy: pacquet_package_manager::UpdateSeedPolicy::KeepAll,
        // Resolve as the caller (forwarded credentials) without baking
        // per-user auth into the interned `&'static Config`.
        auth_override: Some(Arc::clone(auth_headers)),
    }
    .run::<SilentReporter>()
    .await
    .map_err(|err| ResolveError::Install(err.to_string()))?;

    let lockfile = Lockfile::load_wanted_from_dir(dir)
        .map_err(|err| ResolveError::Install(err.to_string()))?
        .ok_or(ResolveError::NoLockfile)?;

    Ok(lockfile)
}

/// Return the caller's frozen input lockfile when pacquet's freshness
/// checks prove the server's lockfile-only resolve would return it
/// unchanged.
pub fn fresh_frozen_input_lockfile(config: &Config, request: &InstallRequest) -> Option<Lockfile> {
    if !request.frozen_lockfile || request.prefer_frozen_lockfile == Some(false) {
        return None;
    }
    if request.overrides.as_ref().is_some_and(|value| match value {
        serde_json::Value::Object(map) => !map.is_empty(),
        serde_json::Value::Null => false,
        _ => true,
    }) {
        return None;
    }
    if config.package_extensions.as_ref().is_some_and(|extensions| !extensions.is_empty())
        || config
            .ignored_optional_dependencies
            .as_ref()
            .is_some_and(|patterns| !patterns.is_empty())
        || config.inject_workspace_packages
    {
        return None;
    }

    let lockfile = request.lockfile.as_ref()?;
    check_lockfile_settings(
        lockfile,
        None,
        None,
        None,
        config.inject_workspace_packages,
        config.peers_suffix_max_length,
    )
    .ok()?;

    if request.ignore_manifest_check {
        return Some(lockfile.clone());
    }

    let mut projects = request.projects_normalized();
    if projects.len() != 1 {
        return None;
    }
    let project = projects.pop()?;
    if project.dir != "." && !project.dir.is_empty() {
        return None;
    }
    let importer = lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY)?;
    let temp = tempfile::Builder::new().prefix("pnpr-frozen-").tempdir().ok()?;
    let manifest_path = temp.path().join("package.json");
    let manifest_json = serde_json::json!({
        "name": "pnpr-resolve",
        "version": "0.0.0",
        "dependencies": project.dependencies,
        "devDependencies": project.dev_dependencies,
        "optionalDependencies": project.optional_dependencies,
    });
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest_json).ok()?).ok()?;
    let manifest = PackageManifest::from_path(manifest_path).ok()?;
    satisfies_package_manifest(importer, &manifest, Lockfile::ROOT_IMPORTER_KEY, &|_: &str| false)
        .ok()?;

    Some(lockfile.clone())
}

/// Extract every registry/tarball package from the lockfile, deriving
/// the tarball URL the same way pacquet's install path does (registry
/// resolutions never store the URL in a v9 lockfile).
pub fn collect_packages(lockfile: &Lockfile, registry: &str) -> Vec<ResolvedPkg> {
    let Some(packages) = lockfile.packages.as_ref() else { return Vec::new() };
    let mut out = Vec::with_capacity(packages.len());
    for (key, metadata) in packages {
        let dep_path = key.to_string();
        let pkg_id = dep_path.split('(').next().unwrap_or(&dep_path).to_string();
        let Some((integrity, tarball_url)) = fetch_info(&metadata.resolution, &pkg_id, registry)
        else {
            continue;
        };
        out.push(ResolvedPkg { pkg_id, integrity, tarball_url });
    }
    out
}

/// Fetch into the shared store every package whose store-index row is
/// absent, populating its `PackageFilesIndex` as a side effect. Cached
/// packages are skipped, matching the server hot-cache no-op.
///
/// Returns the `pkg_id`s actually fetched this call — the upstream
/// accepted the caller's credentials for each, so the gate treats a
/// freshly-fetched private package as proven (no re-verify).
pub async fn fetch_uncached(
    config: &'static Config,
    client: &Arc<ThrottledClient>,
    auth_headers: &AuthHeaders,
    packages: &[ResolvedPkg],
) -> Result<HashSet<String>, ResolveError> {
    let store_dir = &config.store_dir;
    let package_keys: Vec<String> =
        packages.iter().map(|pkg| store_index_key(&pkg.integrity, &pkg.pkg_id)).collect();

    let present: HashSet<String> = match StoreIndex::open_readonly_in(store_dir) {
        Ok(index) => index.existing_keys(&package_keys).unwrap_or_default(),
        Err(_) => HashSet::new(),
    };

    let to_fetch: Vec<&ResolvedPkg> = packages
        .iter()
        .filter(|pkg| !present.contains(&store_index_key(&pkg.integrity, &pkg.pkg_id)))
        .filter(|pkg| !pkg.tarball_url.is_empty())
        .collect();

    if to_fetch.is_empty() {
        return Ok(HashSet::new());
    }

    let fetched_ids: HashSet<String> = to_fetch.iter().map(|pkg| pkg.pkg_id.clone()).collect();

    let integrities: Vec<Option<ssri::Integrity>> =
        to_fetch.iter().map(|pkg| pkg.integrity.parse::<ssri::Integrity>().ok()).collect();

    let shared_index = StoreIndex::shared_readonly_in(store_dir);
    let (writer, writer_task) = StoreIndexWriter::spawn(store_dir);
    let verified = SharedVerifiedFilesCache::default();

    let downloads = to_fetch.iter().zip(integrities.iter()).filter_map(|(pkg, integrity)| {
        let integrity = integrity.as_ref()?;
        let store_index = shared_index.clone();
        let writer = Arc::clone(&writer);
        let verified = SharedVerifiedFilesCache::clone(&verified);
        Some(async move {
            DownloadTarballToStore {
                http_client: client,
                store_dir,
                store_index,
                store_index_writer: Some(writer),
                verify_store_integrity: config.verify_store_integrity,
                verified_files_cache: verified,
                package_integrity: integrity,
                package_unpacked_size: None,
                package_url: &pkg.tarball_url,
                package_id: &pkg.pkg_id,
                auth_headers,
                requester: "pnpr",
                prefetched_cas_paths: None,
                retry_opts: RetryOpts::default(),
                ignore_file_pattern: None,
                offline: false,
            }
            .run_without_mem_cache::<SilentReporter>()
            .await
            .map_err(|err| ResolveError::Install(err.to_string()))
        })
    });

    let results = futures_util::future::join_all(downloads).await;

    drop(writer);
    let _ = writer_task.await;

    for result in results {
        result?;
    }
    Ok(fetched_ids)
}

/// Derive `(integrity, tarball_url)` for a resolution, mirroring
/// pacquet's `tarball_url_and_integrity`. Returns `None` for git,
/// directory, binary, and variations resolutions (not served by the
/// pnpr install accelerator).
fn fetch_info(
    resolution: &LockfileResolution,
    pkg_id: &str,
    registry: &str,
) -> Option<(String, String)> {
    match resolution {
        LockfileResolution::Tarball(tarball) => {
            let integrity = tarball.integrity.as_ref()?;
            Some((integrity.to_string(), tarball.tarball.clone()))
        }
        LockfileResolution::Registry(registry_resolution) => {
            let (name, version) = split_name_version(pkg_id)?;
            let bare = name.rsplit('/').next().unwrap_or(name);
            let registry = registry.strip_suffix('/').unwrap_or(registry);
            Some((
                registry_resolution.integrity.to_string(),
                format!("{registry}/{name}/-/{bare}-{version}.tgz"),
            ))
        }
        _ => None,
    }
}

/// Split `name@version` into its parts, tolerating a leading scope
/// `@` (`@scope/name@1.2.3` → `("@scope/name", "1.2.3")`).
fn split_name_version(pkg_id: &str) -> Option<(&str, &str)> {
    let at = pkg_id.rfind('@')?;
    if at == 0 {
        return None;
    }
    Some((&pkg_id[..at], &pkg_id[at + 1..]))
}

/// Validate a client-supplied importer dir before joining it onto the
/// server's temp dir. The client normalizes to POSIX relative paths, so
/// anything absolute, backslash-bearing, or containing a `..`/empty
/// component is rejected rather than risking a write outside the temp
/// workspace. Returns the canonical `.` for the root.
fn sanitized_importer_dir(dir: &str) -> Result<&str, ResolveError> {
    if dir.is_empty() || dir == "." {
        return Ok(".");
    }
    let trimmed = dir.trim_end_matches('/');
    // A now-empty string means the input was only slashes (`/`, `////`):
    // an absolute path, not the root — reject it rather than collapse it
    // to `.` by trimming.
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.contains('\\')
        || trimmed.split('/').any(|component| component == ".." || component.is_empty())
    {
        return Err(ResolveError::Install(format!("unsafe importer dir: {dir:?}")));
    }
    Ok(trimmed)
}

/// A synthetic, unique `name` for an importer's throwaway manifest. The
/// importer is keyed in the lockfile by its dir, not its name, so the
/// name only needs to be present and distinct across members.
///
/// The dir → name mapping is injective: `-` is first escaped to `--`,
/// then `/` becomes `-`, so distinct dirs (e.g. `packages/foo` vs
/// `packages-foo`) never collide on the same manifest name.
fn importer_manifest_name(dir: &str) -> String {
    if dir == "." {
        "pnpr-resolve".to_string()
    } else {
        format!("pnpr-importer-{}", dir.replace('-', "--").replace('/', "-"))
    }
}

#[cfg(test)]
mod tests;
