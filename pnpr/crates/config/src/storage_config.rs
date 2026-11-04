//! Configuration for authoritative storage and disposable proxy cache.

use crate::{
    ConfigFile, Deserialize, HostedStoreConfig, LibsqlSettings, RegistryError,
    backend_config::build_libsql_settings,
    byte_size::{ByteSize, parse_byte_size},
    config_file::{CacheFile, LibsqlFile},
    upstream::{Interval, parse_interval},
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Configuration for the proxy cache budget and retention policies.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CacheConfig {
    /// Maximum size of the cache in bytes. When set, LRU eviction reclaims
    /// the oldest files when total cache size exceeds this limit.
    pub max_size: Option<u64>,
    /// Maximum age of cached entries. Files older than this duration are
    /// evicted during cache cleanup.
    pub max_age: Option<Duration>,
}

/// Configuration for authoritative package storage and disposable proxy cache.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Directory under which authoritative packuments and tarballs live.
    pub hosted_dir: PathBuf,
    /// Directory under which the disposable proxy cache lives.
    pub cache_dir: PathBuf,
    /// Settings for proxy-cache budget, retention, and eviction.
    pub cache: CacheConfig,
    /// Where the authoritative (hosted) store lives (local filesystem or S3).
    pub hosted_backend: HostedStoreConfig,
    /// Where the hosted packages' metadata lives.
    pub metadata: MetadataBackend,
}

/// Where the hosted packages' metadata lives: package documents, tags,
/// deprecations. Archives stay in the hosted store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataBackend {
    /// An embedded `SQLite` database at this path. Single-instance only, so
    /// only with the filesystem hosted store.
    Sqlite(PathBuf),
    /// A libSQL database, such as a Turso primary, shared by every instance
    /// and read through an embedded replica when one is configured.
    Libsql(LibsqlSettings),
}

/// Disk shape of the `metadata:` block.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MetadataFile {
    /// The metadata database: `sqlite:<path>`, with a relative path resolved
    /// against the config file's directory, or a libSQL database
    /// (`libsql://`, or `http://` and `https://` for a self-hosted `sqld`).
    /// Without it, a filesystem hosted store keeps its metadata in
    /// [`default_metadata_path`].
    #[serde(default)]
    url: Option<String>,
    /// The libSQL database's bearer token.
    #[serde(default)]
    auth_token: Option<String>,
    /// Where the libSQL database's embedded replica lives.
    #[serde(default)]
    replica_path: Option<PathBuf>,
    /// How often the embedded replica pulls from the primary, which bounds
    /// how stale a replica read may be.
    #[serde(default)]
    sync_interval: Option<Interval>,
    /// Most libSQL operations running at once.
    #[serde(default)]
    max_connections: Option<u32>,
    /// The metadata backend of earlier releases. pnpr keeps metadata only in
    /// the database `url` names, so any value is refused.
    #[serde(default)]
    backend: Option<serde::de::IgnoredAny>,
}

/// The default `SQLite` metadata database of a filesystem hosted store. The
/// leading dot keeps it out of the package listing.
#[must_use]
pub fn default_metadata_path(storage: &Path) -> PathBuf {
    storage.join(".metadata").join("metadata.sqlite")
}

pub(crate) fn resolve_metadata_backend(
    file: Option<&MetadataFile>,
    storage: &Path,
    base_dir: &Path,
    hosted: &HostedStoreConfig,
    hosts_packages: bool,
) -> Result<MetadataBackend, RegistryError> {
    // A tier that hosts no packages, such as an artifact-only one, never
    // reads package metadata, whichever store its bucket is.
    let on_fs = matches!(hosted, HostedStoreConfig::Fs) || !hosts_packages;
    let Some(file) = file else { return default_metadata(storage, on_fs) };
    if file.backend.is_some() {
        return Err(invalid(
            "metadata.backend is no longer supported: pnpr keeps package metadata only in the \
             database metadata.url names. Remove metadata.backend, and set metadata.url to \
             `sqlite:<path>` or a libSQL URL, or leave it out to use the SQLite database in the \
             storage directory",
        ));
    }
    match &file.url {
        None if has_libsql_settings(file) => Err(invalid(LIBSQL_ONLY_SETTINGS)),
        None => default_metadata(storage, on_fs),
        Some(url) if is_libsql_url(url) => libsql_metadata(file, url, base_dir),
        Some(url) => sqlite_metadata(file, url, base_dir, on_fs),
    }
}

/// The metadata of a store without a metadata URL: `SQLite` in a filesystem
/// store, or in a tier that hosts no packages. An object store of packages
/// is shared by instances a local database would not be shared by, so it
/// needs a URL.
fn default_metadata(storage: &Path, on_fs: bool) -> Result<MetadataBackend, RegistryError> {
    if on_fs {
        Ok(MetadataBackend::Sqlite(default_metadata_path(storage)))
    } else {
        Err(invalid(
            "an `s3:` hosted store needs metadata.url to name a libSQL database, such as \
             `libsql://<host>`: pnpr keeps package metadata in a database, not in the bucket",
        ))
    }
}

