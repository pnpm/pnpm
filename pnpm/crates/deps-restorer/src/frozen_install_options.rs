use crate::install_frozen_lockfile::LockfileVerificationOverride;
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::{Lockfile, LockfileEntries};
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_resolver_base::ResolutionVerifier;
use pnpm_tarball::MemCache;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Copy)]
pub struct FrozenInstallDrivers<'a> {
    pub config: &'static Config,
    pub http_client: &'a ThrottledClient,
    pub pnpmfile_hook: Option<&'a Arc<dyn pnpm_hooks::PnpmfileHooks>>,

    /// Install-scoped shared in-flight tarball cache, threaded down to
    /// [`crate::CreateVirtualStore`]'s cold-batch downloads. `Some` on
    /// the pnpr client path so the materialization reuses the
    /// the package manager's `TarballPrefetcher` background downloads instead of
    /// re-fetching every tarball; `None` for installs without a shared
    /// prefetch in flight.
    pub tarball_mem_cache: Option<&'a Arc<MemCache>>,
}

#[derive(Clone, Copy)]
pub struct FrozenLockfileInputs<'a> {
    /// The wanted lockfile narrowed to what this install materializes:
    /// the closure of its importers under the included dependency
    /// groups. Its `importers`, `packages` and `snapshots` are read
    /// straight off it, so a caller cannot pair one lockfile's entries
    /// with another's maps.
    pub wanted: &'a Lockfile,
    /// The whole wanted lockfile, before [`Self::wanted`] was narrowed to
    /// the materialization closure. The verification gates read this one
    /// so a narrower install scope can never narrow what gets checked.
    pub verified: &'a Lockfile,
    /// Absolute path of the lockfile being verified, for the on-disk
    /// verification cache. `None` disables the cache.
    pub path: Option<&'a Path>,
    /// The previous install's persisted current lockfile, threaded
    /// through to the hoisted walker for `prev_graph` (orphan
    /// diff). `None` on a first install.
    pub current: Option<&'a Lockfile>,
    /// Entries from the previous install's `lock.yaml`, threaded through
    /// to [`crate::CreateVirtualStore`] for reuse decisions and child-link
    /// cleanup. See [`LockfileEntries::of_previous_install`].
    pub current_entries: LockfileEntries<'a>,
    /// Resolution verifiers to re-apply to every lockfile entry. Run
    /// concurrently with the fetch phase ([`crate::CreateVirtualStore`])
    /// and awaited before any dependency lifecycle script executes, so a
    /// rejected lockfile aborts before [`crate::BuildModules`] runs. Empty
    /// when verification is disabled (`trustLockfile`), in which case the
    /// gate is a no-op. The non-blocking sequencing runs
    /// `verifyLockfileResolutions` concurrently with the fetch and gates
    /// the build on `verifyLockfile`.
    pub resolution_verifiers: &'a [Arc<dyn ResolutionVerifier>],
    /// Fetch-evidence cell `CreateVirtualStore` fills after its
    /// warm/cold partition so the concurrent verification fan-out's
    /// age gate can lean on this install's canonical tarball fetches.
    /// See [`pnpm_resolving_resolver_base::PlannedCanonicalFetches`].
    pub planned_canonical_fetches:
        Option<&'a pnpm_resolving_resolver_base::PlannedCanonicalFetches>,
}

#[derive(Clone, Copy)]
pub struct FrozenProjectInputs<'a> {
    /// Install root — the directory containing `pnpm-lock.yaml`.
    /// For a real workspace, this is the workspace root (the dir
    /// containing `pnpm-workspace.yaml`); for a single-project
    /// install, it's the project dir.
    ///
    /// Reporter envelopes (`pnpm:stage`, `pnpm:summary`, `pnpm:lifecycle`)
    /// use [`requester`], a lossy-UTF-8 string view of this path —
    /// per-importer events like `pnpm:root` use the importer's own
    /// `rootDir` instead. Filesystem operations that need the real
    /// path (the per-importer `node_modules/` write under
    /// `SymlinkDirectDependencies`, the `lockfile_dir` threaded into
    /// `BuildModules`) use `workspace_root` directly so the round-trip
    /// through a lossy string can never corrupt the on-disk path on
    /// hosts with non-UTF-8 filenames.
    ///
    /// [`requester`]: Self::requester
    pub workspace_root: &'a Path,

    /// Lossy-UTF-8 view of [`workspace_root`] for reporter envelopes.
    /// Kept as a separate field rather than recomputed from
    /// `workspace_root` so the caller controls how the conversion is
    /// performed (today: `to_string_lossy().into_owned()` in
    /// `Install::run`).
    ///
    /// [`workspace_root`]: Self::workspace_root
    pub requester: &'a str,
    pub dependency_groups: &'a [DependencyGroup],
    pub manifests: &'a [(PathBuf, &'a pnpm_package_manifest::PackageManifest)],
    pub package_map_manifests: &'a [(PathBuf, &'a pnpm_package_manifest::PackageManifest)],
}

