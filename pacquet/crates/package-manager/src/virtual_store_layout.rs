//! Per-install computed layout of the virtual store.
//!
//! Stage 1 of pnpm/pacquet#432 introduces a path split: when the global
//! virtual store is enabled, packages live at
//! `<store_dir>/links/<scope>/<name>/<version>/<hash>/node_modules/<name>`,
//! not at the project-local
//! `<project>/node_modules/.pnpm/<flat-name>/node_modules/<name>`. The
//! shape of `<flat-name>` versus `<scope>/<name>/<version>/<hash>` is
//! also different — flat name uses [`PkgNameVerPeer::to_virtual_store_name`]
//! while the GVS layout uses
//! [`pacquet_graph_hasher::format_global_virtual_store_path`] over a
//! `calc_graph_node_hash`-computed digest.
//!
//! [`VirtualStoreLayout`] hides that difference behind one
//! [`slot_dir`] lookup so the install pipeline doesn't have to branch
//! on `Config::enable_global_virtual_store` at every site that
//! computes a per-snapshot path.
//!
//! [`slot_dir`]: VirtualStoreLayout::slot_dir
//! [`PkgNameVerPeer::to_virtual_store_name`]: pacquet_lockfile::PkgNameVerPeer::to_virtual_store_name
//! [`pacquet_graph_hasher::format_global_virtual_store_path`]: pacquet_graph_hasher::format_global_virtual_store_path

use crate::{AllowBuildPolicy, install_frozen_lockfile::find_own_runtime_node_major};
use pacquet_config::Config;
use pacquet_graph_hasher::{
    DepsGraphNode, DepsStateCache, calc_graph_node_hash, engine_name,
    format_global_virtual_store_path,
};
use pacquet_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgIdWithPatchHash, PkgName, SnapshotDepRef,
    SnapshotEntry,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// Precomputed mapping from each snapshot key to the directory where
/// its files live on disk. Built once per install in
/// [`InstallFrozenLockfile::run`](crate::InstallFrozenLockfile::run);
/// passed by reference to every helper that needs to know where a
/// particular snapshot is materialised.
///
/// [`Self::slot_dir`] is the only call site every consumer has to
/// touch — it returns an absolute directory whose `node_modules/<name>`
/// subdirectory holds the unpacked package.
pub struct VirtualStoreLayout {
    /// Root containing every per-snapshot subdirectory. Picked from
    /// `Config::global_virtual_store_dir` when GVS is enabled (the
    /// shared `<store_dir>/links` path, or the user's pinned override)
    /// and from `Config::virtual_store_dir` when GVS is disabled (the
    /// project-local `<modules_dir>/.pnpm`). Pacquet keeps the two
    /// fields separate so the legacy non-frozen
    /// [`crate::InstallWithoutLockfile`] path can keep reading
    /// `virtual_store_dir` directly via [`Self::legacy`] without the
    /// frozen-lockfile derivation redirecting it. See
    /// [`Config::apply_global_virtual_store_derivation`] for the
    /// reasoning behind the field split.
    ///
    /// Stored separately from a `&Config` so callers don't have to
    /// thread the full config through the helpers that only need a
    /// path lookup.
    package_store_dir: PathBuf,

    /// `Some` only when the global virtual store is enabled. For each
    /// snapshot, holds the precomputed
    /// `[<scope>/]<name>/<version>/<hash>` suffix that goes after
    /// `package_store_dir`. `None` when GVS is off — callers fall back
    /// to [`PkgNameVerPeer::to_virtual_store_name`] computed on demand
    /// from the snapshot key.
    ///
    /// [`PkgNameVerPeer::to_virtual_store_name`]: pacquet_lockfile::PkgNameVerPeer::to_virtual_store_name
    gvs_suffixes: Option<HashMap<PackageKey, String>>,

