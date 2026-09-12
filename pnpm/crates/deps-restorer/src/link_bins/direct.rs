use super::{build_has_bin_set, pkg_dir_under, read_package};
use crate::PackageManifests;
use pnpm_cmd_shim::{
    BinOrigin, Host, LinkBinsError, LinkBinsOptions, PackageBinSource, ShimTargetCache,
    collect_packages_in_modules_dir, link_bins_of_packages, link_bins_of_packages_cached,
};
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::{PackageKey, PackageMetadata};
use pnpm_package_manifest::parse_manifest_bytes;
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
    let has_hoist_pattern =
        config.hoist_pattern.as_ref().is_some_and(|patterns| !patterns.is_empty());
    let extra_node_paths = if config.extend_node_path
        && matches!(node_linker, NodeLinker::Isolated)
        && has_hoist_pattern
    {
        vec![config.virtual_store_dir.join("node_modules").to_string_lossy().into_owned()]
    } else {
        Vec::new()
    };
    LinkBinsOptions {
        extra_node_paths,
        prefer_symlinked_executables: config.prefer_symlinked_executables.unwrap_or(false),
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
    let deps: Vec<(&str, Option<&Path>)> =
        dep_names.iter().map(|name| (name.as_str(), None)).collect();
    link_named_dep_bins(modules_dir, &deps, link_options)
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
    let deps: Vec<(&str, Option<&Path>)> =
        deps.iter().map(|(name, target)| (name.as_str(), Some(target.as_path()))).collect();
    link_named_dep_bins(modules_dir, &deps, link_options)
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
}
impl<'a> PrefetchedBinLookup<'a> {
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
            match prefetched_bin_source(modules_dir, name, target, snapshot_key.as_ref(), lookup) {
                PrefetchedBin::NoBins => None,
                PrefetchedBin::Source(source) => Some(Ok(source)),
                PrefetchedBin::ReadFromDisk => read_dep_bin_source(modules_dir, name, target),
            }
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
/// The disk-read arm of [`link_direct_dep_bins_prefetched`], with the
/// same `NotFound`-tolerant / other-IO-fatal policy as
/// [`link_direct_dep_bins`].
pub(super) fn read_dep_bin_source(
    modules_dir: &Path,
    name: &str,
    target: &Path,
) -> Option<Result<PackageBinSource, LinkBinsError>> {
    let location = modules_dir.join(name);
    let manifest_path = location.join("package.json");
    let bytes = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            return Some(Err(LinkBinsError::ReadManifest { path: manifest_path, error }));
        }
    };
    let manifest: serde_json::Value = match parse_manifest_bytes(&bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            return Some(Err(LinkBinsError::ParseManifest { path: manifest_path, error }));
        }
    };
    Some(Ok(PackageBinSource::new(location, Arc::new(manifest))
        .with_resolved_location(target.to_path_buf())))
}
pub(super) fn link_named_dep_bins(
    modules_dir: &Path,
    deps: &[(&str, Option<&Path>)],
    link_options: &LinkBinsOptions,
) -> Result<(), LinkBinsError> {
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
            let location = modules_dir.join(name);
            let manifest_path = location.join("package.json");
            let bytes = match fs::read(&manifest_path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
                Err(error) => {
                    return Some(Err(LinkBinsError::ReadManifest { path: manifest_path, error }));
                }
            };
            let manifest: serde_json::Value = match parse_manifest_bytes(&bytes) {
                Ok(manifest) => manifest,
                Err(error) => {
                    return Some(Err(LinkBinsError::ParseManifest { path: manifest_path, error }));
                }
            };
            let mut source = PackageBinSource::new(location, Arc::new(manifest));
            if let Some(resolved) = resolved {
                source = source.with_resolved_location(resolved.to_path_buf());
            }
            Some(Ok(source))
        })
        .collect::<Result<_, _>>()?;
    if bin_sources.is_empty() {
        return Ok(());
    }
    link_bins_of_packages::<Host>(&bin_sources, &modules_dir.join(".bin"), link_options)
}
/// Link bins from resolved direct-dependency locations without requiring
/// importer symlinks. This is the `symlink: false` counterpart of
/// [`link_direct_dep_bins`]: `PnP` still exposes dependency executables in
/// `<modules_dir>/.bin` even though `<modules_dir>/<name>` is absent.
pub fn link_direct_dep_bins_from_locations(
    modules_dir: &Path,
    locations: &[PathBuf],
    link_options: &LinkBinsOptions,
) -> Result<(), LinkBinsError> {
    let bin_sources = locations
        .par_iter()
        .filter_map(|location| match read_package::<Host>(location) {
            // The locations are already the symlink-resolved package
            // dirs, so they double as `resolved_location`.
            Ok(Some(source)) => Some(Ok(source.with_resolved_location(location.clone()))),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if bin_sources.is_empty() {
        return Ok(());
    }
    link_bins_of_packages::<Host>(&bin_sources, &modules_dir.join(".bin"), link_options)
}
/// Top-level bin link that mixes direct-dep candidates and hoisted
/// (`publicly_hoisted_aliases_with_bins`) candidates in a single
/// [`link_bins_of_packages`] call so `pnpm_cmd_shim::pick_winner` (private)
/// can apply [`BinOrigin::Direct`] precedence over
/// [`BinOrigin::Hoisted`] — a hoisted (transitive) dep's bin must
/// never shadow a direct dep's bin with the same name.
///
/// Two-list shape (rather than a single tagged list) keeps the call
/// site cheap: callers already have these names in separate
/// collections — direct deps come from the importer's
/// `dependencies` / `devDependencies` / `optionalDependencies`,
/// hoisted aliases come from the hoist-result's
/// `publicly_hoisted_aliases_with_bins`. Joining them upthread
/// would force every caller to allocate a tagged `Vec`.
///
/// Lifecycle-script-created bins must pick up the post-install
/// state of `package.json` (a `postinstall` script can write a
/// binary that didn't exist at extract time and pacquet must shim
/// it). The caller schedules this pass *after* `BuildModules` runs
/// so the manifests-on-disk reflect the post-script state.
pub fn link_top_level_bins(
    modules_dir: &Path,
    direct_dep_names: &[String],
    hoisted_dep_names: &[String],
    link_options: &LinkBinsOptions,
) -> Result<(), LinkBinsError> {
    let mut bin_sources: Vec<PackageBinSource> = Vec::new();
    // Tag direct deps as `Direct` and hoisted as `Hoisted` so the
    // single downstream `pick_winner` call resolves conflicts via
    // the new [`BinOrigin`] tier.
    for source in read_bin_sources(modules_dir, direct_dep_names)? {
        bin_sources.push(source.with_origin(BinOrigin::Direct));
    }
    // Skip hoisted aliases that already appear under a direct
    // name. Reading the same `package.json` twice wouldn't change
    // the outcome — `pick_winner` would pick the Direct copy
    // anyway — but the work is wasted, so de-duplicate here by
    // filtering out hoisted candidates whose name already appears
    // in the direct set.
    let direct_set: HashSet<&str> = direct_dep_names.iter().map(String::as_str).collect();
    let hoisted_only: Vec<String> = hoisted_dep_names
        .iter()
        .filter(|name| !direct_set.contains(name.as_str()))
        .cloned()
        .collect();
    for source in read_bin_sources(modules_dir, &hoisted_only)? {
        bin_sources.push(source.with_origin(BinOrigin::Hoisted));
    }
    if bin_sources.is_empty() {
        return Ok(());
    }
    link_bins_of_packages::<Host>(&bin_sources, &modules_dir.join(".bin"), link_options)
}
/// Link a project's top-level bins while preserving direct-dependency
/// precedence over every other package present in its `node_modules`.
pub fn link_project_bins(
    modules_dir: &Path,
    direct_dep_names: &[String],
    link_options: &LinkBinsOptions,
) -> Result<(), LinkBinsError> {
    let direct_locations =
        direct_dep_names.iter().map(|name| modules_dir.join(name)).collect::<HashSet<_>>();
    let sources = collect_packages_in_modules_dir::<Host>(modules_dir)?
        .into_iter()
        .map(|source| {
            let origin = if direct_locations.contains(&source.location) {
                BinOrigin::Direct
            } else {
                BinOrigin::Hoisted
            };
            source.with_origin(origin)
        })
        .collect::<Vec<_>>();
    if sources.is_empty() {
        return Ok(());
    }
    link_bins_of_packages::<Host>(&sources, &modules_dir.join(".bin"), link_options)
}
/// Read each `<modules_dir>/<name>/package.json` and assemble the
/// list of [`PackageBinSource`]s. Same `NotFound`-tolerant /
/// other-IO-fatal policy as [`link_direct_dep_bins`]; factored out
/// so [`link_top_level_bins`] can reuse the read pass for both
/// direct and hoisted candidate lists.
pub(super) fn read_bin_sources(
    modules_dir: &Path,
    dep_names: &[String],
) -> Result<Vec<PackageBinSource>, LinkBinsError> {
    let locations: Vec<PathBuf> = dep_names.iter().map(|name| modules_dir.join(name)).collect();
    locations
        .par_iter()
        .filter_map(|location| {
            let manifest_path = location.join("package.json");
            let bytes = match fs::read(&manifest_path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
                Err(error) => {
                    return Some(Err(LinkBinsError::ReadManifest { path: manifest_path, error }));
                }
            };
            let manifest: serde_json::Value = match parse_manifest_bytes(&bytes) {
                Ok(manifest) => manifest,
                Err(error) => {
                    return Some(Err(LinkBinsError::ParseManifest { path: manifest_path, error }));
                }
            };
            Some(Ok(PackageBinSource::new(location.clone(), Arc::new(manifest))))
        })
        .collect()
}
