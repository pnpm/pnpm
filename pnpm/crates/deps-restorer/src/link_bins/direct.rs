use super::{build_has_bin_set, pkg_dir_under};
use crate::PackageManifests;
use pnpm_cmd_shim::{
    Host, LinkBinsError, LinkBinsOptions, PackageBinSource, ShimTargetCache,
    link_bins_of_packages_cached,
};
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::{PackageKey, PackageMetadata};
use pnpm_package_manifest::{find_parent_publish_manifest, parse_manifest_bytes};
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The config-derived options for every bin an install links.
///
/// `extra_node_paths` is pnpm's `getExtraNodePaths`: under the isolated
/// linker with a hoist pattern, every shim written by the install
/// carries the hidden hoisted modules dir
/// (`<virtual-store-dir>/node_modules`) on `NODE_PATH`, unless
/// `extendNodePath: false`. Everything else — the hoisted linker, a
/// disabled hoist pass — gets no `NODE_PATH` at all.
#[must_use]
pub fn shim_link_options(config: &Config, node_linker: NodeLinker) -> LinkBinsOptions {
    let has_hoist_pattern = config.hoist_pattern
        .as_ref()
        .is_some_and(|patterns| !patterns.is_empty());
    let extra_node_paths = if config.extend_node_path
        && matches!(node_linker, NodeLinker::Isolated)
        && has_hoist_pattern
    {
        vec![
            config.virtual_store_dir
                .join("node_modules")
                .to_string_lossy()
                .into_owned(),
        ]
    } else {
        Vec::new()
    };
    LinkBinsOptions {
        extra_node_paths,
        prefer_symlinked_executables: config.prefer_symlinked_executables.unwrap_or(false),
        preserve_bin_name: cfg!(unix) && config.preserve_bin_name,
        relocatable_root: config.modules_dir_anchor().map(Path::to_path_buf),
        project_modules_dir_name: (config.extend_node_path
            && config.modules_dir_name() != "node_modules")
            .then(|| config.modules_dir_name().to_owned()),
        installed_modules_dir: (config.modules_dir_name() != "node_modules").then(|| {
            config.modules_dir.clone()
        }),
    }
}
/// Read the `package.json` of every direct dependency under `modules_dir`
/// and link its bins into `<modules_dir>/.bin`.
///
/// `dep_names` is the list of direct-dependency keys as they appear in
/// `package.json`, the same names already symlinked under `<modules_dir>/`
/// by [`crate::SymlinkDirectDependencies`]. We resolve `package.json` via
/// the symlink (`fs::read` follows it transparently) so the read targets
/// the real package contents in the virtual store.
///
/// Driven on rayon because each location's read+parse is independent.
pub fn link_direct_dep_bins(
    modules_dir: &Path,
    dep_names: &[String],
    link_options: &LinkBinsOptions,
) -> Result<(), LinkBinsError> {
    let deps: Vec<(&str, Option<&Path>)> = dep_names
        .iter()
        .map(|name| (name.as_str(), None))
        .collect();
    link_named_dep_bins(modules_dir, &deps, link_options, false).map(|_| ())
}
/// [`link_direct_dep_bins`] for a `.bin` under the hoisted linker that is
/// linked again after the builds. Every bin whose target is missing is held
/// back until then, so a dependency's lifecycle scripts cannot reach a shim of
/// a file they have yet to create. See [`PackageBinSource::build_pending`].
///
/// Returns whether a bin was held back.
pub fn link_direct_dep_bins_before_builds(
    modules_dir: &Path,
    dep_names: &[String],
    link_options: &LinkBinsOptions,
) -> Result<bool, LinkBinsError> {
    let deps: Vec<(&str, Option<&Path>)> = dep_names
        .iter()
        .map(|name| (name.as_str(), None))
        .collect();
    link_named_dep_bins(modules_dir, &deps, link_options, true)
}
/// Resolve the hoist pass's `(alias, snapshot key)` bin list into the
/// `(alias, slot package dir)` pairs [`link_direct_dep_bins_resolved`]
/// takes — the same slot derivation `symlink_hoisted_dependencies`
/// used to create the aliases.
#[must_use]
pub fn resolve_hoisted_bin_deps(
    layout: &crate::VirtualStoreLayout,
    aliases: &[(String, PackageKey)],
) -> Vec<(String, PathBuf)> {
    aliases
        .iter()
        .map(|(alias, key)| {
            (alias.clone(), pkg_dir_under(&layout.slot_dir(key).join("node_modules"), &key.name))
        })
        .collect()
}
/// [`link_direct_dep_bins`] for callers that also know each symlink's
/// destination (the virtual-store slot the alias points at): threading
/// it through as [`PackageBinSource::resolved_location`] keeps the
/// shim `NODE_PATH` derivation syscall-free.
pub fn link_direct_dep_bins_resolved(
    modules_dir: &Path,
    deps: &[(String, PathBuf)],
    link_options: &LinkBinsOptions,
) -> Result<(), LinkBinsError> {
    let deps: Vec<(&str, Option<&Path>)> = deps
        .iter()
        .map(|(name, target)| (name.as_str(), Some(target.as_path())))
        .collect();
    link_named_dep_bins(modules_dir, &deps, link_options, false).map(|_| ())
}
/// One direct dep of [`link_direct_dep_bins_prefetched`]'s importer:
/// the alias under `node_modules/`, the symlink's destination, and the
/// resolved snapshot key (`None` for `link:` workspace siblings, which
/// have no lockfile row).
pub type PrefetchedDepBin = (String, PathBuf, Option<PackageKey>);
/// The lockfile- and store-index-derived facts that let an importer's
/// bin pass skip per-dep disk IO, shared across every importer of one
/// symlink pass.
pub struct PrefetchedBinLookup<'a> {
    /// Lockfile `hasBin` gate, from [`build_has_bin_set`]: `Some(set)`
    /// is authoritative (a dep whose metadata key is absent declares no
    /// bin and is skipped without IO), `None` means the lockfile had no
    /// `packages:` section and every dep must be probed.
    has_bin: Option<HashSet<PackageKey>>,
    /// Parsed manifests recovered from the store-index prefetch. A hit
    /// replaces the per-importer `package.json` read; a miss falls back
    /// to disk.
    package_manifests: Option<&'a PackageManifests>,
    /// Per-snapshot `requiresBuild` flags from the same prefetch. The
    /// no-IO path is only sound for a snapshot known not to run build
    /// scripts: a persisted global-virtual-store slot that was built may
    /// carry a `package.json` its scripts rewrote, and only the disk
    /// read sees those bins. A missing entry counts as "may build".
    requires_build: Option<&'a crate::RequiresBuildBySnapshot>,
    /// Per-target shim probe memo, shared so each virtual-store bin
    /// file's shebang read and executable-bit fix-up run once per pass
    /// rather than once per importer.
    shim_cache: ShimTargetCache,
    /// See [`Self::with_scheduled_builds`].
    scheduled_builds: Option<&'a crate::build_modules::ScheduledBuilds<'a>>,
}
impl<'a> PrefetchedBinLookup<'a> {
    /// Record the builds that run after this pass. The bins of a package
    /// among them are marked [`PackageBinSource::build_pending`], so a bin
    /// its scripts have yet to create stays off the importer's `.bin` until
    /// the post-build relink.
    #[must_use]
    pub fn with_scheduled_builds(
        mut self,
        scheduled_builds: Option<&'a crate::build_modules::ScheduledBuilds<'a>>,
    ) -> Self {
        self.scheduled_builds = scheduled_builds;
        self
    }