    /// Threshold passed into
    /// [`PkgNameVerPeer::to_virtual_store_name`] for the legacy flat-
    /// name fallback. Mirrors pnpm's `virtualStoreDirMaxLength`: when
    /// the escaped filename exceeds this many bytes, the tail is
    /// replaced with a 32-char sha256 hash so the directory name fits
    /// within filesystem limits (macOS / ext4 cap component names at
    /// 255 bytes, but pnpm defaults to 120 to leave headroom for the
    /// `<name>@<version>/` suffix appended below).
    ///
    /// [`PkgNameVerPeer::to_virtual_store_name`]: pacquet_lockfile::PkgNameVerPeer::to_virtual_store_name
    virtual_store_dir_max_length: usize,
}

impl VirtualStoreLayout {
    /// Construct a layout that always uses the legacy
    /// `<root>/<flat-name>` shape, regardless of any
    /// `enable_global_virtual_store` setting on `Config`. The
    /// non-frozen install path uses this — GVS is scoped to
    /// frozen-lockfile installs (pnpm/pacquet#432), so without-lockfile
    /// callers stay on the project-local flat layout even when
    /// `enable_global_virtual_store: true` is configured.
    pub fn legacy(root: impl Into<PathBuf>, virtual_store_dir_max_length: usize) -> Self {
        VirtualStoreLayout {
            package_store_dir: root.into(),
            gvs_suffixes: None,
            virtual_store_dir_max_length,
        }
    }

