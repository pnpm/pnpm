use super::{
    CacheGcEffect, CacheReport, CachedFile, Duration, HashSet, Path, PathBuf, Result, SystemTime,
    inventory::{
        cas_digest_of_store_file, inventory_cache_dir, is_cache_tmp_file, prune_empty_cache_dirs,
        remove_cached_file,
    },
    is_abandoned_tmp_age,
    sqlite::{
        VerdictExpiry, expire_verdict_rows, prune_store_index_digests, resolver_sqlite_bytes,
        trim_verdict_db_to_fit,
    },
};

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
    if matches!(effect, CacheGcEffect::Apply) && ev.files_evicted > 0 {
        prune_empty_cache_dirs(cache_dir).await?;
    }
    Ok(ev)
}

async fn remove_or_dry_run(
    cache_dir: &Path,
    path: &Path,
    max_effective_time: Option<SystemTime>,
    effect: CacheGcEffect,
) -> bool {
    match effect {
        CacheGcEffect::Apply => remove_cached_file(cache_dir, path, max_effective_time).await,
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
        evict_tmp_file_if_abandoned(cache_dir, &file, state).await;
        return;
    }
    if max_age.is_some_and(|ttl| file.age >= ttl) {
        if let Some(digest) = cas_digest_of_store_file(cache_dir, &file.path) {
            state.pending_cas.push((file, digest));
            return;
        }
        if remove_or_dry_run(cache_dir, &file.path, Some(file.effective_time), state.effect).await {
            state.report.files_evicted += 1;
            state.report.bytes_reclaimed += file.size;
            return;
        }
    }
    state.retained_bytes += file.size;
    state.retained.push(file);
}

async fn evict_tmp_file_if_abandoned(
    cache_dir: &Path,
    file: &CachedFile,
    state: &mut EvictionState,
) {
    if !pnpr_storage::is_active_blob_temp(&file.path)
        && is_abandoned_tmp_age(file.age)
        && remove_or_dry_run(cache_dir, &file.path, None, state.effect).await
    {
        state.report.files_evicted += 1;
        state.report.bytes_reclaimed += file.size;
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
        pending_cas: Vec::new(),
        verdict_expiry: None,
    };
    if let Some(ttl) = max_age {
        let expiry = expire_verdict_rows(cache_dir, ttl, effect).await;
        state.report.bytes_reclaimed += expiry.reclaimed_bytes;
        state.verdict_expiry = Some(expiry);
    }
    for file in inv.files {
        evict_file_if_expired(cache_dir, file, max_age, &mut state).await;
    }
    flush_pending_cas(cache_dir, &mut state, true).await;
    if let Some(limit) = max_size {
        enforce_max_size(cache_dir, limit, &mut state).await;
    }
    Ok(state.report)
}

async fn flush_pending_cas(
    cache_dir: &Path,
    state: &mut EvictionState,
    retain_skipped: bool,
) -> u64 {
    let pending = std::mem::take(&mut state.pending_cas);
    if pending.is_empty() {
        return 0;
    }
    if matches!(state.effect, CacheGcEffect::Apply) {
        let digests: HashSet<String> = pending
            .iter()
            .map(|(_, digest)| digest.clone())
            .collect();
        state.report.bytes_reclaimed += prune_store_index_digests(cache_dir, digests).await;
    }
    let mut skipped_bytes = 0u64;
    for (file, _) in pending {
        if remove_or_dry_run(cache_dir, &file.path, Some(file.effective_time), state.effect).await {
            state.report.files_evicted += 1;
            state.report.bytes_reclaimed += file.size;
        } else {
            skipped_bytes += file.size;
            if retain_skipped {
                state.retained_bytes += file.size;
                state.retained.push(file);
            }
        }
    }
    skipped_bytes
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
            break;
        }
        total_size = select_lru_entry(cache_dir, entry, total_size, state).await;
    }
    total_size += flush_pending_cas(cache_dir, state, false).await;
    let updated_sqlite = resolver_sqlite_bytes(cache_dir).await.saturating_sub(dry_expired);
    let remaining_total = total_size.saturating_sub(sqlite_bytes).saturating_add(updated_sqlite);
    if remaining_total > limit {
        let floor = state.verdict_expiry.as_ref().map(|exp| exp.cutoff_ms);
        state.report.bytes_reclaimed +=
            trim_verdict_db_to_fit(cache_dir, remaining_total - limit, floor, state.effect).await;
    }
}

async fn select_lru_entry(
    cache_dir: &Path,
    entry: CachedFile,
    total_size: u64,
    state: &mut EvictionState,
) -> u64 {
    if let Some(digest) = cas_digest_of_store_file(cache_dir, &entry.path) {
        let next_total = total_size.saturating_sub(entry.size);
        state.pending_cas.push((entry, digest));
        return next_total;
    }
    if remove_or_dry_run(cache_dir, &entry.path, Some(entry.effective_time), state.effect).await {
        state.report.files_evicted += 1;
        state.report.bytes_reclaimed += entry.size;
        return total_size.saturating_sub(entry.size);
    }
    total_size
}
