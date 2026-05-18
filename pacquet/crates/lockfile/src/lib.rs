mod comver;
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

pub use comver::*;
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

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Package key used by the `packages:` and `snapshots:` maps in a v9 lockfile.
///
/// Example: `react-dom@17.0.2(react@17.0.2)`.
pub type PackageKey = PkgNameVerPeer;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockfileSettings {
    pub auto_install_peers: bool,
    pub exclude_links_from_lockfile: bool,
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

    #[serde(skip_serializing_if = "Option::is_none")]
    pub overrides: Option<HashMap<String, String>>,

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

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub importers: HashMap<String, ProjectSnapshot>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub packages: Option<HashMap<PackageKey, PackageMetadata>>,

    #[serde(skip_serializing_if = "Option::is_none")]
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
    pub fn is_empty(&self) -> bool {
        self.importers.values().all(|importer| {
            importer.specifiers.as_ref().is_none_or(HashMap::is_empty)
                && importer.dependencies.as_ref().is_none_or(HashMap::is_empty)
        })
    }
}