    fn is_build_pending(&self, snapshot_key: Option<&PackageKey>) -> bool {
        let (Some(scheduled), Some(key)) = (self.scheduled_builds, snapshot_key) else {
            return false;
        };
        scheduled.includes(key)
    }

    #[must_use]
    pub fn new(
        packages: Option<&HashMap<PackageKey, PackageMetadata>>,
        package_manifests: Option<&'a PackageManifests>,
        requires_build: Option<&'a crate::RequiresBuildBySnapshot>,
    ) -> Self {
        PrefetchedBinLookup {
            has_bin: build_has_bin_set(packages),
            package_manifests,
            requires_build,
            shim_cache: ShimTargetCache::default(),
            scheduled_builds: None,
        }
    }
}
/// [`link_direct_dep_bins_resolved`] with the per-dep `package.json`
/// read replaced by lockfile metadata for snapshots the prefetch
/// proved build-free: those whose lockfile row says `hasBin: false`
/// are skipped without touching the filesystem, and those with bins
/// take their manifest from the store-index prefetch. Deps outside the
/// lookup's reach (`link:` siblings, snapshots that may run build
/// scripts, prefetch misses, a lockfile without a `packages:` section)
/// keep the `NotFound`-tolerant disk read.
pub fn link_direct_dep_bins_prefetched(
    modules_dir: &Path,
    deps: &[PrefetchedDepBin],
    lookup: &PrefetchedBinLookup<'_>,
    link_options: &LinkBinsOptions,
) -> Result<(), LinkBinsError> {
    let bin_sources: Vec<PackageBinSource> = deps
        .par_iter()
        .filter_map(|(name, target, snapshot_key)| {
            let source = match prefetched_bin_source(
                modules_dir,
                name,
                target,
                snapshot_key.as_ref(),
                lookup,
            ) {
                PrefetchedBin::NoBins => None,
                PrefetchedBin::Source(source) => Some(Ok(source)),
                PrefetchedBin::ReadFromDisk => read_dep_bin_source(modules_dir, name, target),
            };
            let build_pending = lookup.is_build_pending(snapshot_key.as_ref());
            source.map(|source| source.map(|source| source.with_build_pending(build_pending)))
        })
        .collect::<Result<_, _>>()?;
    if bin_sources.is_empty() {
        return Ok(());
    }
    link_bins_of_packages_cached::<Host>(
        &bin_sources,
        &modules_dir.join(".bin"),
        link_options,
        &lookup.shim_cache,
    )
    .map(|_| ())
}
/// What the prefetched facts say about one direct dependency's bins.
pub(super) enum PrefetchedBin {
    /// The lockfile row says the package declares no bin.
    NoBins,
    /// Built from the prefetched manifest, no disk read needed.
    Source(PackageBinSource),
    /// Outside the prefetch's reach; the on-disk manifest decides.
    ReadFromDisk,
}
/// The no-IO path only covers snapshots the prefetch proved build-free
/// (see [`PrefetchedBinLookup::requires_build`]); anything else reads
/// the on-disk manifest, which reflects whatever a build script did to
/// it.
pub(super) fn prefetched_bin_source(
    modules_dir: &Path,
    name: &str,
    target: &Path,
    snapshot_key: Option<&PackageKey>,
    lookup: &PrefetchedBinLookup<'_>,
) -> PrefetchedBin {
    let Some(snapshot_key) = snapshot_key else {
        return PrefetchedBin::ReadFromDisk;
    };
    if lookup.requires_build.and_then(|flags| flags.get(snapshot_key)) != Some(&false) {
        return PrefetchedBin::ReadFromDisk;
    }
    let metadata_key = snapshot_key.without_peer();
    if let Some(has_bin) = &lookup.has_bin
        && !has_bin.contains(&metadata_key)
    {
        return PrefetchedBin::NoBins;
    }
    let Some(manifest) =
        lookup.package_manifests.and_then(|manifests| manifests.get(&metadata_key))
    else {
        return PrefetchedBin::ReadFromDisk;
    };
    PrefetchedBin::Source(
        PackageBinSource::new(modules_dir.join(name), Arc::clone(manifest))
            .with_resolved_location(target.to_path_buf()),
    )
}
fn read_manifest_at(manifest_path: &Path) -> Result<Option<serde_json::Value>, LinkBinsError> {
    let bytes = match fs::read(manifest_path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(LinkBinsError::ReadManifest { path: manifest_path.to_path_buf(), error });
        }
    };
    parse_manifest_bytes(&bytes)
        .map(Some)
        .map_err(|error| LinkBinsError::ParseManifest { path: manifest_path.to_path_buf(), error })
}

