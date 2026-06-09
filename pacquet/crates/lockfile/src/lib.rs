mod catalog_snapshots;
mod comver;
mod env_lockfile;
mod freshness;
mod load_lockfile;
mod lockfile_version;
mod package_metadata;
mod pkg_id_with_patch_hash;
mod pkg_name;
mod pkg_name_suffix;
mod pkg_name_ver;
mod pkg_name_ver_peer;
mod pkg_ver_peer;
mod project_snapshot;
mod resolution;
mod resolved_dependency;
mod save_lockfile;
mod serialize_yaml;
mod snapshot_dep_ref;
mod snapshot_entry;
mod yaml_documents;
mod yaml_emit;

pub use catalog_snapshots::*;
pub use comver::*;
pub use env_lockfile::*;
pub use freshness::*;
pub use load_lockfile::*;
pub use lockfile_version::*;
pub use package_metadata::*;
pub use pkg_id_with_patch_hash::*;
pub use pkg_name::*;
pub use pkg_name_suffix::*;
pub use pkg_name_ver::*;
pub use pkg_name_ver_peer::*;
pub use pkg_ver_peer::*;
pub use project_snapshot::*;
pub use resolution::*;
pub use resolved_dependency::*;
pub use save_lockfile::*;
pub use snapshot_dep_ref::*;
pub use snapshot_entry::*;
pub use yaml_documents::*;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Package key used by the `packages:` and `snapshots:` maps in a v9 lockfile.
///
/// Example: `react-dom@17.0.2(react@17.0.2)`.
pub type PackageKey = PkgNameVerPeer;

/// Default `peersSuffixMaxLength` an unset `settings.peersSuffixMaxLength`
/// in the lockfile decays to. Matches pnpm's
/// [`createPeerDepGraphHash` parameter default](https://github.com/pnpm/pnpm/blob/39101f5e37/deps/path/src/index.ts#L197)
/// and the `1000` filter at
/// [`lockfileFormatConverters.ts`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/fs/src/lockfileFormatConverters.ts#L67-L69)
/// that strips the field on serialization when it equals this value.
pub const DEFAULT_PEERS_SUFFIX_MAX_LENGTH: u64 = 1000;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockfileSettings {
    pub auto_install_peers: bool,
    /// Recorded as `Some(true)` when the install ran with
    /// `dedupePeers` on, omitted otherwise. Mirrors pnpm's
    /// [`dedupePeers: opts.dedupePeers || undefined`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/src/install/index.ts#L602)
    /// — the lockfile only carries the key when the setting is
    /// active, so a switch from the default off to on triggers the
    /// `getOutdatedLockfileSetting('settings.dedupePeers')` branch on
    /// the next install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_peers: Option<bool>,
    pub exclude_links_from_lockfile: bool,
    /// `injectWorkspacePackages` recorded by the install that wrote
    /// this lockfile. `false` round-trips as a missing key — pnpm's
    /// [`lockfileFormatConverters.ts:70-72`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/fs/src/lockfileFormatConverters.ts#L70-L72)
    /// strips the key on save so historic v9 lockfiles (which never
    /// carried it) stay byte-identical after a re-save. The drift
    /// gate at
    /// [`getOutdatedLockfileSetting.ts:80-82`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L80-L82)
    /// reads through `Boolean(...)` so missing and `false` are
    /// equivalent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inject_workspace_packages: bool,
    /// Cap that drove this lockfile's peer-suffix rendering. Omitted
    /// from the serialized file when it equals the default ([`DEFAULT_PEERS_SUFFIX_MAX_LENGTH`])
    /// so existing lockfiles round-trip byte-for-byte; mirrors upstream's
    /// strip at <https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/fs/src/lockfileFormatConverters.ts#L67-L69>.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peers_suffix_max_length: Option<u64>,
}

