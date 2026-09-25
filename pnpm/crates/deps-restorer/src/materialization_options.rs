use crate::{CustomFetcherSession, SkippedSnapshots, VirtualStoreLayout};
use pnpm_cmd_shim::LinkBinsOptions;
use pnpm_config::{Config, NodeLinker, PackageImportMethod};
use pnpm_lockfile::{PackageKey, PkgName, SnapshotEntry};
use pnpm_network::ThrottledClient;
use pnpm_store_dir::{SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreIndexWriter};
use pnpm_tarball::{MemCache, PrefetchedCasPaths, SharedReportedProgressKeys};
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicU8},
};

#[derive(Debug, Clone, Copy)]
pub struct PackageImportOptions<'a> {
    pub method: PackageImportMethod,
    /// [`Config::package_import_patterns`]: which files of each package are imported.
    pub patterns: &'a [String],
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// See the comment on `link_file::log_method_once` for why this
    /// is install-scoped rather than module-static.
    pub logged_methods: &'a AtomicU8,
    /// Install root, threaded into `pnpm:progress` `imported`'s
    /// `requester`. Same value as the `prefix` in
    /// [`pnpm_reporter::StageLog`].
    pub requester: &'a str,
}

#[derive(Clone, Copy)]
pub struct SlotImportSource<'a> {
    /// Whether this package's file map points at mutable local source
    /// (a `file:` / [`pnpm_lockfile::LockfileResolution::Directory`]
    /// resolution) rather than immutable CAS entries. pnpm's `file:` is
    /// a copy taken at install time — unlike `link:`, which symlinks —
    /// so the slot has to be rebuilt on every install: the source can
    /// change without the lockfile changing, and the completion-marker
    /// short-circuit in [`fn@crate::import_indexed_dir`] would otherwise
    /// leave the previous install's copy in place forever.
    pub is_mutable: bool,
    /// Whether an existing slot contains a different immutable artifact
    /// under the same package key and must be replaced.
    pub force: bool,
    /// Empty source file imported as `.pnpm-needs-build` before the package's
    /// atomic completion marker when the package needs a build or patch.
    pub build_marker: Option<&'a Path>,
    /// Whether a lifecycle script or a patch will still write this slot's
    /// files after the import. Such a slot must not share inodes with its
    /// source, so it ignores [`PackageImportOptions::method`] and imports
    /// with `clone-or-copy`. See
    /// [`fn@crate::create_virtual_dir_by_snapshot::effective_import_method`].
    pub needs_build: bool,
}

#[derive(Clone, Copy)]
pub struct SnapshotDependencyLinks<'a> {
    pub package_key: &'a PackageKey,
    pub snapshot: &'a SnapshotEntry,
    /// Snapshots whose slots were not materialized on this host —
    /// platform-mismatched optionals, `--no-optional` exclusions, and
    /// swallowed optional fetch failures. `create_symlink_layout`
    /// uses this to skip dangling symlinks to absent slots: an
    /// uninstallable optional snapshot is never linked.
    pub skipped: &'a SkippedSnapshots,
    /// Whether links from the snapshot's `optionalDependencies` map
    /// participate in the slot layout.
    pub include_optional: bool,
    /// Child aliases that were linked by a previous install but are no
    /// longer in this snapshot's dependency set. Their stale symlinks
    /// are unlinked from the slot before the progress event fires, so
    /// a warm reinstall that drops a dependency (e.g. via an override)
    /// doesn't leave a dangling child behind. Empty for fresh packages
    /// and for survivors whose dependency set only changed by addition.
    pub removed_aliases: &'a [PkgName],
    /// Whether dependency links inside the slot should be created.
    /// `symlink: false` still imports the package itself but leaves its
    /// `node_modules` free of graph links for `PnP` resolution.
    pub symlink: bool,
}

