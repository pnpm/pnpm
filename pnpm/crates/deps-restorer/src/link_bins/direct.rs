use super::{build_has_bin_set, pkg_dir_under};
use crate::PackageManifests;
use pnpm_cmd_shim::{LinkBinsError, PackageBinSource, ShimTargetCache};
use pnpm_lockfile::{PackageKey, PackageMetadata};
use pnpm_package_manifest::{find_parent_publish_manifest, parse_manifest_bytes};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Resolve the hoist pass's `(alias, snapshot key)` bin list into the
/// `(alias, slot package dir)` pairs [`crate::BinCandidates::Resolved`]
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
/// One direct dep of a [`crate::BinCandidates::Prefetched`] pass:
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
    /// Parsed manifests recovered from the store-index prefetch or
    /// cold-batch tarball extraction. A hit replaces the per-importer
    /// `package.json` read; a miss falls back to disk.
    package_manifests: Option<&'a PackageManifests>,
    /// Precalculated bin commands and shebang runtimes recovered from
    /// `dist.pnpr.bins` or cold-batch tarball extraction.
    package_bins: Option<&'a crate::PackageBins>,
    /// Per-snapshot `requiresBuild` flags from the same prefetch. The
    /// no-IO path is only sound for a snapshot known not to run build
    /// scripts: a persisted global-virtual-store slot that was built may
    /// carry a `package.json` its scripts rewrote, and only the disk
    /// read sees those bins. A missing entry counts as "may build".
    requires_build: Option<&'a crate::RequiresBuildBySnapshot>,
    /// Per-target shim probe memo, shared so each virtual-store bin
    /// file's shebang read and executable-bit fix-up run once per pass
    /// rather than once per importer.
    pub(super) shim_cache: ShimTargetCache,
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

    #[must_use]
    pub fn with_package_bins(mut self, package_bins: Option<&'a crate::PackageBins>) -> Self {
        self.package_bins = package_bins;
        self
    }

    pub(super) fn is_build_pending(&self, snapshot_key: Option<&PackageKey>) -> bool {
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
            package_bins: None,
            requires_build,
            shim_cache: ShimTargetCache::default(),
            scheduled_builds: None,
        }
    }
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
    if crate::snapshot_has_patch(snapshot_key)
        || lookup.requires_build.and_then(|flags| flags.get(snapshot_key)) != Some(&false)
    {
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
    let precalculated_bins = lookup.package_bins
        .and_then(|bins| bins.get(&metadata_key))
        .map(Arc::clone);
    PrefetchedBin::Source(
        PackageBinSource::new(modules_dir.join(name), Arc::clone(manifest))
            .with_resolved_location(target.to_path_buf())
            .with_precalculated_bins(precalculated_bins),
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

/// Reads `<modules_dir>/<name>/package.json`. A dependency linked to an
/// existing `publishConfig.directory` that has no manifest of its own
/// falls back to the manifest of the project that declares that
/// directory, found through `target`. A publish directory that does not
/// exist yet has no bins to link; a build creates it later.
pub(super) fn read_dep_manifest(
    modules_dir: &Path,
    name: &str,
    target: Option<&Path>,
) -> Option<Result<(PathBuf, serde_json::Value), LinkBinsError>> {
    let location = modules_dir.join(name);
    let manifest = match read_manifest_at(&location.join("package.json")) {
        Ok(Some(manifest)) => manifest,
        Ok(None) => match read_existing_publish_manifest(target?) {
            Ok(Some(manifest)) => manifest,
            Ok(None) => return None,
            Err(err) => return Some(Err(err)),
        },
        Err(err) => return Some(Err(err)),
    };
    Some(Ok((location, manifest)))
}
fn read_existing_publish_manifest(
    target: &Path,
) -> Result<Option<serde_json::Value>, LinkBinsError> {
    match fs::metadata(target) {
        Ok(metadata) if metadata.is_dir() => {
            find_parent_publish_manifest(target).map_err(LinkBinsError::ReadProjectManifest)
        }
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(LinkBinsError::ResolvePath { path: target.to_path_buf(), error }),
    }
}
