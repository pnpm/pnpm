//! The *env lockfile* — the first YAML document of `pnpm-lock.yaml`.
//!
//! Pnpm v11 records configurational dependencies (and the
//! `packageManager`/`devEngines` bootstrap deps) in a separate YAML
//! document written ahead of the regular project lockfile. The
//! `pacquet-env-installer` crate resolves config deps into this
//! document; the main install path preserves it verbatim when it
//! rewrites the wanted lockfile (see [`crate::save_value_to_path`]).
//!
//! The `packages:` and `snapshots:` maps reuse the same
//! [`PackageMetadata`] / [`SnapshotEntry`] types as the main lockfile, so
//! the env document inherits the main lockfile's byte-for-byte
//! serialization parity.

use crate::{
    LoadLockfileError, Lockfile, PackageKey, PackageMetadata, SaveLockfileError, SnapshotEntry,
    extract_env_document, extract_main_document,
    save_lockfile::{ensure_lockfile_is_not_symlink, symlinked_lockfile_error},
    serialize_yaml,
    yaml_documents::{YAML_DOCUMENT_SEPARATOR, YAML_DOCUMENT_START},
};
use pacquet_fs::write_atomic;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{self, ErrorKind, Read as _},
    path::Path,
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt as _;

/// The resolved `{ specifier, version }` pair recorded for each config
/// (or package-manager) dependency under an importer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecifierAndResolution {
    pub specifier: String,
    pub version: String,
}

/// Per-importer entry of the env lockfile. Only the root importer
/// (`.`) is ever populated.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvImporterSnapshot {
    /// Always serialized — the key is seeded even when empty, so the env
    /// document always carries it.
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
/// (`lockfileVersion`, `importers`, `packages`, `snapshots`), the subset
/// of the lockfile root-key order that an env document uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvLockfile {
    /// A plain string (not the numeric [`crate::LockfileVersion`]): the
    /// env document records `lockfileVersion` as a string.
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
    #[must_use]
    pub fn create() -> Self {
        let mut importers = HashMap::new();
        importers.insert(Self::ROOT_IMPORTER_KEY.to_string(), EnvImporterSnapshot::default());
        EnvLockfile {
            // Seeds the `lockfileVersion` "9.0" string.
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
    /// `<root_dir>/pnpm-lock.yaml`:
    ///
    /// - Returns `Ok(None)` when the lockfile is absent, or carries no
    ///   leading env document.
    /// - Otherwise parses the env document and guarantees the root
    ///   importer (and its `configDependencies` map) exists.
    pub fn read(root_dir: &Path) -> Result<Option<Self>, LoadLockfileError> {
        let path = root_dir.join(Lockfile::FILE_NAME);
        let Some(content) = read_lockfile_to_string(&path).map_err(LoadLockfileError::ReadFile)?
        else {
            return Ok(None);
        };
        let Some(env_doc) = extract_env_document(&content) else {
            return Ok(None);
        };
        let mut env: EnvLockfile = serde_saphyr::from_str(env_doc)
            .map_err(|source| LoadLockfileError::parse_yaml(&path, &source))?;
        env.root_importer_mut();
        Ok(Some(env))
    }

    /// Write this env document as the first YAML document of
    /// `<root_dir>/pnpm-lock.yaml`, preserving any existing main
    /// document. Emits `---\n${envYaml}\n---\n${mainDoc}`.
    pub fn write(&self, root_dir: &Path) -> Result<(), SaveLockfileError> {
        let path = root_dir.join(Lockfile::FILE_NAME);
        ensure_lockfile_is_not_symlink(&path).map_err(SaveLockfileError::WriteFile)?;
        let env_yaml = serialize_yaml::to_string(self).map_err(SaveLockfileError::SerializeYaml)?;
        let main_doc = read_lockfile_to_string_no_follow(&path)
            .map_err(SaveLockfileError::WriteFile)?
            .map_or_else(String::new, |existing| extract_main_document(&existing).to_string());
        let combined =
            format!("{YAML_DOCUMENT_START}{env_yaml}{YAML_DOCUMENT_SEPARATOR}{main_doc}");
        write_atomic(&path, combined.as_bytes()).map_err(SaveLockfileError::WriteFile)
    }
}

fn read_lockfile_to_string_no_follow(path: &Path) -> io::Result<Option<String>> {
    read_lockfile_to_string_with(path, open_lockfile_no_follow)
}

/// Reads a whole lockfile, following a symlink;
/// [`ensure_lockfile_is_not_symlink`] covers why only writes refuse one.
fn read_lockfile_to_string(path: &Path) -> io::Result<Option<String>> {
    read_lockfile_to_string_with(path, |path| File::open(path))
}

fn read_lockfile_to_string_with(
    path: &Path,
    open_file: impl FnOnce(&Path) -> io::Result<File>,
) -> io::Result<Option<String>> {
    let mut file = match open_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut content = String::new();
    #[expect(
        clippy::verbose_file_reads,
        reason = "Reading from the caller's file handle avoids reopening the lockfile by path."
    )]
    file.read_to_string(&mut content)?;
    Ok(Some(content))
}

#[cfg(unix)]
fn open_lockfile_no_follow(path: &Path) -> io::Result<File> {
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| normalize_no_follow_error(path, error))
}

#[cfg(not(unix))]
fn open_lockfile_no_follow(path: &Path) -> io::Result<File> {
    ensure_lockfile_is_not_symlink(path)?;
    File::open(path)
}

#[cfg(unix)]
fn normalize_no_follow_error(path: &Path, error: io::Error) -> io::Error {
    if error.raw_os_error() == Some(libc::ELOOP) { symlinked_lockfile_error(path) } else { error }
}

#[cfg(test)]
mod tests;