    /// Build the layout for one install. Reads
    /// [`Config::enable_global_virtual_store`] to decide whether to
    /// precompute GVS slot names, then iterates the lockfile's
    /// `snapshots` (the per-peer-context entries) and computes each
    /// snapshot's [`format_global_virtual_store_path`]-shaped suffix
    /// via [`calc_graph_node_hash`].
    ///
    /// Returns a layout that's safe to pass by reference across rayon
    /// workers: every field is `Send + Sync` once constructed (the
    /// internal `HashMap<PackageKey, String>` doesn't mutate after
    /// `new`).
    ///
    /// `engine` is the install-wide fallback `ENGINE_NAME`-style
    /// string that [`pacquet_graph_hasher::engine_name`] produces;
    /// threaded in instead of recomputed inside so the value matches
    /// whatever the rest of the install (notably the side-effects
    /// cache key) uses. Snapshots that themselves pin Node via
    /// `engines.runtime` (carried in the lockfile as
    /// `dependencies.node: runtime:<version>`) override the fallback
    /// per-snapshot through `find_own_runtime_node_major` — the
    /// engine portion of the hash then tracks the Node that pnpm's
    /// bin linker would spawn for that pinning package's lifecycle
    /// scripts (see
    /// [`bins/linker/src/index.ts:229-237`](https://github.com/pnpm/pnpm/blob/29a42efc3b/bins/linker/src/index.ts#L229-L237)).
    /// Mirrors upstream's
    /// [`readSnapshotRuntimePin`](https://github.com/pnpm/pnpm/blob/HEAD/engine/runtime/system-node-version/src/index.ts)
    /// branch in `@pnpm/deps.graph-hasher`.
    ///
    /// `None` propagates straight into
    /// [`calc_graph_node_hash`]'s `engine` parameter — `None` and
    /// `Some("")` produce *different* GVS hashes (the former omits
    /// the `engine` contribution, the latter hashes the empty string),
    /// so the call site must keep the `Option` shape rather than
    /// flattening to `unwrap_or("")`.
    ///
    /// `snapshots` / `packages` are the lockfile fields the caller
    /// already has by the time the install dispatches to a frozen-
    /// lockfile flow — see
    /// [`crate::InstallFrozenLockfile::run`].
    ///
    /// `allow_build_policy` drives engine-agnostic gating. When
    /// `Some`, the constructor walks `snapshots` once to collect
    /// every key whose `(name, version)` passes
    /// [`AllowBuildPolicy::check`] returning `Some(true)`, then
    /// passes that set as `built_dep_paths` to
    /// [`calc_graph_node_hash`]. Pure-JS subgraphs hash with
    /// `engine = null` so their GVS directories survive Node.js
    /// upgrades. When `None`, every snapshot keeps the engine in
    /// its hash payload — matches upstream's
    /// [`builtDepPaths === undefined`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L140-L142)
    /// branch and the existing pacquet behaviour from
    /// pnpm/pacquet#449.
    pub fn new(
        config: &Config,
        engine: Option<&str>,
        snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
        packages: Option<&HashMap<PackageKey, PackageMetadata>>,
        allow_build_policy: Option<&AllowBuildPolicy>,
    ) -> Self {
        // Pacquet keeps `virtual_store_dir` and `global_virtual_store_dir`
        // as two separate fields (see
        // [`Config::apply_global_virtual_store_derivation`] for why).
        // The frozen-lockfile install picks
        // `global_virtual_store_dir` here when GVS is on so the
        // without-lockfile path can stay on the project-local
        // `virtual_store_dir` without colliding.
        let package_store_dir = if config.enable_global_virtual_store {
            config.global_virtual_store_dir.clone()
        } else {
            config.virtual_store_dir.clone()
        };
        let virtual_store_dir_max_length = config.virtual_store_dir_max_length as usize;
        if !config.enable_global_virtual_store {
            return VirtualStoreLayout {
                package_store_dir,
                gvs_suffixes: None,
                virtual_store_dir_max_length,
            };
        }
        let Some(snapshots) = snapshots else {
            return VirtualStoreLayout {
                package_store_dir,
                gvs_suffixes: Some(HashMap::new()),
                virtual_store_dir_max_length,
            };
        };
        let graph = lockfile_to_dep_graph(snapshots, packages);
        // Build the engine-agnostic gating set once per install,
        // mirroring upstream's
        // [`computeBuiltDepPaths`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L208-L219).
        // `None` here disables gating so every snapshot still hashes
        // with its engine string — the pre-pnpm/pacquet#459 behaviour.
        let built_dep_paths: Option<HashSet<PackageKey>> = allow_build_policy.map(|policy| {
            snapshots
                .keys()
                .filter(|key| {
                    let metadata_key = key.without_peer();
                    let name = metadata_key.name.to_string();
                    let version = metadata_key.suffix.version().to_string();
                    policy.check(&name, &version) == Some(true)
                })
                .cloned()
                .collect()
        });
        let mut cache: DepsStateCache<PackageKey> = HashMap::new();
        // Install-scoped memoization for the `transitivelyRequiresBuild`
        // walk; shared across every snapshot's hash computation so
        // diamond-shaped subgraphs only get visited once. Untouched
        // when `built_dep_paths` is `None`. Mirrors upstream's
        // [`buildRequiredCache`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L113-L114).
        let mut build_required_cache: HashMap<PackageKey, bool> = HashMap::new();
        let mut gvs_suffixes: HashMap<PackageKey, String> = HashMap::with_capacity(snapshots.len());
        for (snapshot_key, snapshot) in snapshots {
            // Per-snapshot engine resolution: a snapshot that declares
            // its own `engines.runtime` carries the desugared
            // `dependencies.node: 'runtime:<version>'` pin, which has
            // to drive the engine portion of *its* hash rather than
            // the install-wide fallback. Match upstream's
            // [`readSnapshotRuntimePin`](https://github.com/pnpm/pnpm/blob/HEAD/engine/runtime/system-node-version/src/index.ts)
            // precedence: own pin first, install-wide fallback
            // second. Default host platform / arch (`None`, `None`)
            // matches whatever the caller used to format the
            // fallback `engine` so the two strings remain comparable
            // across snapshots in one install.
            let own_engine =
                find_own_runtime_node_major(snapshot).map(|major| engine_name(major, None, None));
            let snapshot_engine = own_engine.as_deref().or(engine);
            let hex_digest = calc_graph_node_hash(
                &graph,
                &mut cache,
                snapshot_key,
                snapshot_engine,
                built_dep_paths.as_ref(),
                &mut build_required_cache,
            );
            let metadata_key = snapshot_key.without_peer();
            let name = metadata_key.name.to_string();
            let version = metadata_key.suffix.version().to_string();
            let suffix = format_global_virtual_store_path(&name, &version, &hex_digest);
            gvs_suffixes.insert(snapshot_key.clone(), suffix);
        }
        VirtualStoreLayout {
            package_store_dir,
            gvs_suffixes: Some(gvs_suffixes),
            virtual_store_dir_max_length,
        }
    }