#[derive(Clone, Copy)]
pub struct VirtualStoreLinkOptions<'a> {
    pub layout: &'a crate::VirtualStoreLayout,
    pub dir_clone_cache: Option<&'a crate::DirCloneCache<'a>>,
    pub symlink: bool,
    pub skipped: &'a SkippedSnapshots,
    pub include_optional: bool,
}

#[derive(Clone, Copy)]
pub struct SnapshotFetchContext<'a> {
    pub http_client: &'a ThrottledClient,
    pub store_index: Option<&'a SharedReadonlyStoreIndex>,
    pub store_index_writer: Option<&'a Arc<StoreIndexWriter>>,
    /// Install-scoped batched cache lookup result. See
    /// [`pnpm_tarball::prefetch_cas_paths`].
    pub prefetched_cas_paths: Option<&'a PrefetchedCasPaths>,
    /// Install-scoped shared in-flight tarball cache. When present, the
    /// registry/tarball download routes through
    /// [`IngestTarballToStore::run_with_mem_cache`](pnpm_tarball::IngestTarballToStore::run_with_mem_cache) so it parks on (or
    /// reuses) a download already in flight or completed for the same
    /// URL, rather than racing a second fetch of the same bytes. Both
    /// background prefetchers feed it: the pnpr client's
    /// `TarballPrefetcher` (frozen materialization) and the
    /// fresh-resolve path's `PrefetchingResolver` (cold
    /// batch). `None` keeps the standalone `run_without_mem_cache`
    /// path for installs with no prefetcher (e.g. a plain
    /// `--frozen-lockfile` without pnpr).
    pub tarball_mem_cache: Option<&'a Arc<MemCache>>,
    /// Install-scoped package-status progress dedupe. Shared with the
    /// resolve-time prefetcher on the fresh path so the cold fallback
    /// does not double-count a package whose early prefetch already
    /// emitted `fetched` or `found_in_store`.
    pub progress_reported: Option<&'a SharedReportedProgressKeys>,
    /// Install-scoped `verifiedFilesCache` shared across every
    /// per-snapshot fetch. See `IngestTarballToStore::verified_files_cache`
    /// for the rationale.
    pub verified_files_cache: &'a SharedVerifiedFilesCache,
    /// Custom fetchers from the pnpmfile's `fetchers` export.
    /// Consulted before the built-in resolution-type dispatch; `None`
    /// when no pnpmfile exports fetchers.
    pub custom_fetcher_session: Option<&'a Arc<CustomFetcherSession>>,
}

#[derive(Clone, Copy)]
pub struct RegistryFetchContext<'a> {
    pub http_client: &'a ThrottledClient,
    pub config: &'static Config,
    pub store_index: Option<&'a SharedReadonlyStoreIndex>,
    pub store_index_writer: Option<&'a Arc<StoreIndexWriter>>,
    /// Install-scoped `verifiedFilesCache` shared across every
    /// per-package fetch. See `IngestTarballToStore::verified_files_cache`
    /// for the rationale.
    pub verified_files_cache: &'a SharedVerifiedFilesCache,
    /// Warm-cache prefetch result built once per install via
    /// [`pnpm_tarball::prefetch_cas_paths`] — `cache_key →
    /// Arc<cas_paths>`. When `Some`, the
    /// `IngestTarballToStore::run_without_mem_cache` cache-lookup
    /// branch reads from here before falling back to the per-snapshot
    /// `SQLite` lookup, avoiding `Arc<Mutex<StoreIndex>>` contention on
    /// the resolve hot path.
    pub prefetched_cas_paths: Option<&'a pnpm_tarball::PrefetchedCasPaths>,
    pub tarball_mem_cache: &'a MemCache,
}

#[derive(Clone, Copy)]
pub struct ModuleLinkerContext<'a> {
    /// Install-scoped slot-directory mapping (GVS-aware). Every consumer
    /// that needs to know where a snapshot's slot is routes through it.
    pub layout: &'a VirtualStoreLayout,
    pub kind: NodeLinker,
    pub bin_options: &'a LinkBinsOptions,
}
