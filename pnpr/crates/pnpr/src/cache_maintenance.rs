//! Upstream proxy cache reclamation, orphan pruning, and LRU eviction.
//!
//! Reclaims orphaned upstream cache namespaces left behind by per-process
//! secrets or removed/reconfigured upstreams, evicts stale cached entries
//! exceeding TTL, and enforces cache size caps via LRU eviction.

use crate::{Config, Result};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tokio::fs;

/// Summary of items evicted and reclaimed during cache maintenance.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CacheReport {
    /// Number of orphaned namespace directories removed.
    pub namespaces_removed: usize,
    /// Number of individual cache files evicted.
    pub files_evicted: usize,
    /// Total bytes of cache storage reclaimed.
    pub bytes_reclaimed: u64,
}

/// Options controlling cache reclamation and eviction.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CacheGcOptions {
    /// Report reclaimable entries without deleting anything.
    pub dry_run: bool,
    /// Maximum total cache size to retain. Oldest files are evicted first.
    pub max_size: Option<u64>,
    /// Maximum age of cached entries. Files older than this are deleted.
    pub max_age: Option<Duration>,
    /// Only prune orphaned upstream namespaces without evicting active cache files.
    pub orphans_only: bool,
}

struct CachedFile {
    path: PathBuf,
    size: u64,
    effective_time: SystemTime,
    age: Duration,
}

/// The set of active cache namespaces derived from configured upstreams.
#[must_use]
pub fn active_cache_namespaces(config: &Config) -> HashSet<String> {
    config.routing.upstreams
        .keys()
        .map(|name| crate::server::compute_upstream_cache_namespace(config, name))
        .collect()
}

/// Run a complete cache collection pass according to the given options.
pub async fn collect_cache(config: &Config, options: &CacheGcOptions) -> Result<CacheReport> {
    let active = active_cache_namespaces(config);
    let mut report = prune_orphaned_namespaces(
        &config.storage.cache_dir,
        &active,
        options.dry_run,
        config.has_configured_secret,
    )
    .await?;
    if options.orphans_only {
        return Ok(report);
    }
    let max_age = options.max_age.or(config.storage.cache.max_age);
    let max_size = options.max_size.or(config.storage.cache.max_size);
    if max_age.is_none() && max_size.is_none() {
        return Ok(report);
    }
    let evict_report =
        evict_active_cache(&config.storage.cache_dir, &active, max_age, max_size, options.dry_run)
            .await?;
    report.files_evicted += evict_report.files_evicted;
    report.bytes_reclaimed += evict_report.bytes_reclaimed;
    if !options.dry_run && evict_report.files_evicted > 0 {
        prune_empty_dirs(&config.storage.cache_dir, &active).await?;
    }
    Ok(report)
}

/// Evict cache entries on startup: cleans orphaned namespaces and enforces configured caps.
pub async fn prune_orphaned_namespaces_startup(config: &Config) -> Result<CacheReport> {
    collect_cache(
        config,
        &CacheGcOptions {
            dry_run: false,
            max_size: config.storage.cache.max_size,
            max_age: config.storage.cache.max_age,
            orphans_only: false,
        },
    )
    .await
}

/// Run a periodic eviction pass using configured size and age limits.
pub async fn evict_configured_cache(config: &Config) -> Result<CacheReport> {
    prune_orphaned_namespaces_startup(config).await
}

/// Spawns a background task running cache eviction periodically if configured.
pub fn spawn_periodic_cache_eviction(config: &Config) {
    if config.storage.cache.max_size.is_none() && config.storage.cache.max_age.is_none() {
        return;
    }
    let background_config = config.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_mins(10));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(err) = evict_configured_cache(&background_config).await {
                tracing::warn!(error = %err, "periodic cache eviction failed");
            }
        }
    });
}

async fn prune_orphan_entry(
    entry: &tokio::fs::DirEntry,
    prefix: &str,
    active: &HashSet<String>,
    dry_run: bool,
    report: &mut CacheReport,
) -> Result<()> {
    let Ok(file_type) = entry.file_type().await else {
        return Ok(());
    };
    if !file_type.is_dir() || file_type.is_symlink() {
        return Ok(());
    }
    let ns = format!("{prefix}/{}", entry.file_name().to_string_lossy());
    if active.contains(&ns) {
        return Ok(());
    }
    let (count, bytes) = count_dir_contents(&entry.path()).await?;
    if !dry_run {
        fs::remove_dir_all(entry.path()).await?;
    }
    report.namespaces_removed += 1;
    report.files_evicted += count;
    report.bytes_reclaimed += bytes;
    Ok(())
}

async fn prune_orphan_prefix(
    cache_dir: &Path,
    prefix: &str,
    active: &HashSet<String>,
    dry_run: bool,
    report: &mut CacheReport,
) {
    let parent = cache_dir.join(prefix);
    if let Ok(meta) = fs::symlink_metadata(&parent).await
        && (meta.file_type().is_symlink() || !meta.is_dir())
    {
        return;
    }
    let Ok(mut entries) = fs::read_dir(&parent).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Err(err) = prune_orphan_entry(&entry, prefix, active, dry_run, report).await {
            tracing::warn!(error = %err, path = %entry.path().display(), "orphan cache namespace cleanup failed");
        }
    }
}

/// Prune any namespaces under `~upstreams/` or `~public/` not present in `active`.
async fn prune_orphaned_namespaces(
    cache_dir: &Path,
    active: &HashSet<String>,
    dry_run: bool,
    has_configured_secret: bool,
) -> Result<CacheReport> {
    let mut report = CacheReport::default();
    if has_configured_secret {
        prune_orphan_prefix(cache_dir, "~upstreams", active, dry_run, &mut report).await;
    }
    prune_orphan_prefix(cache_dir, "~public", active, dry_run, &mut report).await;
    Ok(report)
}

