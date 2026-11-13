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
    /// Where the database lives.
    pub database: DatabaseBackend,
}

/// Where the database lives: package documents, tags, accounts, registries,
/// and the rest of pnpr's state. Archives stay in the hosted store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseBackend {
    /// An embedded `SQLite` database at this path. Single-instance only, so
    /// only with the filesystem hosted store.
    Sqlite(PathBuf),
    Postgres(crate::SqlBackendSettings),
    Mysql(crate::SqlBackendSettings),
    /// A libSQL database, such as a Turso primary, shared by every instance
    /// and read through an embedded replica when one is configured.
    Libsql(LibsqlSettings),
}

/// Disk shape of the `storage.database` block.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DatabaseFile {
    /// The database: `sqlite:<path>`, with a relative path resolved
    /// against the config file's directory, `postgres://`, `mysql://`, or
    /// libSQL (`libsql://`, `http://`, or `https://`).
    /// Without it, a filesystem hosted store keeps its metadata in
    /// [`default_database_path`].
    #[serde(default)]
    url: Option<crate::Secret>,
    /// The libSQL database's bearer token.
    #[serde(default)]
    auth_token: Option<crate::Secret>,
    /// Where the libSQL database's embedded replica lives.
    #[serde(default)]
    replica_path: Option<PathBuf>,
    /// How often the embedded replica pulls from the primary, which bounds
    /// how stale a replica read may be.
    #[serde(default)]
    sync_interval: Option<Interval>,
    /// Maximum concurrent libSQL operations or PostgreSQL/MySQL pool connections.
    #[serde(default)]
    max_connections: Option<u32>,
    /// PostgreSQL/MySQL operation and pool-acquisition timeout.
    #[serde(default)]
    timeout: Option<Interval>,
    /// PostgreSQL/MySQL initial connection and schema-operation timeout.
    #[serde(default)]
    startup_timeout: Option<Interval>,
}

/// The `storage:` value: the storage directory, or a table that also names
/// the hosted store and the database.
#[derive(Debug)]
pub(crate) enum StorageFile {
    Path(String),
    // Boxed: the table is far larger than a path.
    Settings(Box<StorageSettingsFile>),
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StorageSettingsFile {
    /// The storage directory. Omitted ⇒ [`DEFAULT_STORAGE_PATH`].
    #[serde(default)]
    pub(crate) path: Option<String>,
    /// Store the hosted (published) packages in an S3-compatible object
    /// store instead of `path`.
    #[serde(default)]
    pub(crate) s3: Option<crate::S3Settings>,
    /// The database for accounts, tokens, and package metadata.
    #[serde(default)]
    pub(crate) database: Option<DatabaseFile>,
}

pub(crate) const DEFAULT_STORAGE_PATH: &str = "./storage";

impl StorageFile {
    pub(crate) fn into_settings(self) -> StorageSettingsFile {
        match self {
            Self::Path(path) => StorageSettingsFile { path: Some(path), ..Default::default() },
            Self::Settings(settings) => *settings,
        }
    }
}

/// Hand-written rather than `#[serde(untagged)]`, which would replace the
/// located error of a nested field, such as an unknown key or a refused
/// [`crate::Secret`], with one that names no field.
impl<'de> Deserialize<'de> for StorageFile {
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(StorageFileVisitor)
    }
}

struct StorageFileVisitor;

impl<'de> serde::de::Visitor<'de> for StorageFileVisitor {
    type Value = StorageFile;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a storage path or a table of `path`, `s3`, and `database`")
    }

    fn visit_str<DeError: serde::de::Error>(self, value: &str) -> Result<StorageFile, DeError> {
        Ok(StorageFile::Path(value.to_string()))
    }

    fn visit_map<Map: serde::de::MapAccess<'de>>(
        self,
        map: Map,
    ) -> Result<StorageFile, Map::Error> {
        StorageSettingsFile::deserialize(serde::de::value::MapAccessDeserializer::new(map))
            .map(|settings| StorageFile::Settings(Box::new(settings)))
    }
}

/// The default `SQLite` database of a filesystem hosted store. The
/// leading dot keeps it out of the package listing.
#[must_use]
pub fn default_database_path(storage: &Path) -> PathBuf {
    storage.join(".metadata").join("metadata.sqlite")
}

pub(crate) fn resolve_database_backend(
    file: Option<&DatabaseFile>,
    storage: &Path,
    base_dir: &Path,
    hosted: &HostedStoreConfig,
    hosts_packages: bool,
) -> Result<DatabaseBackend, RegistryError> {
    // A tier that hosts no packages, such as an artifact-only one, never
    // reads package metadata, whichever store its bucket is.
    let on_fs = matches!(hosted, HostedStoreConfig::Fs) || !hosts_packages;
    let Some(file) = file else { return default_metadata(storage, on_fs) };
    if (file.timeout.is_some() || file.startup_timeout.is_some())
        && !file.url.as_deref().is_some_and(is_network_sql_url)
    {
        return Err(invalid(
            "storage.database.timeout and startupTimeout require a PostgreSQL or MySQL URL",
        ));
    }
    match &file.url {
        None if has_remote_settings(file) => Err(invalid(REMOTE_ONLY_SETTINGS)),
        None => default_metadata(storage, on_fs),
        Some(url) if is_network_sql_url(url) => network_database(file, url),
        Some(url) if is_libsql_url(url) => libsql_database(file, url, base_dir),
        Some(url) => sqlite_database(file, url, base_dir, on_fs),
    }
}

