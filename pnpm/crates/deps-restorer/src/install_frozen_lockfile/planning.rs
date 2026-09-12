use super::{InstallFrozenLockfileError, LockfileVerificationOverride};
use crate::{
    AllowBuildPolicy, CreateVirtualStore, CreateVirtualStoreOutput, SkippedSnapshots,
    VirtualStoreLayout, any_installability_constraint,
};
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::{Lockfile, LockfileEntries, PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_modules_yaml::{Host, IncludedDependencies, read_modules_manifest};
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_resolver_base::ResolutionVerifier;
use pnpm_store_dir::StoreIndexWriter;
use pnpm_tarball::{MemCache, SharedReportedProgressKeys};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

/// What [`InstallFrozenLockfile::plan_materialization`](crate::InstallFrozenLockfile::plan_materialization) decides before
/// the on-disk phases run. Owned by `run` for the whole install; the
/// phases borrow the parts they read.
pub(super) struct MaterializationPlan<'p> {
    pub(super) link_options: pnpm_cmd_shim::LinkBinsOptions,
    pub(super) host: HostPlan,
    pub(super) deferred_engine_name: Option<crate::materialization_plan::DeferredEngineName>,
    pub(super) layout: VirtualStoreLayout,
    /// Borrows the allow-builds policy `run` owns.
    pub(super) dir_clone_cache: Option<crate::DirCloneCache<'p>>,
    pub(super) cas_prefetch: crate::create_virtual_store::CasPrefetch,
    pub(super) git_source_cache: pnpm_git_fetcher::GitSourceCache,
}
/// The install's borrowed inputs, as one `Copy` value a phase's future
/// can capture. Only the `Copy` fields of [`InstallFrozenLockfile`](crate::InstallFrozenLockfile) are
/// here; the owned ones go through [`InstallFrozenLockfile::take_owned`](crate::InstallFrozenLockfile::take_owned).
#[derive(Clone, Copy)]
pub(super) struct FrozenInputs<'a> {
    pub(super) http_client: &'a ThrottledClient,
    pub(super) config: &'static Config,
    pub(super) pnpmfile_hook: Option<&'a Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub(super) lockfile: &'a Lockfile,
    pub(super) resolution_verifiers: &'a [Arc<dyn ResolutionVerifier>],
    pub(super) lockfile_path: Option<&'a Path>,
    pub(super) current_lockfile: Option<&'a Lockfile>,
    pub(super) current_entries: LockfileEntries<'a>,
    pub(super) dependency_groups: &'a [DependencyGroup],
    pub(super) project_manifests: &'a [(PathBuf, &'a pnpm_package_manifest::PackageManifest)],
    pub(super) package_map_project_manifests:
        &'a [(PathBuf, &'a pnpm_package_manifest::PackageManifest)],
    pub(super) workspace_root: &'a Path,
    pub(super) requester: &'a str,
    pub(super) supported_architectures:
        Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    pub(super) skip_runtimes: bool,
    pub(super) node_linker: NodeLinker,
    pub(super) tarball_mem_cache: Option<&'a Arc<MemCache>>,
    pub(super) rebuild: Option<&'a crate::RebuildOptions>,
    pub(super) prior_hoisted_dependencies: Option<&'a crate::HoistedDependencies>,
    pub(super) prior_hoisted_locations: Option<&'a crate::HoistedLocations>,
    pub(super) allow_builds_changed: bool,
    pub(super) prior_unbuilt_builds: &'a crate::UnbuiltBuilds,
    pub(super) prune_orphans: bool,
    pub(super) planned_canonical_fetches:
        Option<&'a pnpm_resolving_resolver_base::PlannedCanonicalFetches>,
}
impl<'a> FrozenInputs<'a> {
    /// Which dependency groups this install includes, in the shape the
    /// skip set and the sidecars record.
    pub(super) fn included(&self) -> IncludedDependencies {
        IncludedDependencies {
            dependencies: self.dependency_groups.contains(&DependencyGroup::Prod),
            dev_dependencies: self.dependency_groups.contains(&DependencyGroup::Dev),
            optional_dependencies: self.dependency_groups.contains(&DependencyGroup::Optional),
        }
    }