async fn process_count_entry(
    entry: &tokio::fs::DirEntry,
    stack: &mut Vec<PathBuf>,
    count: &mut usize,
    bytes: &mut u64,
) {
    let Ok(file_type) = entry.file_type().await else {
        return;
    };
    if file_type.is_dir() {
        stack.push(entry.path());
        return;
    }
    if !file_type.is_file() {
        return;
    }
    *count += 1;
    if let Ok(meta) = entry.metadata().await {
        *bytes += meta.len();
    }
}

/// Count regular files and total bytes in a directory tree.
async fn count_dir_contents(path: &Path) -> Result<(usize, u64)> {
    let mut count = 0;
    let mut bytes = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(mut entries) = fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            process_count_entry(&entry, &mut stack, &mut count, &mut bytes).await;
        }
    }
    Ok((count, bytes))
}

async fn evict_file_if_expired(
    file: CachedFile,
    max_age: Option<Duration>,
    dry_run: bool,
    report: &mut CacheReport,
    total_size: &mut u64,
    retained: &mut Vec<CachedFile>,
) {
    let is_tmp = is_abandoned_tmp(&file.path, file.age);
    let is_expired = max_age.is_some_and(|ttl| file.age >= ttl);
    if is_tmp || is_expired {
        if dry_run || fs::remove_file(&file.path).await.is_ok() {
            report.files_evicted += 1;
            report.bytes_reclaimed += file.size;
        } else {
            *total_size += file.size;
            retained.push(file);
        }
    } else {
        *total_size += file.size;
        retained.push(file);
    }
}

/// Evict expired files and enforce LRU size caps within active namespaces.
async fn evict_active_cache(
    cache_dir: &Path,
    active: &HashSet<String>,
    max_age: Option<Duration>,
    max_size: Option<u64>,
    dry_run: bool,
) -> Result<CacheReport> {
    let mut report = CacheReport::default();
    let mut retained = Vec::new();
    let mut total_size = 0u64;
    for ns in active {
        let ns_root = cache_dir.join(ns);
        let files = inventory_active_namespace(&ns_root).await?;
        for file in files {
            evict_file_if_expired(
                file,
                max_age,
                dry_run,
                &mut report,
                &mut total_size,
                &mut retained,
            )
            .await;
        }
    }
    if let Some(limit) = max_size {
        evict_lru(&mut retained, total_size, limit, dry_run, &mut report).await;
    }
    Ok(report)
}

/// Enforce LRU eviction down to `limit` bytes.
async fn evict_lru(
    retained: &mut [CachedFile],
    mut total_size: u64,
    limit: u64,
    dry_run: bool,
    report: &mut CacheReport,
) {
    if total_size <= limit {
        return;
    }
    retained.sort_by_key(|entry| entry.effective_time);
    for entry in retained {
        if total_size <= limit {
            break;
        }
        if dry_run || fs::remove_file(&entry.path).await.is_ok() {
            total_size = total_size.saturating_sub(entry.size);
            report.files_evicted += 1;
            report.bytes_reclaimed += entry.size;
        }
    }
}

async fn inspect_inventory_entry(
    entry: &tokio::fs::DirEntry,
    stack: &mut Vec<PathBuf>,
    files: &mut Vec<CachedFile>,
) {
    let Ok(file_type) = entry.file_type().await else {
        return;
    };
    if file_type.is_dir() {
        stack.push(entry.path());
        return;
    }
    if !file_type.is_file() {
        return;
    }
    let Ok(metadata) = entry.metadata().await else {
        return;
    };
    let (effective_time, age) = file_effective_time(&metadata);
    files.push(CachedFile { path: entry.path(), size: metadata.len(), effective_time, age });
}

/// Recursively collect all regular files in an active namespace directory.
async fn inventory_active_namespace(ns_root: &Path) -> Result<Vec<CachedFile>> {
    let mut files = Vec::new();
    let mut stack = vec![ns_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(mut entries) = fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            inspect_inventory_entry(&entry, &mut stack, &mut files).await;
        }
    }
    Ok(files)
}

async fn collect_subdirs(root: PathBuf) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(mut entries) = fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let Ok(ft) = entry.file_type().await else {
                continue;
            };
            if ft.is_dir() {
                dirs.push(entry.path());
                stack.push(entry.path());
            }
        }
    }
    dirs
}

/// Prune empty subdirectories within active namespaces bottom-up.
async fn prune_empty_dirs(cache_dir: &Path, active: &HashSet<String>) -> Result<()> {
    for ns in active {
        let ns_root = cache_dir.join(ns);
        let mut dirs = collect_subdirs(ns_root).await;
        dirs.sort_by(|a, b| {
            b.as_os_str()
                .len()
                .cmp(&a.as_os_str().len())
        });
        for dir in dirs {
            let _ = fs::remove_dir(dir).await;
        }
    }
    Ok(())
}

fn file_effective_time(metadata: &std::fs::Metadata) -> (SystemTime, Duration) {
    let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let atime = metadata.accessed().unwrap_or(mtime);
    let effective = mtime.max(atime);
    let age = SystemTime::now().duration_since(mtime).unwrap_or(Duration::ZERO);
    (effective, age)
}

fn is_abandoned_tmp(path: &Path, age: Duration) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.starts_with(".tmp") && age > Duration::from_hours(1)
}

#[cfg(test)]
mod tests;
