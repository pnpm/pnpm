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
/// tarball downloads happen later from upstream URLs or an upstream's
/// `/~<name>/` registry endpoint.
///
/// A request is reconstructed as a real workspace in the temp dir — a
/// `package.json` per requested project and, when there are members, a
/// `pnpm-workspace.yaml` listing their dirs — so pacquet's install path
/// discovers and resolves every importer in one pass, producing a lockfile
/// keyed by the same POSIX importer dirs the client sent.
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
        let name = project.name.clone().unwrap_or_else(|| importer_manifest_name(rel));
        let version = project.version.as_deref().unwrap_or("0.0.0");
        let manifest_json = serde_json::json!({
            "name": name,
            "version": version,
            "dependencies": project.dependencies,
            "devDependencies": project.dev_dependencies,
            "optionalDependencies": project.optional_dependencies,
        });
        let manifest_bytes = serde_json::to_vec(&manifest_json)
            .map_err(|err| ResolveError::Install(err.to_string()))?;
        tokio::fs::write(project_dir.join("package.json"), manifest_bytes).await?;
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
    let manifest = if wrote_root {
        PackageManifest::from_path(manifest_path)
            .map_err(|err| ResolveError::Manifest(err.to_string()))?
    } else {
        // Install needs an active manifest, but keeping this stand-in in
        // memory prevents workspace discovery from inventing a `.` importer
        // that the client did not request.
        PackageManifest::from_value(
            manifest_path,
            serde_json::json!({ "name": "pnpr-resolve", "version": "0.0.0" }),
        )
    };

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
        emit_initial_manifest: true,
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
        peer_issues_sink: None,
        catalogs_override: None,
        disable_optimistic_repeat_install: false,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
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
        "name": project.name.as_deref().unwrap_or("pnpr-resolve"),
        "version": project.version.as_deref().unwrap_or("0.0.0"),
        "dependencies": project.dependencies,
        "devDependencies": project.dev_dependencies,
        "optionalDependencies": project.optional_dependencies,
    });
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest_json).ok()?).ok()?;
    let manifest = PackageManifest::from_path(manifest_path).ok()?;
    // The synthesized manifest carries no `peerDependencies`, so the
    // auto-install-peers fold is a no-op; pass pnpm's default anyway.
    satisfies_package_manifest(importer, &manifest, true, &|_: &str| false).ok()?;

    Some(lockfile.clone())
}

/// Validate a client-supplied importer dir before joining it onto the
/// server's temp dir. The client normalizes to POSIX relative paths, so
/// anything absolute, backslash- or colon-bearing, or containing a
/// non-canonical component is rejected rather than risking a write outside
/// the temp workspace or allowing two importer IDs to address the same path.
/// The exact `.` value is the only accepted root importer.
fn sanitized_importer_dir(dir: &str) -> Result<&str, ResolveError> {
    if dir == "." {
        return Ok(".");
    }
    if dir.is_empty()
        || dir.starts_with('/')
        || dir.contains('\\')
        || dir.contains(':')
        || dir
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(ResolveError::Install(format!("unsafe importer dir: {dir:?}")));
    }
    Ok(dir)
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
