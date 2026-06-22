//! Server-side dependency resolution backed by pacquet.
//!
//! Writes a throwaway project, resolves it lockfile-only (so
//! `node_modules` is never linked and no tarball is fetched), then reads
//! the produced lockfile back. pnpr serves no files, so the store is
//! never populated with package contents — the client fetches every
//! tarball itself.

use std::{
    collections::HashSet,
    sync::{Arc, atomic::AtomicU8},
};

use dashmap::DashMap;
use pacquet_config::{Config, NodeLinker};
use pacquet_lockfile::{Lockfile, check_lockfile_settings, satisfies_package_manifest};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_package_manager::{Install, ResolutionObserver, ResolvedPackages};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::SilentReporter;
use pacquet_tarball::MemCache;

use super::protocol::ResolveRequest;

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
/// The store is intentionally left untouched (no tarball is fetched):
/// pnpr serves no file content, so the client fetches every tarball.
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
    request: &ResolveRequest,
    auth_headers: &Arc<AuthHeaders>,
    observer: Option<Arc<dyn ResolutionObserver>>,
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
        lockfile: pacquet_lockfile::MaybeLazyLockfile::Loaded(input_lockfile),
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
        // (in `handle_resolve`) before we get here, so the install path
        // must not re-verify it.
        trust_lockfile: true,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: NodeLinker::Isolated,
        lockfile_only: true,
        dry_run: false,
        update_seed_policy: pacquet_package_manager::UpdateSeedPolicy::KeepAll,
        // Resolve as the caller (forwarded credentials) without baking
        // per-user auth into the interned `&'static Config`.
        auth_override: Some(Arc::clone(auth_headers)),
        // Stream each resolved tarball to the client as the walk yields
        // it (`/-/pnpr/v0/resolve` NDJSON `package` frames) so tarball fetch
        // overlaps this server-side resolution. `None` falls back to a
        // single terminal `done` frame carrying the whole lockfile.
        resolution_observer: observer,
        catalogs_override: None,
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
pub fn fresh_frozen_input_lockfile(config: &Config, request: &ResolveRequest) -> Option<Lockfile> {
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
        || config.patched_dependencies.as_ref().is_some_and(|map| !map.is_empty())
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
