use super::{CacheGcEffect, CacheReport, HashSet, Path, PathBuf, Result};
use pnpm_fs::OpenDir;
use std::{
    ffi::OsStr,
    io::{self, ErrorKind},
};

pub(super) struct OrphanPruneOutcome {
    pub(super) report: CacheReport,
    pub(super) pruned_dirs: HashSet<PathBuf>,
    pub(super) private_scopes_clean: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PrivateScopePolicy {
    Keep,
    Prune,
}

#[derive(Clone, Copy)]
enum ScopeFilter {
    All,
    PrivateOnly,
}

/// Prune orphaned `~upstreams/` and `~public/` namespaces, and purge private
/// resolver cache scopes when the cache secret rotated.
pub(super) async fn prune_orphaned_namespaces(
    cache_dir: &Path,
    active: &HashSet<String>,
    private_scopes: PrivateScopePolicy,
    effect: CacheGcEffect,
) -> Result<OrphanPruneOutcome> {
    let root = cache_dir.to_path_buf();
    let active = active.clone();
    tokio::task::spawn_blocking(move || {
        prune_orphaned_namespaces_sync(&root, &active, private_scopes, effect)
    })
    .await
    .map_err(|err| io::Error::other(err.to_string()))?
}

pub(super) async fn prune_private_resolver_scopes_only(
    cache_dir: &Path,
    active: &HashSet<String>,
    effect: CacheGcEffect,
) -> Result<bool> {
    let root = cache_dir.to_path_buf();
    let active = active.clone();
    tokio::task::spawn_blocking(move || {
        let mut outcome = OrphanPruneOutcome {
            report: CacheReport::default(),
            pruned_dirs: HashSet::new(),
            private_scopes_clean: true,
        };
        let root_dir = match OpenDir::open(&root) {
            Ok(dir) => dir,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(true),
            Err(err) => return Err(err.into()),
        };
        prune_orphan_prefix(&root_dir, "~upstreams", &active, effect, &mut outcome);
        prune_private_resolver_scopes(&root_dir, effect, &mut outcome);
        Ok(outcome.private_scopes_clean)
    })
    .await
    .map_err(|err| io::Error::other(err.to_string()))?
}

fn prune_orphaned_namespaces_sync(
    cache_dir: &Path,
    active: &HashSet<String>,
    private_scopes: PrivateScopePolicy,
    effect: CacheGcEffect,
) -> Result<OrphanPruneOutcome> {
    let mut outcome = OrphanPruneOutcome {
        report: CacheReport::default(),
        pruned_dirs: HashSet::new(),
        private_scopes_clean: true,
    };
    let root_dir = match OpenDir::open(cache_dir) {
        Ok(dir) => dir,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(outcome),
        Err(err) => return Err(err.into()),
    };
    prune_orphan_prefix(&root_dir, "~upstreams", active, effect, &mut outcome);
    prune_orphan_prefix(&root_dir, "~public", active, effect, &mut outcome);
    if matches!(private_scopes, PrivateScopePolicy::Prune) {
        prune_private_resolver_scopes(&root_dir, effect, &mut outcome);
    }
    Ok(outcome)
}

fn record_orphan_prefix_error(prefix: &str, outcome: &mut OrphanPruneOutcome) {
    if prefix == "~upstreams" {
        outcome.private_scopes_clean = false;
    }
}

fn prune_orphan_prefix(
    root_dir: &OpenDir,
    prefix: &str,
    active: &HashSet<String>,
    effect: CacheGcEffect,
    outcome: &mut OrphanPruneOutcome,
) {
    let parent = match root_dir.open_child_dir(prefix.as_ref()) {
        Ok(Some(dir)) => dir,
        Ok(None) => return,
        Err(err) => {
            record_orphan_prefix_error(prefix, outcome);
            tracing::warn!(error = %err, path = %root_dir.path().join(prefix).display(), "opening cache namespace root failed");
            return;
        }
    };
    let names = match parent.entry_names() {
        Ok(names) => names,
        Err(err) => {
            record_orphan_prefix_error(prefix, outcome);
            tracing::warn!(error = %err, path = %parent.path().display(), "listing cache namespace root failed");
            return;
        }
    };
    prune_orphan_entries(&parent, prefix, names, active, effect, outcome);
}

fn prune_orphan_entries(
    parent: &OpenDir,
    prefix: &str,
    names: Vec<std::ffi::OsString>,
    active: &HashSet<String>,
    effect: CacheGcEffect,
    outcome: &mut OrphanPruneOutcome,
) {
    for name in names {
        let ns = format!("{prefix}/{}", name.to_string_lossy());
        if active.contains(&ns) {
            continue;
        }
        if let Err(err) = remove_orphan_child(parent, &name, effect, outcome) {
            record_orphan_prefix_error(prefix, outcome);
            tracing::warn!(
                error = %err,
                path = %parent.path().join(&name).display(),
                "orphan cache namespace cleanup failed",
            );
        }
    }
}

fn prune_private_resolver_scopes(
    root_dir: &OpenDir,
    effect: CacheGcEffect,
    outcome: &mut OrphanPruneOutcome,
) {
    let npm_rel = Path::new("resolver-cache").join("v12").join("metadata-private");
    prune_descendant_children(root_dir, &npm_rel, ScopeFilter::All, effect, outcome);
    for index_name in ["cargo-index", "python-index"] {
        let rel = Path::new("resolver-cache").join(index_name);
        prune_indexed_private_scopes(root_dir, &rel, effect, outcome);
    }
}

fn prune_indexed_private_scopes(
    root_dir: &OpenDir,
    index_rel: &Path,
    effect: CacheGcEffect,
    outcome: &mut OrphanPruneOutcome,
) {
    let index_dir = match root_dir.open_descendant_dir(index_rel) {
        Ok(Some(dir)) => dir,
        Ok(None) => return,
        Err(err) => {
            outcome.private_scopes_clean = false;
            tracing::warn!(error = %err, path = %root_dir.path().join(index_rel).display(), "opening resolver index cache failed");
            return;
        }
    };
    let registries = match index_dir.entry_names() {
        Ok(names) => names,
        Err(err) => {
            outcome.private_scopes_clean = false;
            tracing::warn!(error = %err, path = %index_dir.path().display(), "listing resolver index registries failed");
            return;
        }
    };
    for reg_name in registries {
        match index_dir.open_child_dir(&reg_name) {
            Ok(Some(reg_dir)) => {
                prune_open_dir_children(&reg_dir, ScopeFilter::PrivateOnly, effect, outcome);
            }
            Ok(None) => {}
            Err(err) => {
                outcome.private_scopes_clean = false;
                tracing::warn!(error = %err, path = %index_dir.path().join(&reg_name).display(), "opening resolver registry cache failed");
            }
        }
    }
}

fn prune_descendant_children(
    root_dir: &OpenDir,
    rel: &Path,
    filter: ScopeFilter,
    effect: CacheGcEffect,
    outcome: &mut OrphanPruneOutcome,
) {
    match root_dir.open_descendant_dir(rel) {
        Ok(Some(parent)) => prune_open_dir_children(&parent, filter, effect, outcome),
        Ok(None) => {}
        Err(err) => {
            outcome.private_scopes_clean = false;
            tracing::warn!(error = %err, path = %root_dir.path().join(rel).display(), "opening private resolver cache failed");
        }
    }
}

fn prune_open_dir_children(
    parent: &OpenDir,
    filter: ScopeFilter,
    effect: CacheGcEffect,
    outcome: &mut OrphanPruneOutcome,
) {
    let entries = match parent.entry_names() {
        Ok(names) => names,
        Err(err) => {
            outcome.private_scopes_clean = false;
            tracing::warn!(error = %err, path = %parent.path().display(), "listing private resolver cache failed");
            return;
        }
    };
    for name in entries {
        if matches!(filter, ScopeFilter::PrivateOnly) && name == "public" {
            continue;
        }
        if let Err(err) = remove_orphan_child(parent, &name, effect, outcome) {
            outcome.private_scopes_clean = false;
            tracing::warn!(
                error = %err,
                path = %parent.path().join(&name).display(),
                "orphan resolver scope cleanup failed",
            );
        }
    }
}

fn remove_orphan_child(
    parent: &OpenDir,
    name: &OsStr,
    effect: CacheGcEffect,
    outcome: &mut OrphanPruneOutcome,
) -> io::Result<()> {
    let Some(child_dir) = parent.open_child_dir(name)? else {
        return Ok(());
    };
    let child_path = child_dir.path().to_path_buf();
    let (count, bytes) = count_dir_contents(child_dir)?;
    match effect {
        CacheGcEffect::Apply => {
            if !parent.remove_child_dir(name)? {
                return Ok(());
            }
        }
        CacheGcEffect::DryRun => {
            outcome.pruned_dirs.insert(child_path);
        }
    }
    outcome.report.namespaces_removed += 1;
    outcome.report.files_evicted += count;
    outcome.report.bytes_reclaimed += bytes;
    Ok(())
}

pub(super) fn count_dir_contents(root: OpenDir) -> io::Result<(usize, u64)> {
    let mut count = 0usize;
    let mut bytes = 0u64;
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for name in dir.entry_names()? {
            if let Some(subdir) = dir.open_child_dir(&name)? {
                stack.push(subdir);
            } else if let Some(stat) = dir.child_file_stat(&name)? {
                count += 1;
                bytes += stat.len;
            }
        }
    }
    Ok((count, bytes))
}
