//! The *env lockfile* — the first YAML document of `pnpm-lock.yaml`.
//!
//! Pnpm v11 records configurational dependencies (and the
//! `packageManager`/`devEngines` bootstrap deps) in a separate YAML
//! document written ahead of the regular project lockfile. The
//! `pnpm-env-installer` crate resolves config deps into this
//! document; the main install path preserves it verbatim when it
//! rewrites the wanted lockfile (see [`crate::save_value_to_path`]).
//!
//! The `packages:` and `snapshots:` maps reuse the same
//! [`PackageMetadata`] / [`SnapshotEntry`] types as the main lockfile, so
//! the env document inherits the main lockfile's byte-for-byte
//! serialization parity. In the next format it has the main document's
//! shape and syntax too, through [`next`].

use crate::{
    LoadLockfileError, Lockfile, LockfileFormat, LockfileVersion, PackageKey, PackageMetadata,
    SaveLockfileError, SnapshotEntry, SymlinkedLockfile, declares_next_format,
    extract_main_document,
    git_merge_file::{ParsedWantedFile, parse_wanted_file},
    merge_env_lockfile_changes,
    save_lockfile::lockfile_write_target,
    serialize_yaml,
    yaml_documents::{
        YAML_DOCUMENT_SEPARATOR, YAML_DOCUMENT_START, normalize_lockfile_content,
        read_first_yaml_document,
    },
};
use pnpm_fs::write_atomic;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{self, ErrorKind},
    path::Path,
};

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
    ///
    /// Only the leading document is read: the dependency graph that
    /// follows it never reaches memory.
    ///
    /// Two branches that each added a config dependency conflict inside
    /// this document, where the main lockfile's own recovery never looks,
    /// so the same merge runs here.
    pub fn read(root_dir: &Path) -> Result<Option<Self>, LoadLockfileError> {
        let path = root_dir.join(Lockfile::FILE_NAME);
        let Some(env_doc) = read_env_document(&path).map_err(LoadLockfileError::ReadFile)? else {
            return Ok(None);
        };
        Ok(Self::parse_conflicted_document(&env_doc, &path)?.value)
    }

    /// [`Self::read`]'s parse of one env document, with the number of
    /// files its Git conflict markers had to be merged out of — 1 or 0,
    /// since it is handed a single document.
    pub(crate) fn parse_conflicted_document(
        env_doc: &str,
        path: &Path,
    ) -> Result<ParsedWantedFile<Self>, LoadLockfileError> {
        parse_wanted_file(env_doc, path, Self::parse_document, merge_env_lockfile_changes)
    }

    fn parse_document(env_doc: &str, path: &Path) -> Result<Option<Self>, LoadLockfileError> {
        let mut env = if declares_next_format(env_doc) {
            next::from_text(env_doc, path)?
        } else {
            serde_saphyr::from_str::<EnvLockfile>(env_doc)
                .map_err(|source| LoadLockfileError::parse_yaml(path, &source))?
        };
        env.root_importer_mut();
        Ok(Some(env))
    }

    /// Write this env document as the first YAML document of
    /// `<root_dir>/pnpm-lock.yaml`, in `format`, the pinned pnpm's (see
    /// [`EnvDocumentFormat::for_pnpm`]), preserving any existing main
    /// document. Emits `---\n${envYaml}\n---\n${mainDoc}`. An
    /// unchanged document is not rewritten, and a changed one treats a
    /// symlinked lockfile as `symlinked` says.
    pub fn write(
        &self,
        root_dir: &Path,
        symlinked: SymlinkedLockfile<'_>,
        format: EnvDocumentFormat,
    ) -> Result<(), SaveLockfileError> {
        let path = root_dir.join(Lockfile::FILE_NAME);
        let env_yaml = self.document_text(format)?;
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => Some(raw),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(SaveLockfileError::WriteFile(error)),
        };
        let existing = raw.as_deref().map(normalize_lockfile_content);
        let main_doc = existing
            .as_deref()
            .map(extract_main_document)
            .unwrap_or_default();
        let combined =
            format!("{YAML_DOCUMENT_START}{env_yaml}{YAML_DOCUMENT_SEPARATOR}{main_doc}");
        if existing.as_deref() == Some(combined.as_str()) {
            return Ok(());
        }
        let target = lockfile_write_target(&path, symlinked).map_err(SaveLockfileError::WriteFile)?;
        write_atomic(&target, combined.as_bytes()).map_err(SaveLockfileError::WriteFile)
    }
}

/// The format of the env document: that of the pnpm major that owns the
/// project, the pinned one, so that the pnpm writing the document and the
/// pinned pnpm agree on it whichever writes. The env document changes
/// format one major after the main document, so that the pnpm versions
/// that switch into a project can read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvDocumentFormat {
    /// Lockfile 9.0's shape and syntax.
    V9,
    /// The next format's shape and syntax, under `lockfileVersion: '13.0'`.
    V13,
}

impl EnvDocumentFormat {
    /// The first pnpm major whose projects have a v13 env document.
    pub const V13_FROM_MAJOR: u64 = 14;

    /// The format for a project that pins, or runs, pnpm `version`. A
    /// version that does not parse gets today's.
    #[must_use]
    pub fn for_pnpm(version: &str) -> Self {
        let major = version
            .split('.')
            .next()
            .and_then(|major| major.parse::<u64>().ok());
        if major.is_some_and(|major| major >= Self::V13_FROM_MAJOR) { Self::V13 } else { Self::V9 }
    }

    fn lockfile_version(self) -> LockfileVersion<9> {
        match self {
            EnvDocumentFormat::V9 => LockfileFormat::V9.version(),
            EnvDocumentFormat::V13 => LockfileFormat::V13.version(),
        }
    }
}

impl EnvLockfile {
    /// This document's text in `format`, which decides its shape and
    /// syntax and the `lockfileVersion` it declares. A document read in
    /// the next format and written in today's declares today's version,
    /// so that it reads back as today's shape; any other version string is
    /// kept, since the env installer does not own it.
    fn document_text(&self, format: EnvDocumentFormat) -> Result<String, SaveLockfileError> {
        match format {
            EnvDocumentFormat::V13 => next::to_text(self, format.lockfile_version()),
            EnvDocumentFormat::V9 if self.declares_next_format() => {
                let mut env = self.clone();
                env.lockfile_version = EnvDocumentFormat::V9.lockfile_version().to_string();
                serialize_yaml::to_string(&env).map_err(SaveLockfileError::SerializeYaml)
            }
            EnvDocumentFormat::V9 => {
                serialize_yaml::to_string(self).map_err(SaveLockfileError::SerializeYaml)
            }
        }
    }

    fn declares_next_format(&self) -> bool {
        self.lockfile_version
            .split('.')
            .next()
            .and_then(|major| major.parse::<u16>().ok())
            == Some(crate::NEXT_LOCKFILE_MAJOR)
    }
}

mod next;

/// Reads the leading env document, following a symlink;
/// [`SymlinkedLockfile`] covers why only writes may refuse one.
fn read_env_document(path: &Path) -> io::Result<Option<String>> {
    match File::open(path) {
        Ok(file) => read_first_yaml_document(file),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

impl EnvLockfile {
    /// The version of the package manager the env document pins, when it
    /// records one.
    #[must_use]
    pub fn pinned_package_manager_version(&self) -> Option<&str> {
        self.importers
            .get(Self::ROOT_IMPORTER_KEY)?
            .package_manager_dependencies
            .as_ref()?
            .values()
            .next()
            .map(|pin| pin.version.as_str())
    }
}

#[cfg(test)]
mod tests;