/// A pnpm v9 lockfile.
///
/// Specification: <https://github.com/pnpm/spec/blob/834f2815cc/lockfile/9.0.md>
/// Reference: <https://github.com/pnpm/pnpm/blob/1819226b51/lockfile/types/src/index.ts>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lockfile {
    pub lockfile_version: LockfileVersion<9>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<LockfileSettings>,

    /// `catalogs:` snapshot — the resolved specifier + version for every
    /// catalog-referenced direct dependency. Sits between `settings` and
    /// `overrides` in pnpm's
    /// [`sortLockfileKeys`](https://github.com/pnpm/pnpm/blob/e7e99f04e4/lockfile/fs/src/sortLockfileKeys.ts#L34-L42)
    /// root-key order, so the field is declared here to serialize in the
    /// same position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalogs: Option<CatalogSnapshots>,

    /// `overrides` recorded by the install that wrote this lockfile.
    /// Kept in an [`IndexMap`] so the entries serialize in the order
    /// the user declared them — pnpm does **not** sort this map (it is
    /// the one lockfile map left out of
    /// [`sortLockfileKeys`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/fs/src/sortLockfileKeys.ts)),
    /// so preserving insertion order is what keeps pacquet byte-stable
    /// *and* faithful to pnpm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overrides: Option<IndexMap<String, String>>,

    /// `packageExtensionsChecksum` recorded by the install that wrote
    /// this lockfile. Top-level in the v9 wire shape, mirroring
    /// upstream's
    /// [`LockfileBase`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/types/src/index.ts#L22).
    /// On a subsequent install, drift between this value and the
    /// freshly-computed checksum of `Config::package_extensions` is
    /// what [`crate::check_lockfile_settings`] flags as outdated,
    /// matching upstream's
    /// [`getOutdatedLockfileSetting.ts:53-55`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L53-L55)
    /// gate. `None` when no `packageExtensions` were configured at
    /// write time — upstream's `hashObjectNullableWithPrefix` short-
    /// circuits to `undefined` on empty input, and pacquet does the
    /// same so an empty `packageExtensions` round-trips identically.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_extensions_checksum: Option<String>,

    /// `pnpmfileChecksum` recorded by the install that wrote this
    /// lockfile — the normalized-content hash of the project's
    /// `.pnpmfile.{cjs,mjs}` when it exports hooks. Top-level in the v9
    /// wire shape, mirroring upstream's
    /// [`LockfileBase`](https://github.com/pnpm/pnpm/blob/1819226b51/lockfile/types/src/index.ts#L24),
    /// and serialized right after `packageExtensionsChecksum` per pnpm's
    /// [`ROOT_KEYS`](https://github.com/pnpm/pnpm/blob/1819226b51/lockfile/fs/src/sortLockfileKeys.ts#L34-L44)
    /// order. `None` when the project has no pnpmfile (or one without a
    /// `hooks` export) — pnpm omits the key in that case, and the
    /// `skip_serializing_if` below does the same so the lockfile
    /// round-trips byte-for-byte.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pnpmfile_checksum: Option<String>,

    /// `ignoredOptionalDependencies` recorded by the install that
    /// wrote this lockfile. Top-level in the v9 wire shape —
    /// **not** inside `settings` — mirroring upstream's
    /// [`LockfileBase`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/types/src/index.ts#L17-L27).
    /// On a subsequent install, drift between this set and
    /// `Config::ignored_optional_dependencies` is what
    /// `satisfies_package_manifest` flags as outdated, matching
    /// upstream's
    /// [`getOutdatedLockfileSetting.ts:58-60`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L58-L60)
    /// gate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored_optional_dependencies: Option<Vec<String>>,

    /// `patchedDependencies` recorded by the install that wrote this
    /// lockfile: each configured `patchedDependencies` key (e.g.
    /// `graceful-fs@4.2.11`) mapped to the SHA-256 hex digest of its
    /// patch file. Top-level in the v9 wire shape, sitting between
    /// `pnpmfileChecksum` and `importers` in pnpm's
    /// [`sortLockfileKeys`](https://github.com/pnpm/pnpm/blob/e7e99f04e4/lockfile/fs/src/sortLockfileKeys.ts#L34-L42)
    /// root-key order. Mirrors upstream's
    /// [`patchedDependencies`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/types/src/index.ts#L23)
    /// field, which records
    /// [`calcPatchHashes(opts.patchedDependencies)`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/src/install/index.ts#L547-L549).
    /// A [`BTreeMap`] so the entries serialize sorted by key, matching
    /// pnpm's `sortDirectKeys` pass over this map.
    ///
    /// [`BTreeMap`]: std::collections::BTreeMap
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patched_dependencies: Option<std::collections::BTreeMap<String, String>>,

    #[serde(
        default,
        skip_serializing_if = "HashMap::is_empty",
        serialize_with = "crate::serialize_yaml::sorted_map"
    )]
    pub importers: HashMap<String, ProjectSnapshot>,

    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub packages: Option<HashMap<PackageKey, PackageMetadata>>,

    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub snapshots: Option<HashMap<PackageKey, SnapshotEntry>>,
}

