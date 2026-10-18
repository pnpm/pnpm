//! `pnpr upgrade-storage`: convert a hosted store to [`pnpr_storage::layout`].
//! Stop every pnpr process that uses the store first.

mod plan;

use futures_util::TryStreamExt;
use miette::IntoDiagnostic as _;
use object_store::{ObjectStore, ObjectStoreExt as _, path::Path as ObjectPath};
use plan::Step;
use pnpr_config::{Config, HostedStoreConfig, build_s3_store, normalize_key_prefix};
use pnpr_storage::{
    HostedLayout, Storage,
    layout::{LAYOUT_FILE, LAYOUT_VERSION},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::fs;

#[derive(Debug, Default)]
pub struct UpgradeReport {
    /// How many paths each kind of step covered.
    pub counts: BTreeMap<&'static str, usize>,
    /// Files that belong to no package, left where they are.
    pub orphans: Vec<String>,
    pub already_current: bool,
    pub dry_run: bool,
}

/// Convert the configured hosted store. With `dry_run`, only report what
/// would change.
pub async fn upgrade_storage(config: &Config, dry_run: bool) -> miette::Result<UpgradeReport> {
    let storage = Storage::new(
        &config.storage.hosted_backend,
        config.storage.hosted_dir.clone(),
        config.storage.cache_dir.clone(),
    )
    .into_diagnostic()?;
    let orgs: BTreeSet<String> = config.routing.hosted
        .values()
        .map(|hosted| hosted.org.clone())
        .filter(|org| !org.is_empty())
        .collect();
    upgrade(&storage, &HostedStore::open(config)?, &orgs, dry_run).await
}

async fn upgrade(
    storage: &Storage,
    store: &HostedStore,
    orgs: &BTreeSet<String>,
    dry_run: bool,
) -> miette::Result<UpgradeReport> {
    let mut report = UpgradeReport { dry_run, ..UpgradeReport::default() };
    match storage.hosted_layout().await.into_diagnostic()? {
        HostedLayout::Unmarked { has_data: true } => {}
        HostedLayout::Current | HostedLayout::Unmarked { has_data: false } => {
            report.already_current = true;
            return Ok(report);
        }
        HostedLayout::Other(layout) => {
            return Err(miette::miette!(
                "the hosted store uses layout {layout}, and this pnpr converts only the unmarked layout to layout {LAYOUT_VERSION}",
            ));
        }
    }
    let steps =
        plan::plan(&store.list().await?, orgs).map_err(|reason| miette::miette!("{reason}"))?;
    report.counts = plan::tally(&steps);
    report.orphans = steps
        .iter()
        .filter(|(_, step)| *step == Step::Orphan)
        .map(|(path, _)| path.clone())
        .collect();
    if !dry_run {
        store.apply(&steps).await?;
        storage.write_hosted_layout_marker().await.into_diagnostic()?;
    }
    Ok(report)
}

/// The hosted store as plain files or objects, below its root.
enum HostedStore {
    Local(PathBuf),
    Objects { store: Arc<dyn ObjectStore>, prefix: String },
}

impl HostedStore {
    fn open(config: &Config) -> miette::Result<Self> {
        Ok(match &config.storage.hosted_backend {
            HostedStoreConfig::Fs => HostedStore::Local(config.storage.hosted_dir.clone()),
            HostedStoreConfig::S3(settings) => HostedStore::Objects {
                store: build_s3_store(settings).into_diagnostic()?,
                prefix: settings.normalized_prefix(),
            },
            HostedStoreConfig::ObjectStore { store, prefix } => HostedStore::Objects {
                store: Arc::clone(store),
                prefix: normalize_key_prefix(Some(prefix)),
            },
        })
    }

    /// Every stored file, as `/`-separated path components below the root.
    async fn list(&self) -> miette::Result<Vec<String>> {
        match self {
            HostedStore::Local(root) => list_local(root).await,
            HostedStore::Objects { store, prefix } => list_objects(store.as_ref(), prefix).await,
        }
    }

    /// Carry out `steps`: every blob and record first, then the documents,
    /// so that a package an interrupted upgrade left half moved still has its
    /// old document and is planned again.
    async fn apply(&self, steps: &[(String, Step)]) -> miette::Result<()> {
        let ordered = steps
            .iter()
            .filter(|(_, step)| matches!(step, Step::Move { document: false, .. } | Step::Remove))
            .chain(
                steps
                    .iter()
                    .filter(|(_, step)| matches!(step, Step::Move { document: true, .. })),
            );
        for (from, step) in ordered {
            match self {
                HostedStore::Local(root) => apply_local(root, from, step).await?,
                HostedStore::Objects { store, prefix } => {
                    apply_object(store.as_ref(), prefix, from, step).await?;
                }
            }
        }
        if let HostedStore::Local(root) = self {
            remove_emptied_dirs(root, steps).await;
        }
        Ok(())
    }
}

async fn list_local(root: &Path) -> miette::Result<Vec<String>> {
    let mut paths = Vec::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = pending.pop() {
        let Some(mut entries) = read_dir_if_present(&dir).await.into_diagnostic()? else {
            continue;
        };
        while let Some(entry) = entries.next_entry().await.into_diagnostic()? {
            let path = child_path(&prefix, &entry.file_name().to_string_lossy());
            if entry
                .file_type()
                .await
                .into_diagnostic()?
                .is_dir()
            {
                pending.push((entry.path(), path));
            } else {
                paths.push(path);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn child_path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() { name.to_string() } else { format!("{prefix}/{name}") }
}

async fn apply_local(root: &Path, from: &str, step: &Step) -> miette::Result<()> {
    let source = root.join(from);
    match step {
        Step::Move { to, .. } => {
            let target = root.join(to);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).await.into_diagnostic()?;
            }
            fs::rename(&source, &target).await.into_diagnostic()
        }
        Step::Remove => fs::remove_file(&source).await.into_diagnostic(),
        Step::Orphan => Ok(()),
    }
}

/// Remove the directories the moves and removals emptied, up to the root.
async fn remove_emptied_dirs(root: &Path, steps: &[(String, Step)]) {
    let mut dirs: Vec<PathBuf> = steps
        .iter()
        .filter(|(_, step)| *step != Step::Orphan)
        .filter_map(|(from, _)| {
            root.join(from)
                .parent()
                .map(Path::to_path_buf)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    // Deepest first, so a parent is tried after its children.
    dirs.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    for dir in dirs {
        let mut current = dir.as_path();
        // A directory that still holds anything is not ours to remove.
        while current != root && fs::remove_dir(current).await.is_ok() {
            let Some(parent) = current.parent() else { break };
            current = parent;
        }
    }
}

async fn list_objects(store: &dyn ObjectStore, prefix: &str) -> miette::Result<Vec<String>> {
    let scope = (!prefix.is_empty()).then(|| ObjectPath::from(prefix.trim_end_matches('/')));
    let objects: Vec<_> = store
        .list(scope.as_ref())
        .try_collect()
        .await
        .into_diagnostic()?;
    let skip = scope
        .as_ref()
        .map_or(0, |scope| scope.parts().count());
    let mut paths: Vec<String> = objects
        .iter()
        .map(|meta| {
            meta.location
                .parts()
                .skip(skip)
                .map(|part| {
                    percent_encoding::percent_decode_str(part.as_ref())
                        .decode_utf8_lossy()
                        .into_owned()
                })
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect();
    paths.sort();
    Ok(paths)
}

async fn apply_object(
    store: &dyn ObjectStore,
    prefix: &str,
    from: &str,
    step: &Step,
) -> miette::Result<()> {
    let source = ObjectPath::from(format!("{prefix}{from}"));
    match step {
        Step::Move { to, .. } => {
            store
                .copy(&source, &ObjectPath::from(format!("{prefix}{to}")))
                .await
                .into_diagnostic()?;
            store.delete(&source).await.into_diagnostic()
        }
        Step::Remove => store.delete(&source).await.into_diagnostic(),
        Step::Orphan => Ok(()),
    }
}

async fn read_dir_if_present(dir: &Path) -> std::io::Result<Option<fs::ReadDir>> {
    match fs::read_dir(dir).await {
        Ok(entries) => Ok(Some(entries)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

#[must_use]
pub fn format_upgrade_report(report: &UpgradeReport) -> String {
    if report.already_current {
        return format!("The hosted store already has a current {LAYOUT_FILE}; nothing to do.\n");
    }
    let (moved, removed) =
        if report.dry_run { ("Would move", "Would remove") } else { ("Moved", "Removed") };
    let mut lines: Vec<String> = report.counts
        .iter()
        .filter(|(kind, _)| !matches!(**kind, "orphans" | "removed"))
        .map(|(kind, count)| format!("{moved} {count} {kind}"))
        .collect();
    if let Some(count) = report.counts.get("removed") {
        lines.push(format!("{removed} {count} listing-index and upload files"));
    }
    if !report.orphans.is_empty() {
        lines.push("Left in place, belonging to no package:".to_string());
        lines.extend(
            report.orphans
                .iter()
                .map(|orphan| format!("  {orphan}")),
        );
    }
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests;
