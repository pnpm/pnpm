use super::{CacheGcEffect, CacheReport, HashSet, Path, PathBuf, Result, fs};

async fn prune_orphan_entry(
    entry: &tokio::fs::DirEntry,
    prefix: &str,
    active: &HashSet<String>,
    effect: CacheGcEffect,
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
    match effect {
        CacheGcEffect::Apply => fs::remove_dir_all(entry.path()).await?,
        CacheGcEffect::DryRun => {}
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
    effect: CacheGcEffect,
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
        if let Err(err) = prune_orphan_entry(&entry, prefix, active, effect, report).await {
            tracing::warn!(error = %err, path = %entry.path().display(), "orphan cache namespace cleanup failed");
        }
    }
}

/// Prune any namespaces under `~upstreams/` or `~public/` not present in `active`.
pub(super) async fn prune_orphaned_namespaces(
    cache_dir: &Path,
    active: &HashSet<String>,
    effect: CacheGcEffect,
    has_configured_secret: bool,
) -> Result<CacheReport> {
    let mut report = CacheReport::default();
    if has_configured_secret {
        prune_orphan_prefix(cache_dir, "~upstreams", active, effect, &mut report).await;
    }
    prune_orphan_prefix(cache_dir, "~public", active, effect, &mut report).await;
    Ok(report)
}

async fn process_count_entry(
    entry: &tokio::fs::DirEntry,
    stack: &mut Vec<PathBuf>,
    count: &mut usize,
    bytes: &mut u64,
) {
    let Ok(ft) = entry.file_type().await else {
        return;
    };
    if ft.is_dir() {
        stack.push(entry.path());
    } else if ft.is_file() {
        *count += 1;
        if let Ok(meta) = entry.metadata().await {
            *bytes += meta.len();
        }
    }
}

/// Count regular files and total bytes in a directory tree.
pub(super) async fn count_dir_contents(path: &Path) -> Result<(usize, u64)> {
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