impl Lockfile {
    /// Base file name of the lockfile.
    pub const FILE_NAME: &str = "pnpm-lock.yaml";

    /// Base file name of the *current* lockfile written under the
    /// virtual store. Mirrors upstream pnpm's `lock.yaml` (the file
    /// that records what was actually materialized in
    /// `node_modules/.pnpm`, as opposed to what the wanted lockfile
    /// asks for).
    ///
    /// Upstream: <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/fs/src/write.ts#L41-L51>.
    pub const CURRENT_FILE_NAME: &str = "lock.yaml";

    /// The key used to refer to the root project inside `importers`.
    pub const ROOT_IMPORTER_KEY: &str = ".";

    /// Convenience accessor for the root project's snapshot.
    #[must_use]
    pub fn root_project(&self) -> Option<&'_ ProjectSnapshot> {
        self.importers.get(Lockfile::ROOT_IMPORTER_KEY)
    }

    /// `true` when no importer in this lockfile has either specifiers
    /// or dependencies recorded. Mirrors upstream's `isEmptyLockfile`
    /// at <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/fs/src/write.ts#L83-L85>;
    /// upstream uses the result to suppress writing
    /// `node_modules/.pnpm/lock.yaml` for an install that resolved to
    /// zero packages. Only `specifiers` and `dependencies` participate
    /// in the check — `devDependencies` and `optionalDependencies`
    /// are ignored to match upstream exactly.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.importers.values().all(|importer| {
            importer.specifiers.as_ref().is_none_or(HashMap::is_empty)
                && importer.dependencies.as_ref().is_none_or(HashMap::is_empty)
        })
    }

    /// Defense-in-depth for pruned lockfiles (older `turbo prune --docker`,
    /// pre vercel/turborepo#12825): a peer-variant injected workspace
    /// snapshot whose base `packages:` entry was dropped is missing its
    /// inherited `resolution`. Reconstruct a directory resolution from
    /// the `file:` depPath so [`crate::PackageMetadata::resolution`]
    /// stays non-optional for downstream callers. Mirrors upstream
    /// `convertToLockfileObject` in `lockfile/fs/src/lockfileFormatConverters.ts`.
    pub fn reconstruct_missing_directory_resolutions(&mut self) {
        let Some(snapshots) = self.snapshots.as_ref() else { return };
        let to_insert: Vec<(PackageKey, DirectoryResolution)> = snapshots
            .keys()
            .filter_map(|snapshot_key| {
                let metadata_key = snapshot_key.without_peer();
                let packages = self.packages.as_ref();
                if packages.is_some_and(|p| p.contains_key(&metadata_key)) {
                    return None;
                }
                let VersionPart::File(path) = metadata_key.suffix.version() else { return None };
                if is_local_tarball_path(path) {
                    return None;
                }
                let directory = path.clone();
                Some((metadata_key, DirectoryResolution { directory }))
            })
            .collect();
        if to_insert.is_empty() {
            return;
        }
        let packages = self.packages.get_or_insert_with(HashMap::new);
        for (key, directory_resolution) in to_insert {
            packages.entry(key).or_insert_with(|| PackageMetadata {
                resolution: LockfileResolution::Directory(directory_resolution),
                version: None,
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
            });
        }
    }
}

/// Mirrors `isFilename` (`/\.(?:tgz|tar\.gz|tar)$/i`) in
/// `resolving/local-resolver/src/parseBareSpecifier.ts` so the directory-vs-
/// tarball boundary applied here matches the resolver's at resolve time.
fn is_local_tarball_path(path: &str) -> bool {
    let lower = path.as_bytes();
    let ends_with_ci = |suffix: &str| {
        let bytes = suffix.as_bytes();
        lower.len() >= bytes.len() && lower[lower.len() - bytes.len()..].eq_ignore_ascii_case(bytes)
    };
    ends_with_ci(".tgz") || ends_with_ci(".tar.gz") || ends_with_ci(".tar")
}
