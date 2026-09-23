use crate::{
    CasPrefetch, CreateVirtualStoreStoreContext, CustomFetcherSession, SkippedSnapshots,
    VirtualStoreLayout,
};
use pnpm_cmd_shim::LinkBinsOptions;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, PackageKey, PackageMetadata, ProjectSnapshot};
use pnpm_network::ThrottledClient;
use pnpm_store_dir::StoreIndexWriter;
use pnpm_tarball::{MemCache, SharedReportedProgressKeys};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct VirtualStoreFetchInputs<'a> {
    pub http_client: &'a ThrottledClient,
    /// Shared store-index writer for the install. Owned by
    /// `InstallFrozenLockfile`, threaded down here for the cold-batch
    /// download path's `InstallPackageBySnapshot` and also reused by
    /// `BuildModules` for the side-effects-cache WRITE path.
    pub store_index_writer: &'a std::sync::Arc<StoreIndexWriter>,
    pub store_context: Option<CreateVirtualStoreStoreContext<'a>>,
    /// A [`CasPrefetch`] the caller started early so its store reads
    /// overlap caller-side async work; `None` makes [`crate::CreateVirtualStore::run`] start
    /// one itself. Must have been started with this run's `snapshots` /
    /// `packages`.
    pub cas_prefetch: Option<CasPrefetch>,
    /// Cache keys whose package status (`fetched` or `found_in_store`)
    /// has already been emitted earlier in this install. The warm batch
    /// still emits `resolved` for those packages, but skips the second
    /// status event so resolve-time prefetch progress is visible without
    /// being double-counted.
    pub progress_reported: &'a SharedReportedProgressKeys,
    /// Install-scoped shared in-flight tarball cache, threaded into each
    /// per-snapshot [`InstallPackageBySnapshot`](crate::InstallPackageBySnapshot) so the cold-batch
    /// download reuses a background prefetcher's in-flight download
    /// instead of re-fetching. `Some` whenever a prefetcher is active —
    /// the pnpr client's `TarballPrefetcher` (frozen path) or
    /// the fresh-resolve path's `PrefetchingResolver` (closing
    /// <https://github.com/pnpm/pnpm/issues/12241>); `None` otherwise.
    pub tarball_mem_cache: Option<&'a std::sync::Arc<MemCache>>,
    /// Custom fetchers from the pnpmfile. Consulted per snapshot
    /// before the built-in resolution-type dispatch.
    pub custom_fetcher_session: Option<&'a Arc<CustomFetcherSession>>,
    /// Fetch-evidence cell filled right after the warm/cold partition
    /// with the cold registry-resolved snapshots this run downloads —
    /// see [`pnpm_resolving_resolver_base::PlannedCanonicalFetches`].
    /// `None` for callers with no concurrent verification fan-out to
    /// feed (the fresh-resolve path, `--filter` passes, tests).
    pub planned_canonical_fetches:
        Option<&'a pnpm_resolving_resolver_base::PlannedCanonicalFetches>,
}

#[derive(Clone, Copy)]
pub struct SnapshotSelection<'a> {
    /// Snapshots the installability pass marked optional+incompatible
    /// on this host. Their virtual-store slots are not created — the
    /// warm/cold partition skips them, and the bundled-manifest +
    /// side-effects-cache lookups they would feed downstream phases
    /// are likewise omitted: only non-skipped snapshots are
    /// materialized into the graph passed to the build phase.
    pub skipped: &'a SkippedSnapshots,
    /// Whether snapshot `optionalDependencies` are included in this
    /// materialization.
    pub include_optional: bool,
    pub supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
}

#[derive(Clone, Copy)]
pub struct ImporterLinkContext<'a> {
    pub config: &'static Config,
    /// Install-scoped slot-directory mapping (GVS-aware). Drives the
    /// per-direct-dep symlink target — `node_modules/<dep>` resolves
    /// to `layout.slot_dir(<key>)/node_modules/<dep>`. See
    /// [`crate::VirtualStoreLayout`].
    pub layout: &'a VirtualStoreLayout,
    /// Workspace root. For a single-project install this is the
    /// directory containing the user's `package.json`; for a real
    /// workspace it's the directory containing `pnpm-workspace.yaml`.
    /// Same value as the `lockfileDir` used for
    /// `pnpm:stage` / `pnpm:summary` events.
    pub workspace_root: &'a Path,

    /// [`crate::shim_link_options`] output — threaded into the
    /// per-importer `.bin` shim pass.
    pub link_options: &'a LinkBinsOptions,
}

#[derive(Clone, Copy)]
pub struct ImporterDependencyGraph<'a> {
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    /// Per-package metadata from the lockfile. Non-registry packages carry
    /// their manifest version here because their importer version slot is a
    /// URL or path rather than the package's semantic version.
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    /// Snapshots the installability pass marked optional+incompatible.
    /// A direct dep whose resolved snapshot key is in this set is
    /// omitted from `node_modules/<name>` (no symlink, no
    /// `pnpm:root added` event, no bin linking).
    pub skipped: &'a SkippedSnapshots,
}

#[derive(Clone, Copy)]
pub struct DirectLinkPolicy<'a> {
    /// `<alias → resolved-target-path>` for every transitive that the
    /// hoist pass will publicly hoist into the root's `node_modules/`.
    /// Folded into the dedupe map alongside the root importer's direct
    /// deps so a non-root importer's direct dep resolving to the same
    /// target as a publicly-hoisted alias is also deduped — matching
    /// pnpm where `linkDirectDepsAndDedupe` reads root's `node_modules/`
    /// *after* the hoist pass already populated it. Pacquet's pipeline
    /// runs hoist after this step, so the caller pre-computes the
    /// hoist plan ([`crate::get_hoisted_dependencies`]) and threads
    /// the public-side targets in here.
    pub public_hoist_targets: Option<&'a BTreeMap<String, PathBuf>>,

    /// Importer ids whose project directories the caller *knows* —
    /// they came from the install's own project list (the programmatic
    /// API's in-memory projects, or `pnpm-workspace.yaml` discovery),
    /// not from parsed lockfile input. These bypass
    /// [`crate::validate_importer_id`]: a declared project may legitimately
    /// live outside the lockfile dir (importer id `..` or `../foo`) —
    /// Bit's capsule installs do exactly that, and pnpm v11 linked
    /// such importers without complaint. Ids *not* in this set keep
    /// the strict malformed-lockfile rejection.
    pub trusted_importer_ids: Option<&'a HashSet<String>>,

    /// When `true`, skip every direct dep whose resolved version
    /// is [`pnpm_lockfile::ImporterDepVersion::Regular`] and only materialize
    /// [`pnpm_lockfile::ImporterDepVersion::Link`] entries — workspace siblings
    /// resolved through `workspace:*` / `link:`. Used by the
    /// hoisted linker to layer workspace-sibling symlinks on top
    /// of the real-directory tree the slice 5 linker produced;
    /// the regular deps already landed under
    /// `<importer>/node_modules/<alias>/` as real directories
    /// from the hoisted linker, and re-symlinking them would
    /// either no-op or corrupt the layout.
    ///
    /// In the hoisted branch this runs after
    /// `linkHoistedModules` with the direct-dependency map filtered to
    /// only `link:`-shaped entries.
    pub link_only: bool,
}

#[derive(Clone, Copy)]
pub struct SkipSetClosure<'a> {
    /// The lockfile whose importers anchor the reachability closure —
    /// the full one, even under a filtered install.
    pub lockfile: &'a Lockfile,
    pub root: &'a Path,
    pub importer_ids: &'a HashSet<String>,
    pub groups: crate::GroupSelection,
}
