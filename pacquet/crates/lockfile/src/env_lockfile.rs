//! The *env lockfile* — the first YAML document of `pnpm-lock.yaml`.
//!
//! Pnpm v11 records configurational dependencies (and the
//! `packageManager`/`devEngines` bootstrap deps) in a separate YAML
//! document written ahead of the regular project lockfile. The
//! `pacquet-env-installer` crate resolves config deps into this
//! document; the main install path preserves it verbatim when it
//! rewrites the wanted lockfile (see [`crate::save_value_to_path`]).
//!
//! Mirrors upstream's `EnvLockfile` type and the
//! `createEnvLockfile` / `readEnvLockfile` / `writeEnvLockfile`
//! helpers at
//! <https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/envLockfile.ts>.
//! The `packages:` and `snapshots:` maps reuse the same
//! [`PackageMetadata`] / [`SnapshotEntry`] types as the main lockfile —
//! upstream uses the identical `LockfilePackageInfo` /
//! `LockfilePackageSnapshot` shapes — so the env document inherits the
//! main lockfile's byte-for-byte serialization parity.

use crate::{
    LoadLockfileError, Lockfile, PackageKey, PackageMetadata, SaveLockfileError, SnapshotEntry,
    extract_env_document, extract_main_document, serialize_yaml,
    yaml_documents::{YAML_DOCUMENT_SEPARATOR, YAML_DOCUMENT_START},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::ErrorKind,
    path::Path,
};

/// The resolved `{ specifier, version }` pair recorded for each config
/// (or package-manager) dependency under an importer. Mirrors upstream's
/// [`SpecifierAndResolution`](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/types/src/index.ts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecifierAndResolution {
    pub specifier: String,
    pub version: String,
}

/// Per-importer entry of the env lockfile. Only the root importer
/// (`.`) is ever populated. Mirrors upstream's
/// [`EnvImporterSnapshot`](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/types/src/index.ts).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvImporterSnapshot {
    /// Always serialized — upstream's `createEnvLockfile` seeds the key
    /// even when empty, so the env document always carries it.
    #[serde(default)]
    pub config_dependencies: BTreeMap<String, SpecifierAndResolution>,
    /// The `packageManager` / `devEngines` bootstrap deps. Omitted when
    /// absent so a config-deps-only env document round-trips identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_manager_dependencies: Option<BTreeMap<String, SpecifierAndResolution>>,
}

/// The env lockfile document.
///
/// Field declaration order is the serialized root-key order
/// (`lockfileVersion`, `importers`, `packages`, `snapshots`), matching
/// the subset of upstream's
/// [`sortLockfileKeys` root order](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/sortLockfileKeys.ts#L33-L44)
/// that an env document uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvLockfile {
    /// A plain string (not the numeric [`crate::LockfileVersion`]),
    /// matching upstream's `EnvLockfile.lockfileVersion: string`. The
    /// reader accepts any value (pnpm checks only that it's a string),
    /// so an env document carrying a non-numeric marker such as
    /// `env-1.0` round-trips instead of hard-failing config-deps
    /// installation.
    pub lockfile_version: String,

    #[serde(default, serialize_with = "crate::serialize_yaml::sorted_map")]
    pub importers: HashMap<String, EnvImporterSnapshot>,

    #[serde(default, serialize_with = "crate::serialize_yaml::sorted_map")]
    pub packages: HashMap<PackageKey, PackageMetadata>,

    #[serde(default, serialize_with = "crate::serialize_yaml::sorted_map")]
    pub snapshots: HashMap<PackageKey, SnapshotEntry>,
}

impl EnvLockfile {
    /// The key used to refer to the root project inside `importers`.
    pub const ROOT_IMPORTER_KEY: &str = ".";

    /// A fresh, empty env lockfile with the root importer seeded.
    /// Mirrors upstream's
    /// [`createEnvLockfile`](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/envLockfile.ts#L14-L24).
    #[must_use]
    pub fn create() -> Self {
        let mut importers = HashMap::new();
        importers.insert(Self::ROOT_IMPORTER_KEY.to_string(), EnvImporterSnapshot::default());
        EnvLockfile {
            // Matches upstream's `createEnvLockfile`, which seeds the
            // `LOCKFILE_VERSION` ("9.0") string.
            lockfile_version: "9.0".to_string(),
            importers,
            packages: HashMap::new(),
            snapshots: HashMap::new(),
        }
    }

    /// Convenience accessor for the root importer's snapshot, creating
    /// it if absent. The env-installer always operates on `.`.
    pub fn root_importer_mut(&mut self) -> &mut EnvImporterSnapshot {
        self.importers.entry(Self::ROOT_IMPORTER_KEY.to_string()).or_default()
    }

    /// Read the env document (first YAML document) from
    /// `<root_dir>/pnpm-lock.yaml`. Mirrors upstream's
    /// [`readEnvLockfile`](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/envLockfile.ts#L26-L57):
    ///
    /// - Returns `Ok(None)` when the lockfile is absent, or carries no
    ///   leading env document.
    /// - Otherwise parses the env document and guarantees the root
    ///   importer (and its `configDependencies` map) exists.
    pub fn read(root_dir: &Path) -> Result<Option<Self>, LoadLockfileError> {
        let path = root_dir.join(Lockfile::FILE_NAME);
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(LoadLockfileError::ReadFile(error)),
        };
        let Some(env_doc) = extract_env_document(&content) else {
            return Ok(None);
        };
        let mut env: EnvLockfile =
            serde_saphyr::from_str(env_doc).map_err(LoadLockfileError::ParseYaml)?;
        env.root_importer_mut();
        Ok(Some(env))
    }

    /// Write this env document as the first YAML document of
    /// `<root_dir>/pnpm-lock.yaml`, preserving any existing main
    /// document. Mirrors upstream's
    /// [`writeEnvLockfile`](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/envLockfile.ts#L59-L77),
    /// which writes `---\n${envYaml}\n---\n${mainDoc}`.
    pub fn write(&self, root_dir: &Path) -> Result<(), SaveLockfileError> {
        let path = root_dir.join(Lockfile::FILE_NAME);
        let env_yaml = serialize_yaml::to_string(self).map_err(SaveLockfileError::SerializeYaml)?;
        let main_doc = match fs::read_to_string(&path) {
            Ok(existing) => extract_main_document(&existing).to_string(),
            Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
            Err(error) => return Err(SaveLockfileError::WriteFile(error)),
        };
        let combined =
            format!("{YAML_DOCUMENT_START}{env_yaml}{YAML_DOCUMENT_SEPARATOR}{main_doc}");
        fs::write(&path, combined).map_err(SaveLockfileError::WriteFile)
    }
}

#[cfg(test)]
mod tests;
