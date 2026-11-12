//! Disposable cache reclamation, orphan pruning, and LRU eviction.
//!
//! Reclaims orphaned upstream cache namespaces, sweeps abandoned publish
//! staging files, evicts stale cached entries exceeding TTL, and enforces
//! whole-cache size caps across upstream mirrors, tar variants, and the
//! resolver's store and caches.

mod eviction;
mod inventory;
mod orphans;
mod sqlite;
mod store_index;

use crate::{Config, Result};
use eviction::{DryRunAdjustments, evict_and_prune_cache};
use orphans::{PrivateScopePolicy, prune_orphaned_namespaces, prune_private_resolver_scopes_only};
use pnpm_fs::ChildFileStat;
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
    /// Prune orphaned upstream and resolver namespaces only, leaving active entries in place.
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

/// Guard that aborts the periodic cache-eviction task when dropped.
pub struct PeriodicCacheEviction(Option<tokio::task::JoinHandle<()>>);

impl Drop for PeriodicCacheEviction {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

struct CachedFile {
    path: PathBuf,
    size: u64,
    effective_time: SystemTime,
    age: Duration,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkerDirPolicy {
    CreateIfMissing,
    ExistingOnly,
}

async fn should_prune_private_resolver_scopes(
    cache_dir: &Path,
    config: &Config,
) -> PrivateScopePolicy {
    let fingerprint = pnpr_config::cache_secret_fingerprint(&config.cache_secret);
    let root = cache_dir.to_path_buf();
    let matches = tokio::task::spawn_blocking(move || {
        let dir = pnpm_fs::OpenDir::open(&root).ok()?;
        let marker = std::ffi::OsStr::new(pnpr_config::CACHE_SECRET_MARKER);
        if dir.has_symlink_child(marker).ok()? {
            return None;
        }
        let bytes = dir
            .read_child_file_bounded(
                marker,
                crate::config_validation::MAX_CACHE_SECRET_MARKER_BYTES,
            )
            .ok()??;
        Some(std::str::from_utf8(&bytes).ok()?.trim() == fingerprint)
    })
    .await
    .ok()
    .flatten()
    .unwrap_or(false);
    if matches { PrivateScopePolicy::Keep } else { PrivateScopePolicy::Prune }
}

async fn sync_configured_cache_marker(
    config: &Config,
    effect: CacheGcEffect,
    dir_policy: MarkerDirPolicy,
) -> Result<()> {
    if !config.has_configured_secret {
        return Ok(());
    }
    match effect {
        CacheGcEffect::DryRun => return Ok(()),
        CacheGcEffect::Apply => {}
    }
    let cache_dir = &config.storage.cache_dir;
    match dir_policy {
        MarkerDirPolicy::CreateIfMissing => fs::create_dir_all(cache_dir).await?,
        MarkerDirPolicy::ExistingOnly => {
            if !fs::try_exists(cache_dir).await.unwrap_or(false) {
                return Ok(());
            }
        }
    }
    let root = cache_dir.clone();
    let fingerprint = pnpr_config::cache_secret_fingerprint(&config.cache_secret);
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let dir = pnpm_fs::OpenDir::open(&root)?;
        dir.write_child_file(
            std::ffi::OsStr::new(pnpr_config::CACHE_SECRET_MARKER),
            fingerprint.as_bytes(),
        )
    })
    .await
    .map_err(|err| std::io::Error::other(err.to_string()))??;
    Ok(())
}

pub(crate) async fn sync_cache_secret_on_startup(config: &Config) -> Result<()> {
    crate::config_validation::validate_cache_paths(config)?;
    let cache_dir = &config.storage.cache_dir;
    let clean = match should_prune_private_resolver_scopes(cache_dir, config).await {
        PrivateScopePolicy::Prune => {
            let active = active_cache_namespaces(config);
            prune_private_resolver_scopes_only(cache_dir, &active, CacheGcEffect::Apply).await?
        }
        PrivateScopePolicy::Keep => true,
    };
    if clean {
        sync_configured_cache_marker(
            config,
            CacheGcEffect::Apply,
            MarkerDirPolicy::CreateIfMissing,
        )
        .await?;
    }
    Ok(())
}

