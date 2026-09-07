//! Materializes packages into the virtual store while resolution is
//! still running.
//!
//! The tree walk announces every package whose subtree has settled
//! peer-free ([`FinalizedPackage`]); such a package's slot name and
//! child edges are already final, and its tarball is on its way into
//! the store through the prefetching resolver. Populating the slot
//! now, with the CAS path map the prefetch leaves in the tarball
//! [`MemCache`], moves the hardlink fan-out off the post-resolution
//! critical path. The later `CreateVirtualStore` pass finds the
//! completion marker, skips the import, and re-links the slot's edges
//! from the lockfile, so a slot whose children were re-recorded after
//! the announcement heals there.

use pnpm_config::{Config, PackageImportMethod};
use pnpm_deps_restorer::{
    ImportIndexedDirOpts, SkippedSnapshots, VirtualStoreLayout, create_symlink_layout,
    import_indexed_dir, install_package_from_registry::extract_tarball,
    safe_join_modules_dir::safe_join_modules_dir,
};
use pnpm_lockfile::{
    LockfileResolution, PackageKey, PkgName, SnapshotDepRef, is_git_hosted_tarball_url,
};
use pnpm_resolving_deps_resolver::{FinalizedPackage, FinalizedPackageFn};
use pnpm_tarball::{CacheValue, MemCache};
use std::{
    collections::HashMap,
    marker::PhantomData,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{sync::Semaphore, task::JoinSet};

/// How long a materialization task waits between looks at the tarball
/// cache while the prefetch it depends on has not registered yet.
const CACHE_POLL_INTERVAL: Duration = Duration::from_millis(20);

pub(crate) struct EarlyMaterializer<Reporter> {
    shared: Arc<Shared>,
    tasks: Mutex<JoinSet<()>>,
    /// Every slot a task was spawned for, so slots the final lockfile
    /// does not carry can be removed again.
    slots: Mutex<Vec<(PackageKey, PathBuf)>>,
    _reporter: PhantomData<fn() -> Reporter>,
}

struct Shared {
    layout: VirtualStoreLayout,
    import_method: PackageImportMethod,
    symlink: bool,
    /// Install-scoped dedupe state for the `pnpm:package-import-method`
    /// log, merged into the install's own state at [`EarlyMaterializer::finish`].
    logged_methods: AtomicU8,
    mem_cache: Arc<MemCache>,
    permits: Semaphore,
    closing: AtomicBool,
    materialized: AtomicUsize,
}

impl<Reporter: pnpm_reporter::Reporter + 'static> EarlyMaterializer<Reporter> {
    pub(crate) fn new(config: &Config, mem_cache: Arc<MemCache>) -> Self {
        let max_length = usize::try_from(config.virtual_store_dir_max_length).unwrap_or(usize::MAX);
        let permits = std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get);
        EarlyMaterializer {
            shared: Arc::new(Shared {
                layout: VirtualStoreLayout::legacy(config.virtual_store_dir.clone(), max_length),
                import_method: config.package_import_method,
                symlink: config.symlink,
                logged_methods: AtomicU8::new(0),
                mem_cache,
                permits: Semaphore::new(permits),
                closing: AtomicBool::new(false),
                materialized: AtomicUsize::new(0),
            }),
            tasks: Mutex::new(JoinSet::new()),
            slots: Mutex::new(Vec::new()),
            _reporter: PhantomData,
        }
    }

    /// The sink to hand the resolver as
    /// [`pnpm_resolving_deps_resolver::WorkspaceResolveOptions::finalized_package`].
    pub(crate) fn hook(self: &Arc<Self>) -> FinalizedPackageFn {
        let materializer = Arc::clone(self);
        Arc::new(move |package| materializer.schedule(&package))
    }

    fn schedule(&self, package: &FinalizedPackage) {
        let Some(name_ver) = package.result.name_ver.as_ref() else { return };
        let Ok((package_url, _)) = extract_tarball(&package.result.resolution) else { return };
        // The prefetch keys its cache by the plain URL and skips these
        // shapes altogether; see `PrefetchingResolver::maybe_kickoff_download`.
        let revision_addressed = matches!(
            &package.result.resolution,
            LockfileResolution::Tarball(tarball) if tarball.revision.is_some(),
        );
        if revision_addressed
            || package_url.starts_with("file:")
            || is_git_hosted_tarball_url(package_url)
        {
            return;
        }
        // A patched package is imported and patched by the normal path.
        if package.pkg_id.contains("(patch_hash=") {
            return;
        }
        let Ok(key) = package.pkg_id.parse::<PackageKey>() else { return };
        let slot_dir = self.shared.layout.slot_dir(&key);
        let virtual_node_modules_dir = slot_dir.join("node_modules");
        let Ok(package_dir) =
            safe_join_modules_dir(&virtual_node_modules_dir, &name_ver.name.to_string())
        else {
            return;
        };
        // Optional edges are left to the final pass, which knows which
        // optional dependencies the installability pass skipped.
        let dependencies: HashMap<PkgName, SnapshotDepRef> = package
            .children
            .iter()
            .filter(|child| !child.optional)
            .filter_map(|child| {
                let alias = PkgName::parse(child.alias.as_str()).ok()?;
                let key = child.pkg_id.parse::<PackageKey>().ok()?;
                Some((alias, SnapshotDepRef::Alias(key)))
            })
            .collect();
        let job = SlotJob {
            package_url: package_url.to_string(),
            self_name: name_ver.name.clone(),
            virtual_node_modules_dir,
            package_dir,
            dependencies,
        };
        lock(&self.slots).push((key, slot_dir));
        let shared = Arc::clone(&self.shared);
        lock(&self.tasks).spawn(async move { job.run::<Reporter>(&shared).await });
    }

    /// Stop scheduling, wait for the tasks in flight, and remove the
    /// slots of packages the final install does not materialize.
    /// Returns the number of slots materialized.
    pub(crate) async fn finish(
        &self,
        is_wanted: impl Fn(&PackageKey) -> bool,
        logged_methods: &AtomicU8,
    ) -> usize {
        self.shared.closing.store(true, Ordering::Release);
        let mut tasks = std::mem::take(&mut *lock(&self.tasks));
        while tasks.join_next().await.is_some() {}
        logged_methods
            .fetch_or(self.shared.logged_methods.load(Ordering::Acquire), Ordering::AcqRel);
        let orphans: Vec<PathBuf> = std::mem::take(&mut *lock(&self.slots))
            .into_iter()
            .filter(|(key, _)| !is_wanted(key))
            .map(|(_, slot_dir)| slot_dir)
            .collect();
        if !orphans.is_empty() {
            let _ = tokio::task::spawn_blocking(move || {
                for slot_dir in orphans {
                    let _ = std::fs::remove_dir_all(slot_dir);
                }
            })
            .await;
        }
        self.shared.materialized.load(Ordering::Acquire)
    }
}