/// The metadata of a store without a metadata URL: `SQLite` in a filesystem
/// store, or in a tier that hosts no packages. An object store of packages
/// is shared by instances a local database would not be shared by, so it
/// needs a URL.
fn default_metadata(storage: &Path, on_fs: bool) -> Result<DatabaseBackend, RegistryError> {
    if on_fs {
        Ok(DatabaseBackend::Sqlite(default_database_path(storage)))
    } else {
        Err(invalid(
            "a `storage.s3` hosted store needs storage.database.url to name a shared database, such as \
             `libsql://<host>`: pnpr keeps package metadata in a database, not in the bucket",
        ))
    }
}

const REMOTE_ONLY_SETTINGS: &str = "storage.database.authToken, replicaPath, and syncInterval require a \
    libSQL URL. storage.database.maxConnections requires a libSQL, PostgreSQL, or MySQL URL";

fn has_remote_settings(file: &DatabaseFile) -> bool {
    file.auth_token.is_some()
        || file.replica_path.is_some()
        || file.sync_interval.is_some()
        || file.max_connections.is_some()
}

fn invalid(reason: &str) -> RegistryError {
    RegistryError::InvalidConfig { reason: reason.to_string() }
}

/// `SQLite` metadata at the path of a `sqlite:` URL.
fn sqlite_database(
    file: &DatabaseFile,
    url: &str,
    base_dir: &Path,
    on_fs: bool,
) -> Result<DatabaseBackend, RegistryError> {
    let Some(path) = url.strip_prefix("sqlite:") else {
        return Err(RegistryError::InvalidConfig {
            reason: "storage.database.url names an unsupported database; use `sqlite:<path>`, `postgres://`, `mysql://`, or `libsql://<host>`".to_string(),
        });
    };
    if has_remote_settings(file) {
        return Err(invalid(REMOTE_ONLY_SETTINGS));
    }
    if path.is_empty() {
        return Err(invalid("storage.database.url `sqlite:` needs a path after the colon"));
    }
    if !on_fs {
        return Err(invalid(
            "storage.database.url `sqlite:` is single-instance and needs the filesystem hosted store; \
             remove the `storage.s3` block or the metadata URL",
        ));
    }
    Ok(DatabaseBackend::Sqlite(resolve_relative(path, base_dir)))
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
fn libsql_database(
    file: &DatabaseFile,
    url: &str,
    base_dir: &Path,
) -> Result<DatabaseBackend, RegistryError> {
    let settings = build_libsql_settings(
        LibsqlFile {
            url: url.to_string(),
            auth_token: file.auth_token.clone().map(crate::Secret::into_inner),
            replica_path: file.replica_path.clone(),
            sync_interval: file.sync_interval.clone(),
            max_connections: file.max_connections,
        },
        base_dir,
        "storage.database",
    )?;
    if settings.replica_path.is_some() && settings.sync_interval_secs == Some(0) {
        return Err(invalid(
            "storage.database.syncInterval must be at least 1s with a replicaPath: it bounds how stale a \
             replica read may be",
        ));
    }
    Ok(DatabaseBackend::Libsql(settings))
}

/// The default location for the disposable proxy cache directory.
#[must_use]
pub fn default_cache_dir(storage: &Path) -> PathBuf {
    storage.join(".pnpr-cache")
}

/// The default location for the local OSV npm vulnerability database under
/// `storage`. The leading dot keeps it out of hosted package listings and
/// unmarked-layout checks.
#[must_use]
pub fn default_osv_path(storage: &Path) -> PathBuf {
    storage
        .join(".osv")
        .join("npm")
        .join("all.zip")
}

pub(crate) fn resolve_relative(raw: &str, base_dir: &Path) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path;
    }
    base_dir.join(path)
}

/// The storage directory at `storage_path`, or at the default path, with the
/// cache directory and its settings.
pub(crate) fn resolve_storage_paths(
    file: &ConfigFile,
    storage_path: Option<&str>,
    base_dir: &Path,
) -> Result<(PathBuf, PathBuf, CacheConfig), RegistryError> {
    let storage = resolve_relative(storage_path.unwrap_or(DEFAULT_STORAGE_PATH), base_dir);
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

fn network_database(file: &DatabaseFile, url: &str) -> Result<DatabaseBackend, RegistryError> {
    if file.auth_token.is_some()
        || file.replica_path.is_some()
        || file.sync_interval.is_some()
    {
        return Err(invalid(
            "storage.database.authToken, replicaPath, and syncInterval require a libSQL URL",
        ));
    }
    if file.max_connections == Some(0) {
        return Err(invalid("storage.database.maxConnections must be greater than 0"));
    }
    let mysql = url
        .split_once(':')
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("mysql"));
    let settings = crate::backend_config::build_sql_backend_settings(crate::SqlBackendFile {
        url: url.to_string(),
        max_connections: file.max_connections,
        timeout: file.timeout.clone(),
        startup_timeout: file.startup_timeout.clone(),
    })?;
    Ok(if mysql { DatabaseBackend::Mysql(settings) } else { DatabaseBackend::Postgres(settings) })
}

fn is_network_sql_url(url: &str) -> bool {
    ["postgres://", "postgresql://", "mysql://"]
        .iter()
        .any(|prefix| {
            url.get(..prefix.len())
                .is_some_and(|scheme| scheme.eq_ignore_ascii_case(prefix))
        })
}
