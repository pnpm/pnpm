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
    remove_orphan_dir(&entry.path(), effect, report).await
}

async fn remove_orphan_dir(
    path: &Path,
    effect: CacheGcEffect,
    report: &mut CacheReport,
) -> Result<()> {
    let (count, bytes) = count_dir_contents(path).await?;
    match effect {
        CacheGcEffect::Apply => fs::remove_dir_all(path).await?,
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

/// Prune orphaned `~upstreams/` and `~public/` namespaces, and purge private
/// resolver cache scopes when the cache secret rotated.
pub(super) async fn prune_orphaned_namespaces(
    cache_dir: &Path,
    active: &HashSet<String>,
    prune_private_resolver: bool,
    effect: CacheGcEffect,
) -> Result<CacheReport> {
    let mut report = CacheReport::default();
    prune_orphan_prefix(cache_dir, "~upstreams", active, effect, &mut report).await;
    prune_orphan_prefix(cache_dir, "~public", active, effect, &mut report).await;
    if prune_private_resolver {
        prune_private_resolver_scopes(cache_dir, effect, &mut report).await;
    }
    Ok(report)
}

#[derive(Clone, Copy)]
enum ScopeFilter {
    All,
    PrivateOnly,
}

async fn prune_private_resolver_scopes(
    cache_dir: &Path,
    effect: CacheGcEffect,
    report: &mut CacheReport,
) {
    let npm_private = cache_dir.join("resolver-cache/v12/metadata-private");
    prune_child_dirs(&npm_private, ScopeFilter::All, effect, report).await;
    for index_rel in ["resolver-cache/cargo-index", "resolver-cache/python-index"] {
        prune_indexed_private_scopes(&cache_dir.join(index_rel), effect, report).await;
    }
}

async fn prune_indexed_private_scopes(
    index_root: &Path,
    effect: CacheGcEffect,
    report: &mut CacheReport,
) {
    if !is_real_dir(index_root).await {
        return;
    }
    let Ok(mut registries) = fs::read_dir(index_root).await else {
        return;
    };
    while let Ok(Some(entry)) = registries.next_entry().await {
        if entry
            .file_type()
            .await
            .is_ok_and(|ft| ft.is_dir() && !ft.is_symlink())
        {
            prune_child_dirs(&entry.path(), ScopeFilter::PrivateOnly, effect, report).await;
        }
    }
}

async fn prune_child_dirs(
    parent: &Path,
    filter: ScopeFilter,
    effect: CacheGcEffect,
    report: &mut CacheReport,
) {
    if !is_real_dir(parent).await {
        return;
    }
    let Ok(mut entries) = fs::read_dir(parent).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        prune_child_entry(&entry, filter, effect, report).await;
    }
}

async fn prune_child_entry(
    entry: &tokio::fs::DirEntry,
    filter: ScopeFilter,
    effect: CacheGcEffect,
    report: &mut CacheReport,
) {
    if !entry
        .file_type()
        .await
        .is_ok_and(|ft| ft.is_dir() && !ft.is_symlink())
    {
        return;
    }
    if matches!(filter, ScopeFilter::PrivateOnly) && entry.file_name() == "public" {
        return;
    }
    if let Err(err) = remove_orphan_dir(&entry.path(), effect, report).await {
        tracing::warn!(error = %err, path = %entry.path().display(), "orphan resolver scope cleanup failed");
    }
}

async fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).await
        .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
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