    /// Root of the layout — the directory that contains every per-
    /// snapshot subdirectory. Exposed so callers that need to pass a
    /// path to existing helpers (e.g. the
    /// [`pacquet_modules_yaml::Modules`] writer, which still records
    /// the legacy [`Config::virtual_store_dir`] string) have one
    /// source of truth.
    pub fn package_store_dir(&self) -> &Path {
        &self.package_store_dir
    }

    /// Whether this install is running in global-virtual-store mode.
    /// Mirrors `config.enable_global_virtual_store` — captured here so
    /// callers can ask the layout itself instead of having to keep a
    /// separate `&Config` reference for the boolean.
    pub fn enable_global_virtual_store(&self) -> bool {
        self.gvs_suffixes.is_some()
    }

    /// Absolute directory that holds `node_modules/<name>` for one
    /// snapshot. Falls back to
    /// [`PkgNameVerPeer::to_virtual_store_name`](pacquet_lockfile::PkgNameVerPeer::to_virtual_store_name)
    /// when GVS is off, or when GVS is on but the key isn't in the
    /// precomputed map (which would indicate a bug — every snapshot
    /// the install touches must have been visited in
    /// [`Self::new`]; the fallback is defensive rather than expected
    /// to fire).
    pub fn slot_dir(&self, key: &PackageKey) -> PathBuf {
        let suffix = match &self.gvs_suffixes {
            Some(map) => map
                .get(key)
                .cloned()
                .unwrap_or_else(|| key.to_virtual_store_name(self.virtual_store_dir_max_length)),
            None => key.to_virtual_store_name(self.virtual_store_dir_max_length),
        };
        self.package_store_dir.join(suffix)
    }
}

