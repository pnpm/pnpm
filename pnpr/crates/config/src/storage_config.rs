//! Configuration for authoritative storage and disposable proxy cache.

use crate::{
    ConfigFile, HostedStoreConfig, RegistryError,
    byte_size::{ByteSize, parse_byte_size},
    config_file::CacheFile,
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
                            reason: format!("cache.max_size: invalid byte size {raw:?}"),
                        })
                })
                .transpose()?;
            let max_age = settings.max_age
                .as_ref()
                .map(|Interval(raw)| {
                    parse_interval(raw)
                        .ok_or_else(|| RegistryError::InvalidConfig {
                            reason: format!("cache.max_age: invalid interval {raw:?}"),
                        })
                })
                .transpose()?;
            (cache_dir, CacheConfig { max_size, max_age })
        }
    };
    Ok((storage, cache_dir, cache))
}
