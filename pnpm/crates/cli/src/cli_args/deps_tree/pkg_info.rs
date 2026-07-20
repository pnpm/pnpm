//! Per-edge node materialization: name, version, filesystem path,
//! resolved tarball URL, and dev/optional/peer/skipped/missing flags.
//! Rust counterpart of the TypeScript tree-builder's `getPkgInfo` and
//! `resolvePackagePath`.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use pacquet_fs::{is_subdir, lexical_normalize};
use pacquet_lockfile::{
    Lockfile, LockfileResolution, PkgNameVerPeer, npm_tarball_url, pick_registry_for_package,
};

use super::{
    DependencyNode,
    dep_types::{DepType, DepTypes},
    graph::GraphEdge,
    peers_suffix_hash,
};

/// Everything that stays constant while resolving node metadata across
/// one tree build.
pub(crate) struct PkgInfoEnv<'a> {
    pub lockfile_dir: PathBuf,
    /// Absolute, symlink-resolved `node_modules` of the lockfile root.
    pub modules_dir: PathBuf,
    /// Absolute virtual store directory (`<modules_dir>/.pnpm` unless
    /// the modules manifest points elsewhere, e.g. a global store).
    pub virtual_store_dir: PathBuf,
    pub virtual_store_dir_max_length: usize,
    /// Registry URLs keyed by `default` / `@scope`.
    pub registries: HashMap<String, String>,
    /// depPaths of packages skipped by the installer (unsupported
    /// platform optional deps).
    pub skipped: HashSet<String>,
    pub store_dir: Option<PathBuf>,
    pub current_lockfile: &'a Lockfile,
    pub wanted_lockfile: Option<&'a Lockfile>,
    pub dep_types: DepTypes,
}

impl PkgInfoEnv<'_> {
    /// Whether the virtual store lives outside the project's
    /// `node_modules` (global virtual store), in which case package
    /// paths must be resolved through symlinks.
    fn is_global_virtual_store(&self) -> bool {
        !is_subdir(&self.modules_dir, &self.virtual_store_dir)
            && self.virtual_store_dir != self.modules_dir
    }
}

/// Where a node's manifest can be read from, for `--long` output and
/// finder callbacks. The store is preferred (it has the manifest even
/// when `node_modules` was never materialized), the package directory
/// is the fallback.
#[derive(Debug, Clone)]
pub(crate) struct ManifestSource {
    pub path: PathBuf,
    pub integrity: Option<String>,
    pub name: String,
    pub version: String,
}

pub(crate) struct EdgeContext<'a> {
    /// Names in the parent's `peerDependencies`.
    pub peers: Option<&'a HashSet<String>>,
    /// Base directory for resolving `link:` paths of this edge.
    pub linked_path_base_dir: PathBuf,
    /// Directory `link:` versions are rewritten relative to.
    pub rewrite_link_version_dir: Option<PathBuf>,
    /// Resolved path of the parent package (used by the global virtual
    /// store to resolve through the parent's `node_modules`).
    pub parent_dir: Option<PathBuf>,
}

pub(crate) fn get_pkg_info(
    env: &PkgInfoEnv<'_>,
    edge: &GraphEdge,
    ctx: &EdgeContext<'_>,
) -> (DependencyNode, ManifestSource) {
    let name;
    let mut version;
    let mut resolved = None;
    let mut integrity = None;
    let mut optional = false;
    let mut is_skipped = false;
    let mut dev = None;

    let full_package_path: PathBuf;

    if let Some(dep_path) = &edge.dep_path {
        let metadata_key = dep_path.without_peer();

        let (in_current, current_snapshot, current_metadata) =
            lookup_dep(env.current_lockfile, dep_path, &metadata_key);
        let (known, snapshot, metadata) = if in_current {
            (true, current_snapshot, current_metadata)
        } else {
            // The package is missing from the current lockfile — it was
            // never materialized (e.g. skipped platform-specific
            // optional deps).
            is_skipped = env.skipped.contains(&dep_path.to_string());
            match env.wanted_lockfile {
                Some(wanted) => lookup_dep(wanted, dep_path, &metadata_key),
                None => (false, None, None),
            }
        };

        if known {
            name = dep_path.name.to_string();
            version = metadata
                .and_then(|metadata| metadata.version.clone())
                .unwrap_or_else(|| dep_path.suffix.version().to_string());
            optional = snapshot.is_some_and(|snapshot| snapshot.optional);
            if let Some(metadata) = metadata {
                integrity = metadata.resolution.integrity().map(ToString::to_string);
                resolved = resolved_tarball_url(env, &metadata.resolution, &name, &version);
            }
        } else {
            name = edge.alias.clone();
            version = edge.ref_display.clone();
        }
        dev = match env.dep_types.get(dep_path) {
            Some(DepType::DevOnly) => Some(true),
            Some(DepType::ProdOnly) => Some(false),
            Some(DepType::DevAndProd) | None => None,
        };
        full_package_path = resolve_package_path(env, dep_path, &name, &edge.alias, ctx);
    } else {
        name = edge.alias.clone();
        version = edge.ref_display.clone();
        let link_target = edge.link_target.as_deref().unwrap_or("");
        full_package_path = lexical_normalize(&ctx.linked_path_base_dir.join(link_target));
    }

    if version.is_empty() {
        version = edge.ref_display.clone();
    }
    if version.starts_with("link:")
        && let Some(rewrite_dir) = &ctx.rewrite_link_version_dir
    {
        let relative = pathdiff::diff_paths(&full_package_path, rewrite_dir)
            .unwrap_or_else(|| full_package_path.clone());
        version = format!("link:{}", relative.to_string_lossy().replace('\\', "/"));
    }

    let path = full_package_path.to_string_lossy().into_owned();
    let manifest_source = ManifestSource {
        path: full_package_path,
        integrity,
        name: name.clone(),
        version: version.clone(),
    };
    let node = DependencyNode {
        alias: edge.alias.clone(),
        name,
        version,
        path,
        resolved,
        is_peer: ctx.peers.is_some_and(|peers| peers.contains(&edge.alias)),
        is_skipped,
        dev,
        optional,
        peers_suffix_hash: edge.dep_path.as_ref().and_then(peers_suffix_hash),
        ..DependencyNode::default()
    };
    (node, manifest_source)
}