/// The disk-read arm of [`link_direct_dep_bins_prefetched`], with the
/// same `NotFound`-tolerant / other-IO-fatal policy as
/// [`link_direct_dep_bins`].
pub(super) fn read_dep_bin_source(
    modules_dir: &Path,
    name: &str,
    target: &Path,
) -> Option<Result<PackageBinSource, LinkBinsError>> {
    read_dep_manifest(modules_dir, name, Some(target))
        .map(|result| {
            result.map(|(location, manifest)| {
                PackageBinSource::new(location, Arc::new(manifest))
                    .with_resolved_location(target.to_path_buf())
            })
        })
}
/// Reads `<modules_dir>/<name>/package.json`. A dependency linked to a
/// `publishConfig.directory` that has no manifest of its own falls back
/// to the manifest of the project that declares that directory, found
/// through `target`.
fn read_dep_manifest(
    modules_dir: &Path,
    name: &str,
    target: Option<&Path>,
) -> Option<Result<(PathBuf, serde_json::Value), LinkBinsError>> {
    let location = modules_dir.join(name);
    let manifest = match read_manifest_at(&location.join("package.json")) {
        Ok(Some(manifest)) => manifest,
        Ok(None) => match find_parent_publish_manifest(target?)
            .map_err(LinkBinsError::ReadProjectManifest)
        {
            Ok(Some(manifest)) => manifest,
            Ok(None) => return None,
            Err(err) => return Some(Err(err)),
        },
        Err(err) => return Some(Err(err)),
    };
    Some(Ok((location, manifest)))
}
pub(super) fn link_named_dep_bins(
    modules_dir: &Path,
    deps: &[(&str, Option<&Path>)],
    link_options: &LinkBinsOptions,
    build_pending: bool,
) -> Result<bool, LinkBinsError> {
    // Swallow only `NotFound`: a direct-dep symlink target can
    // legitimately be missing right after a partial pacquet run, or
    // be an in-progress install. Every other IO error (permission
    // denied, EIO, etc.) and every JSON parse error must surface as
    // `LinkBinsError::{ReadManifest, ParseManifest}` so the failure
    // is diagnosable rather than hiding behind a missing `.bin`
    // entry. Matches the read-side error policy in
    // `pnpm_cmd_shim::link_bins`.
    let bin_sources: Vec<PackageBinSource> = deps
        .par_iter()
        .filter_map(|(name, resolved)| {
            let (location, manifest) = match read_dep_manifest(modules_dir, name, *resolved)? {
                Ok(found) => found,
                Err(err) => return Some(Err(err)),
            };
            let mut source = PackageBinSource::new(location, Arc::new(manifest))
                .with_build_pending(build_pending);
            if let Some(resolved) = resolved {
                source = source.with_resolved_location(resolved.to_path_buf());
            }
            Some(Ok(source))
        })
        .collect::<Result<_, _>>()?;
    if bin_sources.is_empty() {
        return Ok(false);
    }
    link_bins_of_packages_cached::<Host>(
        &bin_sources,
        &modules_dir.join(".bin"),
        link_options,
        &ShimTargetCache::default(),
    )
}