struct SlotJob {
    package_url: String,
    self_name: PkgName,
    virtual_node_modules_dir: PathBuf,
    package_dir: PathBuf,
    dependencies: HashMap<PkgName, SnapshotDepRef>,
}

impl SlotJob {
    async fn run<Reporter: pnpm_reporter::Reporter>(self, shared: &Arc<Shared>) {
        let Some(cas_paths) = wait_for_cas_paths(shared, &self.package_url).await else { return };
        let Ok(_permit) = shared.permits.acquire().await else { return };
        // Once the install is linking, the link phase's own parallel
        // pass takes the slot; finishing it here would only delay that.
        if shared.closing.load(Ordering::Acquire) {
            return;
        }
        let shared = Arc::clone(shared);
        let outcome = tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&self.virtual_node_modules_dir)
                .map_err(|error| error.to_string())?;
            import_indexed_dir::<Reporter>(
                &shared.logged_methods,
                shared.import_method,
                &self.package_dir,
                &cas_paths,
                ImportIndexedDirOpts::default(),
            )
            .map_err(|error| error.to_string())?;
            if shared.symlink {
                create_symlink_layout(
                    Some(&self.dependencies),
                    None,
                    false,
                    &self.self_name,
                    &SkippedSnapshots::new(),
                    &shared.layout,
                    &self.virtual_node_modules_dir,
                )
                .map_err(|error| error.to_string())?;
            }
            shared.materialized.fetch_add(1, Ordering::AcqRel);
            Ok::<(), String>(())
        })
        .await;
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::debug!(
                target: "pacquet::install",
                package_url = %self.package_url,
                %error,
                "early materialization failed; the link phase retries the slot",
            ),
            Err(error) => tracing::debug!(
                target: "pacquet::install",
                package_url = %self.package_url,
                %error,
                "early materialization task panicked; the link phase retries the slot",
            ),
        }
    }
}

/// Wait for the prefetch of `package_url` to land its CAS path map in
/// the tarball cache. `None` when the fetch failed or when the
/// materializer closes first, which covers both a tarball the prefetch
/// skipped (it never registers) and one still in flight when the
/// install starts linking.
async fn wait_for_cas_paths(
    shared: &Shared,
    package_url: &str,
) -> Option<Arc<HashMap<String, PathBuf>>> {
    loop {
        let slot = shared.mem_cache.get(package_url).map(|entry| Arc::clone(entry.value()));
        let Some(slot) = slot else {
            if shared.closing.load(Ordering::Acquire) {
                return None;
            }
            tokio::time::sleep(CACHE_POLL_INTERVAL).await;
            continue;
        };
        let notify = match &*slot.read().await {
            CacheValue::Available(cas_paths) => return Some(Arc::clone(cas_paths)),
            CacheValue::Failed => return None,
            CacheValue::InProgress(notify) => Arc::clone(notify),
        };
        // A bounded wait rather than a bare `notified()`: the owner
        // notifies only once, on the flip, and this wait registers after
        // the read above, so the flip may already have happened.
        let _ = tokio::time::timeout(CACHE_POLL_INTERVAL * 5, notify.notified()).await;
        if shared.closing.load(Ordering::Acquire) {
            return None;
        }
    }
}

fn lock<Inner>(mutex: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
