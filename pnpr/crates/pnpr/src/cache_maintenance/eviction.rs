use super::{
    CacheGcEffect, CacheReport, CachedFile, Duration, HashSet, Path, PathBuf, Result, SystemTime,
    inventory::{
        CachedFileRemoval, cas_digest_of_store_file, inventory_cache_dir, is_cache_tmp_file,
        prune_empty_cache_dirs, remove_cached_files,
    },
    is_abandoned_tmp_age,
    sqlite::{
        VerdictExpiry, VerdictTrimParams, expire_verdict_rows, resolver_sqlite_bytes,
        trim_verdict_db_to_fit,
    },
    store_index::prune_store_index_digests,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum SkippedCasPolicy {
    RetainForSizeCheck,
    CountTowardsSize,
}

#[derive(Clone, Copy)]
enum RemovalFallback {
    RetainEvictable,
    CountNonEvictable,
    CountTowardsSize,
}

struct PendingFile {
    file: CachedFile,
    max_effective_time: Option<SystemTime>,
    fallback: RemovalFallback,
}

pub(super) struct DryRunAdjustments {
    pub(super) pruned_dirs: HashSet<PathBuf>,
    pub(super) swept_staging_bytes: u64,
}

struct EvictionState {
    effect: CacheGcEffect,
    report: CacheReport,
    retained: Vec<CachedFile>,
    retained_bytes: u64,
    non_evictable_bytes: u64,
    pending_files: Vec<PendingFile>,
    pending_cas: Vec<(CachedFile, String)>,
    verdict_expiry: Option<VerdictExpiry>,
}

pub(super) async fn evict_and_prune_cache(
    cache_dir: &Path,
    max_age: Option<Duration>,
    max_size: Option<u64>,
    effect: CacheGcEffect,
    adjustments: &DryRunAdjustments,
) -> Result<CacheReport> {
    let ev = evict_cache(cache_dir, max_age, max_size, effect, adjustments).await?;
    match effect {
        CacheGcEffect::Apply if ev.files_evicted > 0 => prune_empty_cache_dirs(cache_dir).await?,
        CacheGcEffect::Apply | CacheGcEffect::DryRun => {}
    }
    Ok(ev)
}

fn classify_file_for_ttl(
    cache_dir: &Path,
    file: CachedFile,
    max_age: Option<Duration>,
    state: &mut EvictionState,
) {
    if is_cache_tmp_file(cache_dir, &file.path) {
        queue_tmp_file_if_abandoned(file, state);
        return;
    }
    if max_age.is_some_and(|ttl| file.age >= ttl) {
        if let Some(digest) = cas_digest_of_store_file(cache_dir, &file.path) {
            state.pending_cas.push((file, digest));
        } else {
            let max_effective_time = Some(file.effective_time);
            state.pending_files.push(PendingFile {
                file,
                max_effective_time,
                fallback: RemovalFallback::RetainEvictable,
            });
        }
        return;
    }
    state.retained_bytes += file.size;
    state.retained.push(file);
}

fn queue_tmp_file_if_abandoned(file: CachedFile, state: &mut EvictionState) {
    if !pnpr_storage::is_active_blob_temp(&file.path) && is_abandoned_tmp_age(file.age) {
        state.pending_files.push(PendingFile {
            file,
            max_effective_time: None,
            fallback: RemovalFallback::CountNonEvictable,
        });
    } else {
        state.non_evictable_bytes += file.size;
    }
}

async fn evict_cache(
    cache_dir: &Path,
    max_age: Option<Duration>,
    max_size: Option<u64>,
    effect: CacheGcEffect,
    adjustments: &DryRunAdjustments,
) -> Result<CacheReport> {
    let inv = inventory_cache_dir(cache_dir, &adjustments.pruned_dirs).await?;
    let dry_swept = match effect {
        CacheGcEffect::DryRun => adjustments.swept_staging_bytes,
        CacheGcEffect::Apply => 0,
    };
    let mut state = EvictionState {
        effect,
        report: CacheReport::default(),
        retained: Vec::new(),
        retained_bytes: 0,
        non_evictable_bytes: inv.non_evictable_bytes.saturating_sub(dry_swept),
        pending_files: Vec::new(),
        pending_cas: Vec::new(),
        verdict_expiry: None,
    };
    if let Some(ttl) = max_age {
        let expiry = expire_verdict_rows(cache_dir, ttl, effect).await;
        state.report.bytes_reclaimed += expiry.reclaimed_bytes;
        state.verdict_expiry = Some(expiry);
    }
    for file in inv.files {
        classify_file_for_ttl(cache_dir, file, max_age, &mut state);
    }
    flush_pending_files(cache_dir, &mut state).await;
    flush_pending_cas(cache_dir, &mut state, SkippedCasPolicy::RetainForSizeCheck).await;
    if let Some(limit) = max_size {
        enforce_max_size(cache_dir, limit, &mut state).await;
    }
    Ok(state.report)
}

async fn flush_pending_files(cache_dir: &Path, state: &mut EvictionState) -> u64 {
    let pending = std::mem::take(&mut state.pending_files);
    if pending.is_empty() {
        return 0;
    }
    match state.effect {
        CacheGcEffect::DryRun => {
            state.report.files_evicted += pending.len();
            state.report.bytes_reclaimed += pending
                .iter()
                .map(|item| item.file.size)
                .sum::<u64>();
            0
        }
        CacheGcEffect::Apply => apply_pending_file_removals(cache_dir, pending, state).await,
    }
}

async fn apply_pending_file_removals(
    cache_dir: &Path,
    pending: Vec<PendingFile>,
    state: &mut EvictionState,
) -> u64 {
    let targets: Vec<CachedFileRemoval> = pending
        .iter()
        .map(|item| CachedFileRemoval {
            path: item.file.path.clone(),
            max_effective_time: item.max_effective_time,
        })
        .collect();
    let outcomes = remove_cached_files(cache_dir, targets).await;
    let mut skipped_bytes = 0u64;
    for (item, removed) in pending.into_iter().zip(outcomes) {
        if removed {
            state.report.files_evicted += 1;
            state.report.bytes_reclaimed += item.file.size;
        } else {
            skipped_bytes =
                skipped_bytes.saturating_add(record_skipped_file(item.file, item.fallback, state));
        }
    }
    skipped_bytes
}

fn record_skipped_file(
    file: CachedFile,
    fallback: RemovalFallback,
    state: &mut EvictionState,
) -> u64 {
    match fallback {
        RemovalFallback::RetainEvictable => {
            state.retained_bytes += file.size;
            state.retained.push(file);
            0
        }
        RemovalFallback::CountNonEvictable => {
            state.non_evictable_bytes += file.size;
            0
        }
        RemovalFallback::CountTowardsSize => file.size,
    }
}

async fn flush_pending_cas(
    cache_dir: &Path,
    state: &mut EvictionState,
    policy: SkippedCasPolicy,
) -> u64 {
    let pending = std::mem::take(&mut state.pending_cas);
    if pending.is_empty() {
        return 0;
    }
    match state.effect {
        CacheGcEffect::DryRun => {
            state.report.files_evicted += pending.len();
            state.report.bytes_reclaimed += pending
                .iter()
                .map(|(file, _)| file.size)
                .sum::<u64>();
            0
        }
        CacheGcEffect::Apply => apply_pending_cas_removals(cache_dir, pending, state, policy).await,
    }
}

async fn apply_pending_cas_removals(
    cache_dir: &Path,
    pending: Vec<(CachedFile, String)>,
    state: &mut EvictionState,
    policy: SkippedCasPolicy,
) -> u64 {
    let digests: HashSet<String> = pending
        .iter()
        .map(|(_, digest)| digest.clone())
        .collect();
    match prune_store_index_digests(cache_dir, digests).await {
        Ok(reclaimed) => state.report.bytes_reclaimed += reclaimed,
        Err(err) => {
            tracing::warn!(error = %err, "pruning resolver store index failed; retaining CAS files");
            return retain_all_pending_cas(pending, state, policy);
        }
    }
    let targets: Vec<CachedFileRemoval> = pending
        .iter()
        .map(|(file, _)| CachedFileRemoval {
            path: file.path.clone(),
            max_effective_time: Some(file.effective_time),
        })
        .collect();
    let outcomes = remove_cached_files(cache_dir, targets).await;
    let mut skipped_bytes = 0u64;
    for ((file, _), removed) in pending.into_iter().zip(outcomes) {
        if removed {
            state.report.files_evicted += 1;
            state.report.bytes_reclaimed += file.size;
        } else {
            skipped_bytes += retain_cas_entry(file, state, policy);
        }
    }
    skipped_bytes
}

fn retain_all_pending_cas(
    pending: Vec<(CachedFile, String)>,
    state: &mut EvictionState,
    policy: SkippedCasPolicy,
) -> u64 {
    pending
        .into_iter()
        .map(|(file, _)| retain_cas_entry(file, state, policy))
        .sum()
}

fn retain_cas_entry(file: CachedFile, state: &mut EvictionState, policy: SkippedCasPolicy) -> u64 {
    let size = file.size;
    match policy {
        SkippedCasPolicy::RetainForSizeCheck => {
            state.retained_bytes += size;
            state.retained.push(file);
        }
        SkippedCasPolicy::CountTowardsSize => {}
    }
    size
}

async fn flush_size_removals(cache_dir: &Path, state: &mut EvictionState) -> u64 {
    let skipped_files = flush_pending_files(cache_dir, state).await;
    let skipped_cas = flush_pending_cas(cache_dir, state, SkippedCasPolicy::CountTowardsSize).await;
    skipped_files.saturating_add(skipped_cas)
}

async fn enforce_max_size(cache_dir: &Path, limit: u64, state: &mut EvictionState) {
    let dry_expired = match (state.effect, &state.verdict_expiry) {
        (CacheGcEffect::DryRun, Some(exp)) => exp.reclaimed_bytes,
        _ => 0,
    };
    let sqlite_bytes = resolver_sqlite_bytes(cache_dir).await.saturating_sub(dry_expired);
    let mut total_size = state.non_evictable_bytes + sqlite_bytes + state.retained_bytes;
    if total_size <= limit {
        return;
    }
    state.retained.sort_by_key(|entry| entry.effective_time);
    let candidates = std::mem::take(&mut state.retained);
    for entry in candidates {
        if total_size <= limit {
            total_size = total_size.saturating_add(flush_size_removals(cache_dir, state).await);
            if total_size <= limit {
                break;
            }
        }
        total_size = queue_lru_entry(cache_dir, entry, total_size, state);
    }
    total_size = total_size.saturating_add(flush_size_removals(cache_dir, state).await);
    let updated_sqlite = resolver_sqlite_bytes(cache_dir).await.saturating_sub(dry_expired);
    let remaining_total = total_size.saturating_sub(sqlite_bytes).saturating_add(updated_sqlite);
    if remaining_total > limit {
        let params = VerdictTrimParams {
            excess_bytes: remaining_total - limit,
            min_verified_at_ms: state.verdict_expiry.as_ref().map(|exp| exp.cutoff_ms),
            deferred_free_bytes: state.verdict_expiry
                .as_ref()
                .map(|exp| exp.deferred_free_bytes)
                .unwrap_or(0),
            effect: state.effect,
        };
        state.report.bytes_reclaimed += trim_verdict_db_to_fit(cache_dir, params).await;
    }
}

fn queue_lru_entry(
    cache_dir: &Path,
    entry: CachedFile,
    total_size: u64,
    state: &mut EvictionState,
) -> u64 {
    let next_total = total_size.saturating_sub(entry.size);
    if let Some(digest) = cas_digest_of_store_file(cache_dir, &entry.path) {
        state.pending_cas.push((entry, digest));
    } else {
        let max_effective_time = Some(entry.effective_time);
        state.pending_files.push(PendingFile {
            file: entry,
            max_effective_time,
            fallback: RemovalFallback::CountTowardsSize,
        });
    }
    next_total
}
