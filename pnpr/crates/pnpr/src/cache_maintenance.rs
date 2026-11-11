//! Disposable cache reclamation, orphan pruning, and LRU eviction.
//!
//! Reclaims orphaned upstream cache namespaces, sweeps abandoned publish
//! staging files, evicts stale cached entries exceeding TTL, and enforces
//! whole-cache size caps across upstream mirrors, tar variants, and the
//! resolver's store and caches.

mod inventory;
mod orphans;
mod sqlite;

use crate::{Config, Result};
use inventory::{
    cas_digest_of_store_file, inventory_cache_dir, is_cache_tmp_file, prune_empty_cache_dirs,
};
use orphans::prune_orphaned_namespaces;
use sqlite::{
    expire_verdict_rows, prune_store_index_digests, resolver_sqlite_bytes, trim_verdict_db_to_fit,
};
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

/// Whether a collection pass deletes what it selects.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CacheGcEffect {
    /// Delete the entries the pass selects.
    #[default]
    Apply,
    /// Leave every entry in place, and report what [`Self::Apply`] would
    /// have removed.
    DryRun,
}

/// How much of the cache a collection pass covers.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CacheGcScope {
    /// Prune orphaned namespaces, sweep staging files, and evict across the
    /// whole cache directory.
    #[default]
    Full,
    /// Prune orphaned upstream namespaces only, leaving active entries in place.
    OrphansOnly,
}

/// Options controlling cache reclamation and eviction.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CacheGcOptions {
    /// Whether this pass deletes what it finds.
    pub effect: CacheGcEffect,
    /// Maximum total cache size to retain. Oldest files are evicted first.
    pub max_size: Option<u64>,
    /// Maximum age of cached entries. Files older than this are deleted.
    pub max_age: Option<Duration>,
    /// How much of the cache this pass covers.
    pub scope: CacheGcScope,
}

struct CachedFile {
    path: PathBuf,
    size: u64,
    effective_time: SystemTime,
    age: Duration,
}

struct EvictionState {
    effect: CacheGcEffect,
    report: CacheReport,
    retained: Vec<CachedFile>,
    retained_bytes: u64,
    non_evictable_bytes: u64,
    removed_cas_digests: HashSet<String>,
}

/// The set of active upstream and tar-variant cache namespaces.
#[must_use]
pub fn active_cache_namespaces(config: &Config) -> HashSet<String> {
    config.routing.upstreams
        .keys()
        .map(|name| crate::server::compute_upstream_cache_namespace(config, name))
        .chain([crate::server::TAR_VARIANT_CACHE_NAMESPACE.to_string()])
        .collect()
}

/// How long an archive staging file may stay untouched before it is swept:
/// the HTTP request timeout when one is set, or
/// [`pnpr_storage::INCOMPLETE_MAX_AGE`] when request timeouts are disabled.
#[must_use]
pub(crate) fn archive_staging_max_age(config: &Config) -> Duration {
    config.http.request_timeout.unwrap_or(pnpr_storage::INCOMPLETE_MAX_AGE)
}

async fn sweep_staging_cache(
    cache_dir: &Path,
    max_age: Duration,
    effect: CacheGcEffect,
) -> pnpr_storage::TempSweep {
    let base = pnpr_storage::cache_staging_tmp_base(cache_dir);
    let result = match effect {
        CacheGcEffect::Apply => pnpr_storage::sweep_stale_temps(&base, max_age).await,
        CacheGcEffect::DryRun => pnpr_storage::count_stale_temps(&base, max_age).await,
    };
    match result {
        Ok(sweep) => sweep,
        Err(err) => {
            tracing::warn!(error = %err, "sweeping publish staging files failed");
            pnpr_storage::TempSweep::default()
        }
    }
}

/// Run a complete cache collection pass according to the given options.
pub async fn collect_cache(config: &Config, options: &CacheGcOptions) -> Result<CacheReport> {
    let active = active_cache_namespaces(config);
    let mut report =
        prune_orphaned_namespaces(&config.storage.cache_dir, &active, options.effect).await?;
    match options.scope {
        CacheGcScope::OrphansOnly => return Ok(report),
        CacheGcScope::Full => {}
    }
    let staging = sweep_staging_cache(
        &config.storage.cache_dir,
        archive_staging_max_age(config),
        options.effect,
    )
    .await;
    report.files_evicted += staging.files;
    report.bytes_reclaimed += staging.bytes;
    let max_age = options.max_age.or(config.storage.cache.max_age);
    let max_size = options.max_size.or(config.storage.cache.max_size);
    if max_age.is_some() || max_size.is_some() {
        let ev =
            evict_and_prune_cache(&config.storage.cache_dir, max_age, max_size, options.effect)
                .await?;
        report.files_evicted += ev.files_evicted;
        report.bytes_reclaimed += ev.bytes_reclaimed;
    }
    Ok(report)
}

async fn evict_and_prune_cache(
    cache_dir: &Path,
    max_age: Option<Duration>,
    max_size: Option<u64>,
    effect: CacheGcEffect,
) -> Result<CacheReport> {
    let ev = evict_cache(cache_dir, max_age, max_size, effect).await?;
    if matches!(effect, CacheGcEffect::Apply) && ev.files_evicted > 0 {
        prune_empty_cache_dirs(cache_dir).await?;
    }
    Ok(ev)
}