/// Why settings only a libSQL URL takes are refused anywhere else.
const LIBSQL_ONLY_SETTINGS: &str = "metadata.authToken, replicaPath, syncInterval, and \
                                    maxConnections apply only to a libSQL metadata URL";

/// Whether `file` sets anything only a libSQL URL takes.
fn has_libsql_settings(file: &MetadataFile) -> bool {
    file.auth_token.is_some()
        || file.replica_path.is_some()
        || file.sync_interval.is_some()
        || file.max_connections.is_some()
}

fn invalid(reason: &str) -> RegistryError {
    RegistryError::InvalidConfig { reason: reason.to_string() }
}

/// `SQLite` metadata at the path of a `sqlite:` URL.
fn sqlite_metadata(
    file: &MetadataFile,
    url: &str,
    base_dir: &Path,
    on_fs: bool,
) -> Result<MetadataBackend, RegistryError> {
    let Some(path) = url.strip_prefix("sqlite:") else {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "metadata.url {url:?} names an unsupported database; use `sqlite:<path>` or \
                 `libsql://<host>`",
            ),
        });
    };
    if has_libsql_settings(file) {
        return Err(invalid(LIBSQL_ONLY_SETTINGS));
    }
    if path.is_empty() {
        return Err(invalid("metadata.url `sqlite:` needs a path after the colon"));
    }
    if !on_fs {
        return Err(invalid(
            "metadata.url `sqlite:` is single-instance and needs the filesystem hosted store; \
             remove the `s3:` block or the metadata URL",
        ));
    }
    Ok(MetadataBackend::Sqlite(resolve_relative(path, base_dir)))
}

/// Whether `url` names a libSQL database: `libsql://`, or the HTTP a
/// self-hosted `sqld` speaks.
fn is_libsql_url(url: &str) -> bool {
    ["libsql://", "http://", "https://"]
        .iter()
        .any(|scheme| {
            url.get(..scheme.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
        })
}

/// libSQL metadata, which any hosted store can use: it is shared by every
/// instance.
fn libsql_metadata(
    file: &MetadataFile,
    url: &str,
    base_dir: &Path,
) -> Result<MetadataBackend, RegistryError> {
    let settings = build_libsql_settings(
        LibsqlFile {
            url: url.to_string(),
            auth_token: file.auth_token.clone(),
            replica_path: file.replica_path.clone(),
            sync_interval: file.sync_interval.clone(),
            max_connections: file.max_connections,
        },
        base_dir,
        "metadata",
    )?;
    if settings.replica_path.is_some() && settings.sync_interval_secs == Some(0) {
        return Err(invalid(
            "metadata.syncInterval must be at least 1s with a replicaPath: it bounds how stale a \
             replica read may be",
        ));
    }
    Ok(MetadataBackend::Libsql(settings))
}

pub(crate) fn default_storage_string() -> String {
    "./storage".to_string()
}

/// The default location for the disposable proxy cache directory.
#[must_use]
pub fn default_cache_dir(storage: &Path) -> PathBuf {
    storage.join(".pnpr-cache")
}

pub(crate) fn resolve_relative(raw: &str, base_dir: &Path) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path;
    }
    base_dir.join(path)
}

pub(crate) fn resolve_storage_paths(
    file: &ConfigFile,
    base_dir: &Path,
) -> Result<(PathBuf, PathBuf, CacheConfig), RegistryError> {
    let storage = resolve_relative(&file.storage, base_dir);
    let (cache_dir, cache) = match &file.cache {
        None => (default_cache_dir(&storage), CacheConfig::default()),
        Some(CacheFile::Path(raw)) => (resolve_relative(raw, base_dir), CacheConfig::default()),
        Some(CacheFile::Settings(settings)) => {
            let cache_dir = settings.path
                .as_deref()
                .map(|raw| resolve_relative(raw, base_dir))
                .unwrap_or_else(|| default_cache_dir(&storage));
            let max_size = settings.max_size
                .as_ref()
                .map(|ByteSize(raw)| {
                    parse_byte_size(raw)
                        .ok_or_else(|| RegistryError::InvalidConfig {
                            reason: format!("cache.maxSize: invalid byte size {raw:?}"),
                        })
                })
                .transpose()?;
            let max_age = settings.max_age
                .as_ref()
                .map(|Interval(raw)| {
                    parse_interval(raw)
                        .ok_or_else(|| RegistryError::InvalidConfig {
                            reason: format!("cache.maxAge: invalid interval {raw:?}"),
                        })
                })
                .transpose()?;
            (cache_dir, CacheConfig { max_size, max_age })
        }
    };
    Ok((storage, cache_dir, cache))
}