/// Build the dependency graph from the lockfile's `snapshots` /
/// `packages` sections. Mirrors upstream's
/// [`lockfileToDepGraph`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L162-L181)
/// — every entry in `snapshots` becomes a node whose `full_pkg_id` is
/// `<pkg_id_with_patch_hash>:<integrity>` (for tarball / registry
/// resolutions) and whose `children` are the alias→snapshot-key edges
/// pulled from the snapshot's combined `dependencies` +
/// `optionalDependencies`.
///
/// Packages whose metadata is missing or whose resolution has no
/// `integrity` (directory / git) are emitted with the bare
/// `pkg_id_with_patch_hash` as their `full_pkg_id`. The frozen-
/// lockfile install path rejects those resolutions before reaching the
/// linker, so a stub `full_pkg_id` here is safe — the GVS hash for an
/// install that contains one of those snapshots is irrelevant because
/// the install will error out before consulting it.
fn lockfile_to_dep_graph(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> HashMap<PackageKey, DepsGraphNode<PackageKey>> {
    snapshots
        .iter()
        .map(|(snapshot_key, snapshot)| {
            let children = collect_children(snapshot);
            let metadata_key = snapshot_key.without_peer();
            let pkg_id_with_patch_hash = PkgIdWithPatchHash::from(metadata_key.to_string());
            let resolution =
                packages.and_then(|map| map.get(&metadata_key)).map(|meta| &meta.resolution);
            let full_pkg_id = create_full_pkg_id(&pkg_id_with_patch_hash, resolution);
            (snapshot_key.clone(), DepsGraphNode { full_pkg_id, children })
        })
        .collect()
}

/// Combine a snapshot's `dependencies` and `optionalDependencies` into
/// the graph's alias→key edges. Mirrors
/// [`lockfileDepsToGraphChildren`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L237-L246)
/// composed with upstream's `{...deps, ...optionalDeps}` spread at the
/// caller.
fn collect_children(snapshot: &SnapshotEntry) -> HashMap<String, PackageKey> {
    let mut children = HashMap::new();
    if let Some(deps) = &snapshot.dependencies {
        merge_into_children(&mut children, deps);
    }
    if let Some(deps) = &snapshot.optional_dependencies {
        merge_into_children(&mut children, deps);
    }
    children
}

fn merge_into_children(
    children: &mut HashMap<String, PackageKey>,
    deps: &HashMap<PkgName, SnapshotDepRef>,
) {
    for (alias, dep_ref) in deps {
        // `link:` deps have no snapshot key — skip them.
        let Some(resolved) = dep_ref.resolve(alias) else {
            continue;
        };
        children.insert(alias.to_string(), resolved);
    }
}

/// Mirrors upstream's
/// [`createFullPkgId`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L248-L274).
/// `variations` (cross-platform variant) resolutions don't exist in
/// pacquet's lockfile model yet — when they're added, this helper
/// will need the `selectPlatformVariant` branch upstream uses to pick
/// the right integrity.
fn create_full_pkg_id(
    pkg_id_with_patch_hash: &PkgIdWithPatchHash,
    resolution: Option<&LockfileResolution>,
) -> String {
    match resolution.and_then(LockfileResolution::integrity) {
        Some(integrity) => format!("{pkg_id_with_patch_hash}:{integrity}"),
        // Directory / git / missing-metadata fall through to the bare
        // id. The install path rejects these resolutions before the
        // hash is consulted (see
        // [`crate::InstallPackageBySnapshotError::UnsupportedResolution`]),
        // so the value never actually drives a slot path on disk.
        None => pkg_id_with_patch_hash.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::VirtualStoreLayout;
    use pacquet_config::Config;
    use pacquet_lockfile::{
        LockfileResolution, PackageKey, PackageMetadata, PkgName, RegistryResolution,
        SnapshotDepRef, SnapshotEntry,
    };
    use pretty_assertions::{assert_eq, assert_ne};
    use std::{collections::HashMap, path::PathBuf};

    /// Build a `Config` test-double with the GVS-relevant fields
    /// wired explicitly. `gvs_dir` populates `global_virtual_store_dir`
    /// for the GVS-on path; `virtual_store_dir` stays at the
    /// project-local default for the GVS-off path.
    fn make_config(gvs: bool, virtual_store_dir: PathBuf, gvs_dir: PathBuf) -> Config {
        let mut config = Config::new();
        config.enable_global_virtual_store = gvs;
        config.virtual_store_dir = virtual_store_dir;
        config.global_virtual_store_dir = gvs_dir;
        config
    }

    /// With GVS off, the layout reproduces today's flat-name layout
    /// (`<virtual_store_dir>/<flat-name>`) — proving the helper is a
    /// drop-in for the legacy path.
    #[test]
    fn slot_dir_uses_flat_name_when_gvs_off() {
        let config = make_config(
            false,
            PathBuf::from("/tmp/proj/node_modules/.pnpm"),
            PathBuf::from("/tmp/store/links"),
        );
        let layout = VirtualStoreLayout::new(&config, Some("ignored"), None, None, None);
        let key: PackageKey = "@scope/foo@1.2.3".parse().unwrap();
        assert_eq!(
            layout.slot_dir(&key),
            PathBuf::from("/tmp/proj/node_modules/.pnpm/@scope+foo@1.2.3"),
        );
    }

    /// With GVS on and a single snapshot, the layout produces the
    /// `<root>/<scope>/<name>/<version>/<hash>` shape upstream's tests
    /// assert against. The hash is opaque; we only check the prefix
    /// and depth.
    #[test]
    fn slot_dir_uses_gvs_layout_when_gvs_on() {
        let config = make_config(
            true,
            PathBuf::from("/tmp/proj/node_modules/.pnpm"),
            PathBuf::from("/tmp/store/links"),
        );
        let key: PackageKey = "@scope/foo@1.2.3".parse().unwrap();
        let mut packages = HashMap::new();
        packages.insert(
            key.clone(),
            PackageMetadata {
                resolution: LockfileResolution::Registry(RegistryResolution {
                    integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                        .parse()
                        .expect("parse integrity"),
                }),
                engines: None,
                cpu: None,
                os: None,
                libc: None,
                deprecated: None,
                has_bin: None,
                prepare: None,
                bundled_dependencies: None,
                peer_dependencies: None,
                peer_dependencies_meta: None,
            },
        );
        let mut snapshots = HashMap::new();
        snapshots.insert(key.clone(), SnapshotEntry::default());
        let layout = VirtualStoreLayout::new(
            &config,
            Some("darwin-arm64-node20"),
            Some(&snapshots),
            Some(&packages),
            None,
        );
        let slot = layout.slot_dir(&key);
        // Shape: `/tmp/store/links/@scope/foo/1.2.3/<64-hex>`.
        let stripped = slot
            .strip_prefix("/tmp/store/links/@scope/foo/1.2.3/")
            .expect("slot dir must live under <root>/<scope>/<name>/<version>/ when GVS is on");
        assert_eq!(
            stripped.to_string_lossy().len(),
            64,
            "trailing hash component must be a full sha256 hex digest",
        );
    }

    /// Unscoped packages get an `@/` prefix so every entry in the
    /// shared store sits at the same `<scope>/<name>/<version>/<hash>`
    /// depth — easier `readdir`-driven traversal.
    #[test]
    fn slot_dir_prefixes_unscoped_with_at_slash_under_gvs() {
        let config = make_config(
            true,
            PathBuf::from("/tmp/proj/node_modules/.pnpm"),
            PathBuf::from("/tmp/store/links"),
        );
        let key: PackageKey = "foo@1.0.0".parse().unwrap();
        let mut packages = HashMap::new();
        packages.insert(
            key.clone(),
            PackageMetadata {
                resolution: LockfileResolution::Registry(RegistryResolution {
                    integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                        .parse()
                        .expect("parse integrity"),
                }),
                engines: None,
                cpu: None,
                os: None,
                libc: None,
                deprecated: None,
                has_bin: None,
                prepare: None,
                bundled_dependencies: None,
                peer_dependencies: None,
                peer_dependencies_meta: None,
            },
        );
        let mut snapshots = HashMap::new();
        snapshots.insert(key.clone(), SnapshotEntry::default());
        let layout = VirtualStoreLayout::new(
            &config,
            Some("linux-x64-node22"),
            Some(&snapshots),
            Some(&packages),
            None,
        );
        let slot = layout.slot_dir(&key);
        let _ = slot
            .strip_prefix("/tmp/store/links/@/foo/1.0.0/")
            .expect("unscoped GVS slots live under <root>/@/<name>/<version>/<hash>");
    }

    /// End-to-end gating check: a pure-JS snapshot's GVS slot is
    /// engine-agnostic when an empty `AllowBuildPolicy` is supplied
    /// (matches upstream's
    /// [`enableGlobalVirtualStore: true` → `allowBuilds ??= {}`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L342-L344)
    /// shape). Two installs that differ only in the `engine` string
    /// produce the *same* slot directory.
    #[test]
    fn slot_dir_engine_agnostic_with_empty_allow_build_policy() {
        let config = make_config(
            true,
            PathBuf::from("/tmp/proj/node_modules/.pnpm"),
            PathBuf::from("/tmp/store/links"),
        );
        let key: PackageKey = "left-pad@1.0.0".parse().unwrap();
        let mut packages = HashMap::new();
        packages.insert(
            key.clone(),
            PackageMetadata {
                resolution: LockfileResolution::Registry(RegistryResolution {
                    integrity: "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"
                        .parse()
                        .expect("parse integrity"),
                }),
                engines: None,
                cpu: None,
                os: None,
                libc: None,
                deprecated: None,
                has_bin: None,
                prepare: None,
                bundled_dependencies: None,
                peer_dependencies: None,
                peer_dependencies_meta: None,
            },
        );
        let mut snapshots = HashMap::new();
        snapshots.insert(key.clone(), SnapshotEntry::default());
        let policy = crate::AllowBuildPolicy::default();
        let darwin = VirtualStoreLayout::new(
            &config,
            Some("darwin-arm64-node20"),
            Some(&snapshots),
            Some(&packages),
            Some(&policy),
        )
        .slot_dir(&key);
        let linux = VirtualStoreLayout::new(
            &config,
            Some("linux-x64-node22"),
            Some(&snapshots),
            Some(&packages),
            Some(&policy),
        )
        .slot_dir(&key);
        assert_eq!(
            darwin, linux,
            "pure-JS snapshot must share one GVS slot across engines when gating is active",
        );
    }

    /// Symmetric to [`slot_dir_engine_agnostic_with_empty_allow_build_policy`]:
    /// when the snapshot is in `allow_builds`, the engine *is* part
    /// of the slot path. Two installs that differ in `engine` end up
    /// in different directories.
    #[test]
    fn slot_dir_engine_specific_when_snapshot_is_built() {
        let config = make_config(
            true,
            PathBuf::from("/tmp/proj/node_modules/.pnpm"),
            PathBuf::from("/tmp/store/links"),
        );
        let key: PackageKey = "native-pkg@1.0.0".parse().unwrap();
        let mut packages = HashMap::new();
        packages.insert(
            key.clone(),
            PackageMetadata {
                resolution: LockfileResolution::Registry(RegistryResolution {
                    integrity: "sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC"
                        .parse()
                        .expect("parse integrity"),
                }),
                engines: None,
                cpu: None,
                os: None,
                libc: None,
                deprecated: None,
                has_bin: None,
                prepare: None,
                bundled_dependencies: None,
                peer_dependencies: None,
                peer_dependencies_meta: None,
            },
        );
        let mut snapshots = HashMap::new();
        snapshots.insert(key.clone(), SnapshotEntry::default());
        let allowed: std::collections::HashSet<String> =
            ["native-pkg".to_string()].into_iter().collect();
        let policy = crate::AllowBuildPolicy::new(allowed, std::collections::HashSet::new(), false);
        let darwin = VirtualStoreLayout::new(
            &config,
            Some("darwin-arm64-node20"),
            Some(&snapshots),
            Some(&packages),
            Some(&policy),
        )
        .slot_dir(&key);
        let linux = VirtualStoreLayout::new(
            &config,
            Some("linux-x64-node22"),
            Some(&snapshots),
            Some(&packages),
            Some(&policy),
        )
        .slot_dir(&key);
        assert_ne!(darwin, linux, "builder snapshot must partition GVS slot by engine string");
    }

    /// Per-snapshot `engines.runtime` resolution: two builder
    /// siblings that pin *different* Node majors must land on
    /// different GVS slots even when given the same install-wide
    /// fallback engine. Mirrors the upstream behaviour in
    /// [`@pnpm/deps.graph-hasher`'s `readSnapshotRuntimePin`
    /// branch](https://github.com/pnpm/pnpm/blob/HEAD/deps/graph-hasher/src/index.ts).
    /// The bin linker spawns each pinning package's lifecycle scripts
    /// through its own downloaded Node, so anchoring the engine
    /// portion of the hash to a single install-wide value would
    /// produce the wrong side-effects-cache key for cross-pinning
    /// installs.
    #[test]
    fn cross_pinning_siblings_get_distinct_slots() {
        let config = make_config(
            true,
            PathBuf::from("/tmp/proj/node_modules/.pnpm"),
            PathBuf::from("/tmp/store/links"),
        );

        let pins_22: PackageKey = "pins-22@1.0.0".parse().unwrap();
        let pins_20: PackageKey = "pins-20@1.0.0".parse().unwrap();
        let node22_key: PackageKey = "node@runtime:22.11.0".parse().unwrap();
        let node20_key: PackageKey = "node@runtime:20.18.0".parse().unwrap();

        let mut packages = HashMap::new();
        let integrities = [
            (
                pins_22.clone(),
                "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            ),
            (
                pins_20.clone(),
                "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
            ),
            (
                node22_key.clone(),
                "sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC",
            ),
            (
                node20_key.clone(),
                "sha512-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD",
            ),
        ];
        for (key, integrity_str) in integrities {
            packages.insert(
                key,
                PackageMetadata {
                    resolution: LockfileResolution::Registry(RegistryResolution {
                        integrity: integrity_str.parse().expect("parse integrity"),
                    }),
                    engines: None,
                    cpu: None,
                    os: None,
                    libc: None,
                    deprecated: None,
                    has_bin: None,
                    prepare: None,
                    bundled_dependencies: None,
                    peer_dependencies: None,
                    peer_dependencies_meta: None,
                },
            );
        }

        // Two builder siblings, each with `dependencies.node:
        // runtime:<major>` — the desugared form upstream's resolver
        // writes for a manifest-level `engines.runtime` declaration.
        let mut pins_22_deps = HashMap::new();
        pins_22_deps.insert(
            PkgName::parse("node").expect("parse pkg name"),
            SnapshotDepRef::Plain("runtime:22.11.0".parse().expect("parse ver-peer")),
        );
        let pins_22_snapshot =
            SnapshotEntry { dependencies: Some(pins_22_deps), ..SnapshotEntry::default() };

        let mut pins_20_deps = HashMap::new();
        pins_20_deps.insert(
            PkgName::parse("node").expect("parse pkg name"),
            SnapshotDepRef::Plain("runtime:20.18.0".parse().expect("parse ver-peer")),
        );
        let pins_20_snapshot =
            SnapshotEntry { dependencies: Some(pins_20_deps), ..SnapshotEntry::default() };

        let mut snapshots = HashMap::new();
        snapshots.insert(pins_22.clone(), pins_22_snapshot);
        snapshots.insert(pins_20.clone(), pins_20_snapshot);
        snapshots.insert(node22_key.clone(), SnapshotEntry::default());
        snapshots.insert(node20_key.clone(), SnapshotEntry::default());

        // Both siblings are approved builders so the engine portion
        // of the hash isn't dropped by the engine-agnostic gating.
        let allowed: std::collections::HashSet<String> =
            ["pins-22".to_string(), "pins-20".to_string()].into_iter().collect();
        let policy = crate::AllowBuildPolicy::new(allowed, std::collections::HashSet::new(), false);

        // Same install-wide fallback for both layout queries — the
        // divergence has to come from the per-snapshot pin lookup.
        let layout = VirtualStoreLayout::new(
            &config,
            Some("darwin;arm64;node24"),
            Some(&snapshots),
            Some(&packages),
            Some(&policy),
        );
        let slot_22 = layout.slot_dir(&pins_22);
        let slot_20 = layout.slot_dir(&pins_20);
        assert_ne!(slot_22, slot_20, "cross-pinning builders must land on distinct GVS slots");
    }
}