/// Run a complete cache collection pass according to the given options.
pub async fn collect_cache(config: &Config, options: &CacheGcOptions) -> Result<CacheReport> {
    collect_cache_inner(config, options, MarkerDirPolicy::ExistingOnly).await
}

async fn collect_cache_inner(
    config: &Config,
    options: &CacheGcOptions,
    marker_dir_policy: MarkerDirPolicy,
) -> Result<CacheReport> {
    crate::config_validation::validate_cache_paths(config)?;
    let cache_dir = &config.storage.cache_dir;
    let active = active_cache_namespaces(config);
    let private_scopes = should_prune_private_resolver_scopes(cache_dir, config).await;
    let outcome =
        prune_orphaned_namespaces(cache_dir, &active, private_scopes, options.effect).await?;
    if outcome.private_scopes_clean {
        sync_configured_cache_marker(config, options.effect, marker_dir_policy).await?;
    }
    let mut report = outcome.report;
    match options.scope {
        CacheGcScope::OrphansOnly => return Ok(report),
        CacheGcScope::Full => {}
    }
    let staging =
        sweep_staging_cache(cache_dir, archive_staging_max_age(config), options.effect).await;
    report.files_evicted += staging.files;
    report.bytes_reclaimed += staging.bytes;
    let max_age = options.max_age.or(config.storage.cache.max_age);
    let max_size = options.max_size.or(config.storage.cache.max_size);
    if max_age.is_some() || max_size.is_some() {
        let adjustments = DryRunAdjustments {
            pruned_dirs: outcome.pruned_dirs,
            swept_staging_bytes: staging.bytes,
        };
        let ev = evict_and_prune_cache(cache_dir, max_age, max_size, options.effect, &adjustments)
            .await?;
        report.files_evicted += ev.files_evicted;
        report.bytes_reclaimed += ev.bytes_reclaimed;
    }
    Ok(report)
}

/// Evict cache entries on startup: cleans orphaned namespaces and enforces configured caps.
pub async fn prune_orphaned_namespaces_startup(config: &Config) -> Result<CacheReport> {
    let opts = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: config.storage.cache.max_size,
        max_age: config.storage.cache.max_age,
        scope: CacheGcScope::Full,
    };
    collect_cache_inner(config, &opts, MarkerDirPolicy::CreateIfMissing).await
}

/// Run a periodic eviction pass using configured size and age limits.
pub async fn evict_configured_cache(config: &Config) -> Result<CacheReport> {
    collect_cache(
        config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: config.storage.cache.max_size,
            max_age: config.storage.cache.max_age,
            scope: CacheGcScope::Full,
        },
    )
    .await
}

/// Spawns a background task running cache eviction periodically if configured.
#[must_use]
pub fn spawn_periodic_cache_eviction(config: &Config) -> PeriodicCacheEviction {
    if (!config.features.registry.enabled && !config.features.resolver.enabled)
        || (config.storage.cache.max_size.is_none()
            && config.storage.cache.max_age.is_none())
    {
        return PeriodicCacheEviction(None);
    }
    let background_config = config.clone();
    let handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_mins(10));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(err) = evict_configured_cache(&background_config).await {
                tracing::warn!(error = %err, "periodic cache eviction failed");
            }
        }
    });
    PeriodicCacheEviction(Some(handle))
}

fn stat_effective_time(stat: ChildFileStat) -> (SystemTime, Duration) {
    let effective = stat.modified.max(stat.accessed);
    let age = SystemTime::now().duration_since(stat.modified).unwrap_or(Duration::ZERO);
    (effective, age)
}

fn is_abandoned_tmp_age(age: Duration) -> bool {
    age > Duration::from_hours(1)
}

#[cfg(test)]
mod tests;