fn lookup_dep<'l>(
    lockfile: &'l Lockfile,
    dep_path: &PkgNameVerPeer,
    metadata_key: &PkgNameVerPeer,
) -> (
    bool,
    Option<&'l pacquet_lockfile::SnapshotEntry>,
    Option<&'l pacquet_lockfile::PackageMetadata>,
) {
    let snapshot = lockfile.snapshots.as_ref().and_then(|snapshots| snapshots.get(dep_path));
    let metadata = lockfile.packages.as_ref().and_then(|packages| packages.get(metadata_key));
    (snapshot.is_some() || metadata.is_some(), snapshot, metadata)
}

/// The tarball URL recorded in (or reconstructible from) a lockfile
/// resolution. `None` for resolutions that have no tarball (git,
/// directory, binary, and so on), matching the TypeScript CLI, which
/// drops the `resolved` field for those.
fn resolved_tarball_url(
    env: &PkgInfoEnv<'_>,
    resolution: &LockfileResolution,
    name: &str,
    version: &str,
) -> Option<String> {
    match resolution {
        LockfileResolution::Tarball(tarball) => Some(tarball.tarball.clone()),
        LockfileResolution::Registry(_) => {
            let registry = pick_registry_for_package(&env.registries, name, None);
            Some(npm_tarball_url(name, version, &registry))
        }
        _ => None,
    }
}

/// A lockfile-derived path component that could escape the directory
/// it is joined under — the same guard `pnpm licenses` applies before
/// dereferencing store paths built from lockfile keys.
fn is_unsafe_path_component(component: &str) -> bool {
    component.contains("..") || Path::new(component).is_absolute()
}

/// Filesystem path of a package addressed by `dep_path`. For a local
/// virtual store the path is constructed directly; for a global
/// virtual store the symlink through the parent's `node_modules` is
/// resolved instead. A name that could traverse outside the virtual
/// store is never joined or dereferenced.
pub(crate) fn resolve_package_path(
    env: &PkgInfoEnv<'_>,
    dep_path: &PkgNameVerPeer,
    name: &str,
    alias: &str,
    ctx: &EdgeContext<'_>,
) -> PathBuf {
    let store_name = dep_path.to_virtual_store_name(env.virtual_store_dir_max_length);
    if is_unsafe_path_component(&store_name) || is_unsafe_path_component(name) {
        return env.virtual_store_dir.clone();
    }
    let constructed = env.virtual_store_dir.join(store_name).join("node_modules").join(name);

    if !env.is_global_virtual_store() || is_unsafe_path_component(alias) {
        return constructed;
    }

    let node_modules_dir = match &ctx.parent_dir {
        Some(parent_dir) => {
            let mut dir = parent_dir.parent().map(Path::to_path_buf).unwrap_or_default();
            // Scoped parents live one level deeper (`node_modules/@scope/pkg`).
            if dir.file_name().is_some_and(|component| component.to_string_lossy().starts_with('@'))
                && let Some(grandparent) = dir.parent()
            {
                dir = grandparent.to_path_buf();
            }
            dir
        }
        None => env.modules_dir.clone(),
    };
    dunce::canonicalize(node_modules_dir.join(alias)).unwrap_or(constructed)
}

#[cfg(test)]
mod tests;
