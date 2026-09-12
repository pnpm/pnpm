use super::CreateVirtualStoreError;
use crate::{
    install_package_by_snapshot::{runtime_platform_selector, unverified_fetch_is_allowed},
    store_index_key_for_resolution,
};
use pnpm_config::Config;
use pnpm_lockfile::{
    LockfileEntries, LockfileResolution, PackageKey, PackageMetadata, PlatformSelector,
    SnapshotEntry, select_platform_variant,
};
use pnpm_store_dir::store_index_key;
use std::collections::HashMap;

pub(super) fn derive_cache_keys(
    config: &Config,
    entries: LockfileEntries<'_>,
    supported_architectures: Option<&pnpm_package_is_installable::SupportedArchitectures>,
) -> HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>> {
    let (Some(snapshots), Some(packages)) = (entries.snapshots, entries.packages) else {
        return HashMap::new();
    };
    let selector = runtime_platform_selector(supported_architectures);
    snapshots
        .keys()
        .map(|snapshot_key| {
            let cache_key =
                snapshot_cache_key(snapshot_key, packages, config.ignore_scripts, &selector);
            (snapshot_key.clone(), cache_key)
        })
        .collect()
}
/// Sorted + deduplicated so `prefetch_cas_paths` doesn't redo identical
/// SELECT + integrity-check work for peer variants of one package.
/// Derived leniently over the whole lockfile — the superset over what
/// the plan pass will keep merely prefetches a few rows nothing reads.
pub(super) fn prefetch_keys(
    cache_keys: &HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>>,
) -> Vec<String> {
    let mut refs: Vec<&str> = cache_keys
        .values()
        .filter_map(|cache_key| cache_key.as_ref().ok())
        .filter_map(|cache_key| cache_key.value.as_deref())
        .collect();
    refs.sort_unstable();
    refs.dedup();
    refs.into_iter().map(String::from).collect()
}
/// Build the store-index cache key for a snapshot.
///
/// Returns `Err` for missing metadata — a condition the install would
/// fail on anyway — so the orchestrator can short-circuit *before* the
/// warm rayon batch runs; otherwise a malformed lockfile does up to
/// ~6 s of warm-batch linking before the actual error fires.
///
/// Shared by the upfront prefetch-keys loop and the warm/cold
/// partition in [`CreateVirtualStore::run`](crate::CreateVirtualStore::run), so a future change to
/// the resolution-type handling or key shape stays in one place.
/// A drift between the two loops would silently misclassify warm
/// entries as cold and quietly halve install speed.
pub(super) fn snapshot_cache_key(
    snapshot_key: &PackageKey,
    packages: &HashMap<PackageKey, PackageMetadata>,
    ignore_scripts: bool,
    runtime_platform_selector: &PlatformSelector,
) -> Result<SnapshotCacheKey, CreateVirtualStoreError> {
    let metadata_key = snapshot_key.without_peer();
    let metadata = packages.get(&metadata_key).ok_or_else(|| {
        CreateVirtualStoreError::MissingPackageMetadata {
            snapshot_key: snapshot_key.to_string(),
            metadata_key: metadata_key.to_string(),
        }
    })?;
    let pkg_id = metadata_key.pkg_id();
    match &metadata.resolution {
        LockfileResolution::Tarball(t) => {
            tarball_cache_key(t, &metadata.resolution, &pkg_id, ignore_scripts)
        }
        LockfileResolution::Registry(r) => Ok(SnapshotCacheKey {
            value: Some(store_index_key(&r.integrity.to_string(), &pkg_id)),
            is_git_hosted: false,
        }),
        LockfileResolution::Directory(_) => {
            // Directory resolutions are injected workspace deps and
            // bypass the CAFS entirely (the directory-fetcher returns
            // source-path entries; no `write_cas_file` happens, no
            // `PackageFilesIndex` row is written). There is therefore
            // no warm-cache key to recover the install from — every
            // install re-walks the source dir (the source may have
            // changed since the last install). Returning `Ok(None)`
            // routes the snapshot
            // through the cold path which runs the fetcher.
            Ok(SnapshotCacheKey { value: None, is_git_hosted: false })
        }
        LockfileResolution::Git(_) => {
            // `Git` resolutions land in CAS via
            // `pnpm_git_fetcher::GitFetcher`, which writes the
            // row under the same `gitHostedStoreIndexKey` shape as
            // the git-hosted tarball path. Returning the key here
            // lets the warm prefetch reuse a previous install's
            // clone + checkout + prepare + packlist work — without
            // this, every git install cold-paths regardless of
            // whether the snapshot is already in `index.db`. `built`
            // tracks `!ignore_scripts` to match the dispatcher's
            // write key.
            Ok(SnapshotCacheKey {
                value: store_index_key_for_resolution(
                    &metadata.resolution,
                    &pkg_id,
                    !ignore_scripts,
                ),
                is_git_hosted: true,
            })
        }
        // Runtime artifacts (Node.js / Bun / Deno): the per-archive
        // integrity is the warm-cache key, same shape as the
        // registry / tarball arms above. Mirrors the per-snapshot
        // dispatch in [`InstallPackageBySnapshot::run`]; the cold
        // path's variant selector + binary fetcher writes the row
        // under this key when it succeeds, so a re-install hits
        // here instead of cold-fetching the runtime archive again.
        LockfileResolution::Binary(binary) => Ok(SnapshotCacheKey {
            value: Some(store_index_key(&binary.integrity.to_string(), &pkg_id)),
            is_git_hosted: false,
        }),
        // `Variations` is a meta-shape: its integrity lives on the
        // *picked* variant, not the wrapper. Run the same host-
        // matching selector the cold path runs so the warm key
        // resolves to the variant that would actually be installed.
        // No variant matched → return `Ok(None)` and let the cold
        // path surface the typed `NoMatchingPlatformVariant` error
        // (a warm-key miss is the right shape; the warm prefetch
        // is best-effort and the cold path is where errors are
        // raised).
        LockfileResolution::Variations(variations) => {
            variant_cache_key(variations, runtime_platform_selector, &pkg_id)
        }
        // Custom resolutions have no built-in warm-cache key — the
        // cold path consults the pnpmfile custom fetchers, and the
        // delegated resolution (unknowable here) determines the row
        // that gets written.
        LockfileResolution::Custom(_) => Ok(SnapshotCacheKey { value: None, is_git_hosted: false }),
    }
}
pub(super) struct SnapshotCacheKey {
    pub(super) value: Option<String>,
    pub(super) is_git_hosted: bool,
}
/// Two snapshots agree on dependency wiring when both their
/// `dependencies` and `optionalDependencies` maps are equal (an
/// absent map and an empty map count as equal).
pub(super) fn snapshot_deps_equal(current: &SnapshotEntry, wanted: &SnapshotEntry) -> bool {
    fn maps_equal<Key, Value>(
        lhs: Option<&HashMap<Key, Value>>,
        rhs: Option<&HashMap<Key, Value>>,
    ) -> bool
    where
        Key: std::cmp::Eq + std::hash::Hash,
        Value: PartialEq,
    {
        match (lhs, rhs) {
            (None, None) => true,
            (Some(map), None) | (None, Some(map)) => map.is_empty(),
            (Some(x), Some(y)) => x == y,
        }
    }
    maps_equal(current.dependencies.as_ref(), wanted.dependencies.as_ref())
        && maps_equal(current.optional_dependencies.as_ref(), wanted.optional_dependencies.as_ref())
}
/// Compare the `integrity` field on two `packages:` entries.
pub(super) fn integrity_equal(
    current: Option<&PackageMetadata>,
    wanted: Option<&PackageMetadata>,
) -> bool {
    let current_integrity = current.and_then(|meta| meta.resolution.integrity());
    let wanted_integrity = wanted.and_then(|meta| meta.resolution.integrity());
    current_integrity == wanted_integrity
}
/// Whether a slot may be served by the macOS directory-clone cache
/// ([`crate::DirCloneCache`]).
///
/// The canonical slot is trusted by its completion marker alone, so
/// everything that identifies the slot's contents must be inside its
/// graph-hash path. That rules out:
///
/// - slots that need a build or patch marker — the canonical copy must
///   stay plain pre-build CAS content;
/// - mutable local sources, which reuse one slot for changing contents;
/// - forced re-imports, whose existing slot is known stale;
/// - any resolution without a checkable integrity. A git dependency
///   hashes to the same slot whether or not its fetch-time `prepare`
///   ran (`--ignore-scripts` versus a build-allowed install), so a
///   cached copy could serve the wrong variant.
pub(crate) fn dir_clone_cacheable(
    packages: &HashMap<PackageKey, PackageMetadata>,
    snapshot_key: &PackageKey,
    needs_build: bool,
    source_is_mutable: bool,
    force_import: bool,
) -> bool {
    !needs_build
        && !source_is_mutable
        && !force_import
        && packages
            .get(&snapshot_key.without_peer())
            .and_then(|metadata| metadata.resolution.checkable_integrity())
            .is_some()
}
pub(crate) fn package_content_changed(
    current_packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    wanted_packages: &HashMap<PackageKey, PackageMetadata>,
    snapshot_key: &PackageKey,
) -> bool {
    let current = current_packages.and_then(|packages| packages.get(&snapshot_key.without_peer()));
    let wanted = wanted_packages.get(&snapshot_key.without_peer());
    current.is_some() && !integrity_equal(current, wanted)
}
pub(super) fn variant_cache_key(
    variations: &pnpm_lockfile::VariationsResolution,
    runtime_platform_selector: &PlatformSelector,
    pkg_id: &str,
) -> Result<SnapshotCacheKey, CreateVirtualStoreError> {
    let Some(variant) = select_platform_variant(&variations.variants, runtime_platform_selector)
    else {
        return Ok(SnapshotCacheKey { value: None, is_git_hosted: false });
    };
    match &variant.resolution {
        LockfileResolution::Binary(binary) => Ok(SnapshotCacheKey {
            value: Some(store_index_key(&binary.integrity.to_string(), pkg_id)),
            is_git_hosted: false,
        }),
        // Non-`Binary` variant (corrupt lockfile, or a
        // future shape pacquet doesn't recognise). The
        // cold path raises the typed
        // `VariantHasNonBinaryResolution` error; we just
        // skip the warm key.
        _ => Ok(SnapshotCacheKey { value: None, is_git_hosted: false }),
    }
}
/// Rejects warm reuse when the downloader would refuse missing integrity.
/// The key must match the fetcher's git-hosted and script-policy variants.
pub(super) fn tarball_cache_key(
    tarball: &pnpm_lockfile::TarballResolution,
    resolution: &LockfileResolution,
    pkg_id: &str,
    ignore_scripts: bool,
) -> Result<SnapshotCacheKey, CreateVirtualStoreError> {
    if tarball.integrity.is_none() && !unverified_fetch_is_allowed(&tarball.tarball) {
        return Ok(SnapshotCacheKey { value: None, is_git_hosted: false });
    }
    Ok(SnapshotCacheKey {
        value: store_index_key_for_resolution(resolution, pkg_id, !ignore_scripts),
        is_git_hosted: tarball.is_git_hosted(),
    })
}