/// Evict cache entries on startup: cleans orphaned namespaces and enforces configured caps.
pub async fn prune_orphaned_namespaces_startup(config: &Config) -> Result<CacheReport> {
    let opts = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: config.storage.cache.max_size,
        max_age: config.storage.cache.max_age,
        scope: CacheGcScope::Full,
    };
    collect_cache(config, &opts).await
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

async fn counts_as_removed(effect: CacheGcEffect, path: &Path) -> bool {
    match effect {
        CacheGcEffect::Apply => fs::remove_file(path).await.is_ok(),
        CacheGcEffect::DryRun => true,
    }
}

async fn evict_file_if_expired(
    cache_dir: &Path,
    file: CachedFile,
    max_age: Option<Duration>,
    state: &mut EvictionState,
) {
    if is_cache_tmp_file(cache_dir, &file.path) {
        if is_abandoned_tmp_age(file.age) && counts_as_removed(state.effect, &file.path).await {
            state.report.files_evicted += 1;
            state.report.bytes_reclaimed += file.size;
        } else {
            state.non_evictable_bytes += file.size;
        }
        return;
    }
    if max_age.is_some_and(|ttl| file.age >= ttl)
        && counts_as_removed(state.effect, &file.path).await
    {
        record_evicted_file(cache_dir, &file.path, file.size, state);
        return;
    }
    state.retained_bytes += file.size;
    state.retained.push(file);
}

fn record_evicted_file(cache_dir: &Path, path: &Path, size: u64, state: &mut EvictionState) {
    state.report.files_evicted += 1;
    state.report.bytes_reclaimed += size;
    if let Some(digest) = cas_digest_of_store_file(cache_dir, path) {
        state.removed_cas_digests.insert(digest);
    }
}

async fn evict_cache(
    cache_dir: &Path,
    max_age: Option<Duration>,
    max_size: Option<u64>,
    effect: CacheGcEffect,
) -> Result<CacheReport> {
    let inv = inventory_cache_dir(cache_dir).await?;
    let mut state = EvictionState {
        effect,
        report: CacheReport::default(),
        retained: Vec::new(),
        retained_bytes: 0,
        non_evictable_bytes: inv.non_evictable_bytes,
        removed_cas_digests: HashSet::new(),
    };
    if let Some(ttl) = max_age {
        state.report.bytes_reclaimed += expire_verdict_rows(cache_dir, ttl, effect).await;
    }
    for file in inv.files {
        evict_file_if_expired(cache_dir, file, max_age, &mut state).await;
    }
    flush_removed_cas_digests(cache_dir, &mut state).await;
    if let Some(limit) = max_size {
        enforce_max_size(cache_dir, limit, &mut state).await;
    }
    Ok(state.report)
}

async fn flush_removed_cas_digests(cache_dir: &Path, state: &mut EvictionState) {
    let digests = std::mem::take(&mut state.removed_cas_digests);
    if matches!(state.effect, CacheGcEffect::Apply) && !digests.is_empty() {
        state.report.bytes_reclaimed += prune_store_index_digests(cache_dir, digests).await;
    }
}

async fn enforce_max_size(cache_dir: &Path, limit: u64, state: &mut EvictionState) {
    let sqlite_bytes = resolver_sqlite_bytes(cache_dir).await;
    let mut total_size = state.non_evictable_bytes + sqlite_bytes + state.retained_bytes;
    if total_size <= limit {
        return;
    }
    state.retained.sort_by_key(|entry| entry.effective_time);
    let candidates = std::mem::take(&mut state.retained);
    for entry in &candidates {
        if total_size <= limit {
            break;
        }
        if evict_lru_entry(cache_dir, entry, state).await {
            total_size = total_size.saturating_sub(entry.size);
        }
    }
    flush_removed_cas_digests(cache_dir, state).await;
    let updated_sqlite = resolver_sqlite_bytes(cache_dir).await;
    let remaining_total = total_size.saturating_sub(sqlite_bytes).saturating_add(updated_sqlite);
    if remaining_total > limit {
        state.report.bytes_reclaimed +=
            trim_verdict_db_to_fit(cache_dir, remaining_total - limit, state.effect).await;
    }
}

async fn evict_lru_entry(cache_dir: &Path, entry: &CachedFile, state: &mut EvictionState) -> bool {
    match state.effect {
        CacheGcEffect::Apply => {
            if let Ok(meta) = fs::metadata(&entry.path).await {
                let (current_time, _) = file_effective_time(&meta);
                if current_time > entry.effective_time {
                    return false;
                }
            }
            if fs::remove_file(&entry.path).await.is_err() {
                return false;
            }
        }
        CacheGcEffect::DryRun => {}
    }
    record_evicted_file(cache_dir, &entry.path, entry.size, state);
    true
}

fn file_effective_time(metadata: &std::fs::Metadata) -> (SystemTime, Duration) {
    let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let atime = metadata.accessed().unwrap_or(mtime);
    let effective = mtime.max(atime);
    let age = SystemTime::now().duration_since(mtime).unwrap_or(Duration::ZERO);
    (effective, age)
}

fn is_abandoned_tmp_age(age: Duration) -> bool {
    age > Duration::from_hours(1)
}

#[cfg(test)]
mod tests;