#[derive(Clone, Copy)]
pub struct FrozenPlatformOptions<'a> {
    /// CLI-merged `supportedArchitectures` from
    /// `pnpm-workspace.yaml` plus `--cpu` / `--os` / `--libc`
    /// overrides. Threaded into [`crate::InstallabilityHost`] so the
    /// platform-tagged optional-dependency filter respects user-
    /// supplied architecture overrides.
    pub supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,

    /// When `true`, runtime dependencies (`node@runtime:`,
    /// `deno@runtime:`, `bun@runtime:`) — i.e. packages whose
    /// metadata resolution is `Binary` or `Variations` — are
    /// added to the install-time skip set and the rest of the
    /// install ignores them. Computed at the CLI layer from
    /// `config.skip_runtimes || --no-runtime`.
    pub skip_runtimes: bool,

    /// `nodeLinker` value to honor for *this* invocation. Threaded
    /// from the package manager's `Install` caller (which has already
    /// applied any `--node-linker` CLI override on top of
    /// [`pnpm_config::Config::node_linker`]).
    ///
    /// Under [`NodeLinker::Hoisted`] the install pipeline routes
    /// through [`crate::lockfile_to_hoisted_dep_graph`] +
    /// [`crate::link_hoisted_modules()`] instead of the isolated
    /// linker's [`crate::SymlinkDirectDependencies`] +
    /// [`crate::LinkVirtualStoreBins`] + [`crate::get_hoisted_dependencies`]
    /// chain, matching the `nodeLinker === 'hoisted'` branch in
    /// `headlessInstall`.
    ///
    /// [`NodeLinker::Pnp`] shares the isolated virtual-store materialization,
    /// then replaces importer dependency links with the project-level `PnP`
    /// loader during the link phase.
    pub node_linker: NodeLinker,
}

#[derive(Clone, Copy)]
pub struct PriorMaterialization<'a> {
    /// Forced-rebuild selection threaded from `pacquet rebuild` /
    /// `approve-builds`; `None` for a normal install. Forwarded to
    /// [`crate::run_build_phase`]'s [`crate::BuildPhaseInputs`]. See
    /// [`crate::RebuildOptions`].
    pub rebuild: Option<&'a crate::RebuildOptions>,
    /// `hoistedDependencies` recorded by the previous install's
    /// `.modules.yaml`, for [`crate::PruneStaleModules`]'s orphan
    /// hoist-link cleanup. `None` on a first install or when the file
    /// couldn't be fully parsed.
    pub hoisted_dependencies: Option<&'a crate::HoistedDependencies>,
    /// `hoistedLocations` recorded by the previous install's
    /// `.modules.yaml`, for the hoisted linker's already-in-place check.
    /// `None` on a first install or when the file couldn't be fully
    /// parsed.
    pub hoisted_locations: Option<&'a crate::HoistedLocations>,
    /// See [`crate::PriorLinkState::previously_skipped`].
    pub previously_skipped: &'a crate::SkippedSnapshots,
    /// `allowBuilds` changed since the previous install: a build it
    /// ignored may now be allowed, or one it ran may no longer be. The
    /// hoisted linker then hands every package to the build phase, present
    /// or not. See [`crate::PriorHoistedState::build_present_packages`].
    pub allow_builds_changed: bool,
    /// See [`crate::PriorHoistedState::unbuilt_builds`].
    pub unbuilt_builds: &'a crate::UnbuiltBuilds,
    /// See [`crate::PruneStaleModules::prune_orphans`].
    pub prune_orphans: bool,
    /// Relink the bins of every slot the lockfile records, not only of the
    /// slots this install materializes. A tree that moved with its project
    /// needs it, because its other slots may hold bins naming where it was.
    pub relink_every_slot_bin: bool,
}

#[derive(Default)]
pub struct FrozenInstallSeed<'a> {
    /// A host detection the install entry point spawned right after
    /// the wanted lockfile parsed (see
    /// [`crate::materialization_plan::HostDetection::spawn`]), so its
    /// `node --version` overlaps the planning here. Must have been
    /// spawned with this install's `node_version` /
    /// `supported_architectures` / `engine_strict`. `None` runs the
    /// detection here.
    pub early_host_detection: Option<crate::materialization_plan::HostDetection>,

    /// Effective `nodeVersion`: an explicit config value, otherwise the
    /// minimum version declared by the root manifest's runtime engine.
    pub node_version: Option<String>,
    pub skipped: Option<Vec<String>>,
    /// When set, replaces the local `resolution_verifiers` fan-out as the
    /// trust verdict — used by the pnpr client to delegate verification to
    /// the server's `/-/pnpr/v0/verify-lockfile` while the fetch runs locally. The
    /// same concurrent sequencing and build gate apply.
    pub lockfile_verification_override: Option<LockfileVerificationOverride<'a>>,
}

impl<'a> PriorMaterialization<'a> {
    pub(crate) fn link_state(self) -> crate::PriorLinkState<'a> {
        crate::PriorLinkState {
            prune_orphans: self.prune_orphans,
            hoisted_dependencies: self.hoisted_dependencies,
            hoisted_locations: self.hoisted_locations,
            build_present_packages: self.rebuild.is_some() || self.allow_builds_changed,
            unbuilt_builds: self.unbuilt_builds,
            previously_skipped: self.previously_skipped,
        }
    }
}