    // Declared projects may live outside the lockfile directory, unlike untrusted importer keys.
    pub(super) fn importer_sets(&self) -> (HashSet<String>, HashSet<String>) {
        let install = self;
        // Importer ids backed by the install's own declared projects.
        // These may legitimately live outside the lockfile dir (Bit's
        // capsule installs), so they bypass the malformed-lockfile
        // importer-key rejection.
        let importer_id =
            |(project_dir, _): &(PathBuf, &pnpm_package_manifest::PackageManifest)| {
                pnpm_workspace::importer_id_from_root_dir(install.workspace_root, project_dir)
            };
        let trusted_importer_ids: std::collections::HashSet<String> =
            install.project_manifests.iter().map(importer_id).collect();
        let root_component_importers: std::collections::HashSet<String> = install
            .project_manifests
            .iter()
            .filter(|(_, manifest)| {
                manifest.install_config_hoisting_limits() == Some(crate::HOISTING_LIMITS_WORKSPACES)
            })
            .map(importer_id)
            .collect();
        (trusted_importer_ids, root_component_importers)
    }

    pub(super) fn virtual_store<'p>(
        self,
        ctx: &'p crate::InstallContext<'p>,
        (cas_prefetch, dir_clone_cache): (
            crate::create_virtual_store::CasPrefetch,
            Option<&'p crate::DirCloneCache<'p>>,
        ),
        store_index_writer: &'p Arc<StoreIndexWriter>,
        skipped: &'p SkippedSnapshots,
        progress_reported: &'p SharedReportedProgressKeys,
        custom_fetcher_session: Option<&'p Arc<crate::CustomFetcherSession>>,
    ) -> CreateVirtualStore<'p>
    where
        'a: 'p,
    {
        let install = self;
        CreateVirtualStore {
            ctx,
            http_client: install.http_client,
            entries: install.entries(),
            current_entries: install.current_entries,
            store_index_writer,
            store_context: None,
            cas_prefetch: Some(cas_prefetch),
            skipped,
            include_optional_dependencies: install.included().optional_dependencies,
            supported_architectures: install.supported_architectures,
            dir_clone_cache,
            progress_reported,
            tarball_mem_cache: install.tarball_mem_cache,
            custom_fetcher_session,
            planned_canonical_fetches: install.planned_canonical_fetches,
            #[cfg(test)]
            link_concurrency_probe: None,
        }
    }

    // Validate path containment before cache construction can write to the store, even with trustLockfile.
    pub(super) fn verified_layout(
        &self,
        allow_build_policy: &AllowBuildPolicy,
        engine_name: Option<&str>,
    ) -> Result<VirtualStoreLayout, InstallFrozenLockfileError> {
        let install = self;
        let LockfileEntries { packages, snapshots } = install.entries();
        // Build the install-scoped slot-directory layout. When
        // `enable_global_virtual_store` is on the layout precomputes
        // each snapshot's `<scope>/<name>/<version>/<hash>` suffix
        // from [`pnpm_graph_hasher::calc_graph_node_hash`];
        // otherwise it falls through to the legacy
        // `to_virtual_store_name`-shaped flat name on every
        // `slot_dir` call. Either way every downstream consumer
        // (warm batch, cold batch, direct-dep symlinks, bin linker,
        // build module) routes through this one lookup.
        let phase_start = std::time::Instant::now();
        let layout = VirtualStoreLayout::new_cached(
            install.config,
            engine_name,
            snapshots,
            packages,
            Some(allow_build_policy),
            Some(install.workspace_root),
        );
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "virtual_store_layout",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "phase complete",
        );

        // Reject a lockfile whose dependency names, aliases, or
        // virtual-store slots would escape the project or the store once
        // joined into a filesystem path. Runs before any materialization
        // and before the warm-install skip filter, and unconditionally —
        // so it is not bypassed by `trustLockfile`, which disables the
        // resolution-verification fan-out where the offline name check
        // would otherwise run. The slot-containment half needs the
        // install-time `layout`, so it can't live in the verifier crate.
        pnpm_lockfile_verification::verify_lockfile_dependency_names(install.lockfile)
            .map_err(InstallFrozenLockfileError::LockfileVerification)?;
        crate::validate_virtual_store_slot_containment(snapshots, &layout)
            .map_err(InstallFrozenLockfileError::LockfileVerification)?;

        Ok(layout)
    }

    // Called only after verified_layout rejects unsafe dependency names and slot paths.
    pub(super) fn dir_clone_cache<'p>(
        &self,
        allow_build_policy: &'p AllowBuildPolicy,
        engine: crate::dir_clone_cache::EngineNameSource,
    ) -> Option<crate::DirCloneCache<'p>>
    where
        'a: 'p,
    {
        let install = self;
        let LockfileEntries { packages, snapshots } = install.entries();
        crate::DirCloneCache::build(
            install.config,
            install.node_linker,
            engine,
            snapshots,
            packages,
            Some(allow_build_policy),
            Some(install.workspace_root),
        )
    }

    pub(super) fn entries(&self) -> LockfileEntries<'a> {
        LockfileEntries::from(self.lockfile)
    }
}
/// The inputs `run` consumes rather than borrows. See
/// [`InstallFrozenLockfile::take_owned`](crate::InstallFrozenLockfile::take_owned).
pub(super) struct OwnedInputs<'a> {
    pub(super) early_host_detection: Option<crate::materialization_plan::HostDetection>,
    pub(super) node_version: Option<String>,
    pub(super) seed_skipped: Option<Vec<String>>,
    pub(super) verification_override: Option<LockfileVerificationOverride<'a>>,
}
/// What [`InstallFrozenLockfile::build`](crate::InstallFrozenLockfile::build) reads from the phases before it.
pub(super) struct BuildInputs<'p> {
    pub(super) fetched: &'p CreateVirtualStoreOutput,
    pub(super) linked: &'p crate::linking::LinkPhaseOutput,
    pub(super) skipped: &'p SkippedSnapshots,
    pub(super) store_index_writer: &'p Arc<StoreIndexWriter>,
    pub(super) engine_name: Option<String>,
    pub(super) deferred_engine_name: Option<crate::materialization_plan::DeferredEngineName>,
}
/// What [`InstallFrozenLockfile::link`](crate::InstallFrozenLockfile::link) reads from the phases before it.
pub(super) struct LinkInputs<'p> {
    pub(super) fetched: &'p CreateVirtualStoreOutput,
    /// Taken out of `fetched`: the hoisted linker consumes it.
    pub(super) cas_paths_by_pkg_id: Option<crate::CasPathsByPkgId>,
    pub(super) host_node: Option<&'p crate::materialization_plan::HostNode>,
}
/// What [`InstallFrozenLockfile::fetch`](crate::InstallFrozenLockfile::fetch) needs beyond the install's own
/// inputs: the plan's store-side half and the state the phases before
/// it produced.
pub(super) struct FetchInputs<'p> {
    pub(super) cas_prefetch: crate::create_virtual_store::CasPrefetch,
    pub(super) dir_clone_cache: Option<&'p crate::DirCloneCache<'p>>,
    pub(super) store_index_writer: &'p Arc<StoreIndexWriter>,
    pub(super) skipped: &'p SkippedSnapshots,
    pub(super) verification_override: Option<LockfileVerificationOverride<'p>>,
}
/// The engine name the build cache keys on, once the host is known:
/// what the plan pinned, or else what the host says, delivered to the
/// slot the directory-clone cache is waiting on.
pub(super) fn settle_engine_name(
    pending_slot: Option<&std::sync::OnceLock<Option<String>>>,
    pinned: Option<String>,
    host_node: Option<&crate::materialization_plan::HostNode>,
) -> Option<String> {
    let Some(slot) = pending_slot else {
        return pinned;
    };
    let name = host_node.and_then(crate::materialization_plan::engine_name_from_host);
    let _ = slot.set(name.clone());
    name
}
/// The half of the plan the host probe decides, consumed by
/// [`InstallFrozenLockfile::settle_skip_set`](crate::InstallFrozenLockfile::settle_skip_set).
pub(super) struct HostPlan {
    pub(super) host_detection: crate::materialization_plan::HostDetection,
    /// Known outright from a runtime pin, or `None` until the probe
    /// resolves and fills `pending_host_engine_slot`.
    pub(super) engine_name: Option<String>,
    pub(super) pending_host_engine_slot:
        Option<std::sync::Arc<std::sync::OnceLock<Option<String>>>>,
    pub(super) needs_installability_check: bool,
}
/// What [`InstallFrozenLockfile::settle_skip_set`](crate::InstallFrozenLockfile::settle_skip_set) leaves for the phases
/// after it.
pub(super) struct SkipSetPlan {
    pub(super) skipped: SkippedSnapshots,
    pub(super) engine_name: Option<String>,
    pub(super) host_node: Option<crate::materialization_plan::HostNode>,
}
/// Seed the skip set from the previous install's
/// `.modules.yaml.skipped`. Each entry there is a depPath string a
/// previous run wrote out; treating it as already-skipped short-circuits
/// its per-snapshot installability check and keeps
/// `pnpm:skipped-optional-dependency` from being re-emitted for a
/// known-skipped package.
///
/// A read error (corrupt yaml, permissions) degrades to an empty seed —
/// `.modules.yaml` is a cache artifact, not an authoritative source.
pub(super) fn seed_skip_set(
    config: &pnpm_config::Config,
    seed_skipped: Option<Vec<String>>,
) -> SkippedSnapshots {
    // `--force` installs previously-skipped snapshots too, so the
    // recorded skip set must not survive into this install.
    if config.force {
        return SkippedSnapshots::new();
    }
    if let Some(skipped) = seed_skipped {
        return SkippedSnapshots::from_strings(&skipped);
    }
    match read_modules_manifest::<Host>(&config.modules_dir) {
        Ok(Some(manifest)) => SkippedSnapshots::from_strings(&manifest.skipped),
        Ok(None) => SkippedSnapshots::new(),
        Err(error) => {
            tracing::warn!(
                target: "pacquet::install",
                ?error,
                "failed to read .modules.yaml for skipped seed; starting from empty",
            );
            SkippedSnapshots::new()
        }
    }
}
/// Detecting the host is what costs a `node --version`, so it is skipped
/// entirely for the common constraint-free lockfile — otherwise the
/// probe serializes against the extraction that dominates a cold
/// install.
///
/// `any_installability_constraint` short-circuits on `packages` alone,
/// so the empty-snapshots guard is load-bearing: without it a lockfile
/// with constrained metadata but no snapshots would pay for a
/// `node --version` it has nothing to check.
pub(super) fn needs_installability_check(
    config: &pnpm_config::Config,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> bool {
    !config.force
        && match (snapshots, packages) {
            (Some(snaps), Some(pkgs)) if !snaps.is_empty() => {
                any_installability_constraint(snaps, pkgs)
            }
            _ => false,
        }
}
pub(super) struct HostDetectionInputs<'a> {
    pub(super) config: &'a pnpm_config::Config,
    pub(super) early_host_detection: Option<crate::materialization_plan::HostDetection>,
    pub(super) node_version: Option<String>,
    pub(super) supported_architectures:
        Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    pub(super) needs_installability_check: bool,
}
/// The global-virtual-store layout needs the engine name — and so the
/// host — synchronously, but otherwise the detection stays pending and
/// is only resolved once the skip-set computation needs the host, so the
/// probe runs under the store-side warm-cache prefetch instead of
/// serializing before it. A detection the install entry point already
/// spawned (before the lockfile parse) is adopted so its head start
/// counts; an early detection a constraint-free lockfile turns out not
/// to need is dropped — the probe finishes in the background and its
/// result goes unused.
pub(super) async fn detect_host(
    inputs: HostDetectionInputs<'_>,
) -> crate::materialization_plan::HostDetection {
    let HostDetectionInputs {
        config,
        early_host_detection,
        node_version,
        supported_architectures,
        needs_installability_check,
    } = inputs;
    if !needs_installability_check {
        return crate::materialization_plan::HostDetection::Resolved(None);
    }
    if !config.enable_global_virtual_store {
        return early_host_detection.unwrap_or_else(|| {
            crate::materialization_plan::HostDetection::spawn(
                config.engine_strict,
                node_version,
                supported_architectures.cloned(),
            )
        });
    }
    let host = match early_host_detection {
        Some(detection) => detection.resolve().await,
        None => {
            crate::materialization_plan::detect_installability_host(
                true,
                config.engine_strict,
                node_version,
                supported_architectures,
            )
            .await
        }
    };
    crate::materialization_plan::HostDetection::Resolved(host)
}
/// How this install obtains the engine name.
pub(super) struct EngineNamePlan {
    pub(super) name: Option<String>,
    /// Set when the name comes from a `node --version` probe deferred
    /// into the blocking pool, awaited right before `BuildModules`.
    pub(super) deferred: Option<crate::materialization_plan::DeferredEngineName>,
    /// Set when the name comes from a host detection that is still
    /// pending; the directory-clone cache reads it through this slot.
    pub(super) pending_slot: Option<std::sync::Arc<std::sync::OnceLock<Option<String>>>>,
}
impl EngineNamePlan {
    /// Where the directory-clone cache reads the engine name from: the
    /// slot a pending host probe will fill, the deferred probe's shared
    /// handle, or the name already known.
    pub(super) fn source(&self) -> crate::EngineNameSource {
        match (&self.pending_slot, &self.deferred) {
            (Some(slot), _) => crate::EngineNameSource::Pending(std::sync::Arc::clone(slot)),
            (None, Some(deferred)) => crate::EngineNameSource::Pending(deferred.shared()),
            (None, None) => crate::EngineNameSource::Ready(self.name.clone()),
        }
    }
}
/// `engine_name` feeds two sites:
///
/// - The GVS-aware [`VirtualStoreLayout`] needs it *before*
///   `CreateVirtualStore::run` to produce per-snapshot
///   `<scope>/<name>/<version>/<hash>` suffixes under
///   `<store_dir>/links`. Only matters when GVS is on.
/// - `BuildModules` uses it for the side-effects-cache key prefix. Read
///   by both the cache read-gate and the write-gate; when `None`, both
///   gates close and the cache is bypassed.
///
/// An `engines.runtime` / `devEngines.runtime` pin that reached the
/// lockfile wins: the runtime resolver writes the chosen Node as a
/// `node@runtime:<version>` snapshot, and anchoring the GVS hash and the
/// side-effects-cache key prefix to that pinned Node is what keeps
/// pinned and non-pinned installs on one host from splitting the shared
/// store. Otherwise the name is derived from the host — synchronously
/// when it is already detected, through
/// [`EngineNamePlan::pending_slot`] while a detection is in flight, and
/// through [`EngineNamePlan::deferred`] when no detection was needed at
/// all. A synthetic fallback host (`detected: false`) yields `None` so a
/// bogus `99999.0.0`-derived key can't poison either the cache or the
/// GVS hash.
pub(super) async fn plan_engine_name(
    config: &pnpm_config::Config,
    host_detection: &crate::materialization_plan::HostDetection,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
) -> EngineNamePlan {
    let host = match host_detection {
        crate::materialization_plan::HostDetection::Pending { .. } => {
            let name = crate::materialization_plan::engine_name_from_runtime_pin(snapshots);
            if name.is_some() {
                return EngineNamePlan { name, deferred: None, pending_slot: None };
            }
            return EngineNamePlan {
                name: None,
                deferred: None,
                pending_slot: Some(std::sync::Arc::new(std::sync::OnceLock::new())),
            };
        }
        crate::materialization_plan::HostDetection::Resolved(host) => host,
    };
    let host_node = host.as_ref().map(crate::materialization_plan::HostNode::from);
    let (name, deferred) = crate::materialization_plan::resolve_engine_name(
        config.enable_global_virtual_store,
        snapshots,
        host_node.as_ref(),
    )
    .await;
    EngineNamePlan { name, deferred, pending_slot: None }
}
