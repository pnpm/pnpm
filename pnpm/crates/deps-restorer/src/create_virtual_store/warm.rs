use super::{
    CreateVirtualStoreError, SnapshotWithCacheKey,
    cache_keys::{dir_clone_cacheable, package_content_changed},
    cas_paths_key, partition, removed_aliases_for,
    slot_linking::{LinkSlotsParallel, SlotLink, emit_warm_snapshot_progress, link_slots_parallel},
};
use crate::{CasPathsByPkgId, InstallPackageBySnapshotError};
use pnpm_git_fetcher::{GitFetcherError, assert_package_build_allowed};
use pnpm_lockfile::{LockfileResolution, PackageKey, PackageMetadata, PkgName};
use pnpm_package_manifest::{
    files_include_install_scripts, manifest_requires_build, parse_manifest,
};
use pnpm_reporter::Reporter;
use pnpm_tarball::PrefetchResult;
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) fn enforce_cached_git_prepare_policy(
    snapshots: &mut [SnapshotWithCacheKey<'_>],
    packages: &HashMap<PackageKey, PackageMetadata>,
    prefetch: &PrefetchResult,
    allow_build_policy: &crate::AllowBuildPolicy,
    ignore_scripts: bool,
    has_git_hosted_survivor: bool,
) -> Result<(), CreateVirtualStoreError> {
    if ignore_scripts || !has_git_hosted_survivor {
        return Ok(());
    }
    for (snapshot_key, _snapshot, cache_key) in snapshots {
        if !cached_git_prepare_allowed(
            snapshot_key,
            cache_key.as_deref(),
            (packages, prefetch),
            allow_build_policy,
        )? {
            *cache_key = None;
        }
    }
    Ok(())
}
/// Whether the warm slot of one git-hosted snapshot may be reused. `false`
/// drops the cache key so the snapshot is fetched and prepared afresh.
pub(super) fn cached_git_prepare_allowed(
    snapshot_key: &PackageKey,
    cache_key: Option<&str>,
    lockfile: (&HashMap<PackageKey, PackageMetadata>, &PrefetchResult),
    allow_build_policy: &crate::AllowBuildPolicy,
) -> Result<bool, CreateVirtualStoreError> {
    let (packages, prefetch) = lockfile;
    let Some(key) = cache_key else { return Ok(true) };
    let Some(cas_paths) = prefetch.cas_paths.get(key) else { return Ok(true) };
    let metadata_key = snapshot_key.without_peer();
    let metadata = packages.get(&metadata_key).ok_or_else(|| {
        CreateVirtualStoreError::MissingPackageMetadata {
            snapshot_key: snapshot_key.to_string(),
            metadata_key: metadata_key.to_string(),
        }
    })?;
    if !is_git_hosted_resolution(&metadata.resolution)
        || prefetch.requires_prepare.get(key) == Some(&false)
    {
        return Ok(true);
    }
    let Some(manifest) = cached_git_manifest(prefetch, key, cas_paths) else {
        return Ok(false);
    };
    let package_id = metadata_key.pkg_id();
    let name = manifest.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
    if allow_build_policy.check(&format!("{name}@{package_id}")) == Some(true) {
        return Ok(true);
    }
    if !prefetch.requires_prepare.contains_key(key) {
        return Ok(false);
    }
    let allow_build = |dep_path: &str| allow_build_policy.check(dep_path).unwrap_or(false);
    assert_package_build_allowed(&allow_build, &package_id, &manifest).map_err(|error| {
        CreateVirtualStoreError::InstallPackageBySnapshot(InstallPackageBySnapshotError::GitFetch(
            GitFetcherError::Prepare(error),
        ))
    })?;
    Ok(true)
}
/// The prefetched manifest, or the one the warm slot's `package.json` holds.
/// `None` when neither can be read, which leaves the slot unusable.
pub(super) fn cached_git_manifest<'a>(
    prefetch: &'a PrefetchResult,
    key: &str,
    cas_paths: &HashMap<String, PathBuf>,
) -> Option<Cow<'a, serde_json::Value>> {
    if let Some(manifest) = prefetch.manifests.get(key) {
        return Some(Cow::Borrowed(manifest.as_ref()));
    }
    let package_json = cas_paths.get("package.json")?;
    let contents = fs::read_to_string(package_json).ok()?;
    parse_manifest(&contents).ok().map(Cow::Owned)
}
pub(super) fn is_git_hosted_resolution(resolution: &LockfileResolution) -> bool {
    match resolution {
        LockfileResolution::Git(_) => true,
        LockfileResolution::Tarball(tarball) => tarball.is_git_hosted(),
        _ => false,
    }
}
pub(super) fn requires_build_from_cas_paths(cas_paths: &HashMap<String, PathBuf>) -> bool {
    if files_include_install_scripts(cas_paths.keys()) {
        return true;
    }
    let Some(package_json) = cas_paths.get("package.json") else { return false };
    let Ok(contents) = fs::read_to_string(package_json) else { return false };
    let Ok(manifest) = parse_manifest(&contents) else {
        return false;
    };
    manifest_requires_build(&manifest)
}
pub(super) fn snapshot_needs_build_marker(snapshot_key: &PackageKey, requires_build: bool) -> bool {
    requires_build || crate::snapshot_has_patch(snapshot_key)
}
pub(super) fn gvs_slot_needs_rebuild(
    layout: &crate::VirtualStoreLayout,
    allow_build_policy: &crate::AllowBuildPolicy,
    snapshot_key: &PackageKey,
) -> bool {
    if !layout.enable_global_virtual_store() {
        return false;
    }
    let can_build = crate::snapshot_has_patch(snapshot_key)
        || allow_build_policy.check(&snapshot_key.without_peer().to_string()) == Some(true);
    can_build
        && layout
            .slot_dir(snapshot_key)
            .join("node_modules")
            .join(snapshot_key.name.to_string())
            .join(crate::NEEDS_BUILD_MARKER)
            .is_file()
}
/// The pristine CAS paths of every warm snapshot whose package may publish
/// shared side effects.
pub(super) fn warm_shared_base_cas_paths(
    shared_packages: Option<&HashSet<&str>>,
    warm: &[partition::WarmEntry<'_>],
) -> crate::shared_side_effects::BaseCasPaths {
    let mut base_cas_paths = crate::shared_side_effects::BaseCasPaths::new();
    let Some(shared_packages) = shared_packages else { return base_cas_paths };
    for (snapshot_key, _, cas_paths, _, _) in warm {
        if shared_packages.contains(snapshot_key.name.to_string().as_str()) {
            base_cas_paths.insert((*snapshot_key).clone(), (***cas_paths).clone());
        }
    }
    base_cas_paths
}
pub(super) fn warm_cas_paths_by_pkg_id(warm: &[partition::WarmEntry<'_>]) -> CasPathsByPkgId {
    let mut map = CasPathsByPkgId::with_capacity(warm.len());
    for (snapshot_key, _snapshot, cas_paths, _cache_key, _needs_build_marker) in warm {
        map.entry(cas_paths_key(snapshot_key)).or_insert_with(|| Arc::clone(cas_paths));
    }
    map
}
/// What the warm batch links, beyond the slots themselves.
pub(super) struct WarmLinkBatch<'a> {
    pub(super) packages: &'a HashMap<PackageKey, PackageMetadata>,
    pub(super) current_packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub(super) is_hoisted: bool,
    pub(super) needs_build_marker_source: Option<&'a Path>,
    pub(super) removed_aliases_by_key: &'a HashMap<PackageKey, Vec<PkgName>>,
    pub(super) template: &'a LinkSlotsParallel<'a>,
}
/// Link every warm slot. Hoisted skips the batch entirely: no virtual-store
/// slot gets written, so there's no per-snapshot link work to do — the CAS
/// paths captured by the caller are the only output the link phase consumes,
/// and all link work is routed into the hoisted linker instead. It still wants
/// the progress reporter to fire so `pnpm:progress imported`-style updates
/// render the warm hits.
pub(super) fn link_warm_batch<Reporter: self::Reporter>(
    warm: &[partition::WarmEntry<'_>],
    batch: &WarmLinkBatch<'_>,
) -> Result<(), CreateVirtualStoreError> {
    if batch.is_hoisted {
        emit_hoisted_warm_progress::<Reporter>(warm, batch);
        return Ok(());
    }
    let warm_slots: Vec<SlotLink<'_>> = warm
        .iter()
        .map(|(snapshot_key, snapshot, cas_paths, cache_key, needs_build_marker)| {
            let force_import =
                package_content_changed(batch.current_packages, batch.packages, snapshot_key);
            SlotLink {
                snapshot_key,
                snapshot,
                cas_paths: cas_paths.as_ref(),
                warm_cache_key: Some(cache_key),
                // A cache key means the file map is CAS-backed, and
                // `snapshot_cache_key` yields none for a directory resolution,
                // so a warm slot's source is immutable by construction.
                source_is_mutable: false,
                force_import,
                needs_build_marker_source: needs_build_marker
                    .then_some(batch.needs_build_marker_source)
                    .flatten(),
                dir_clone_cacheable: dir_clone_cacheable(
                    batch.packages,
                    snapshot_key,
                    *needs_build_marker,
                    false,
                    force_import,
                ),
                removed_aliases: removed_aliases_for(batch.removed_aliases_by_key, snapshot_key),
            }
        })
        .collect();
    link_slots_parallel::<Reporter>(LinkSlotsParallel {
        batch: "warm",
        slots: &warm_slots,
        ..*batch.template
    })
}
pub(super) fn emit_hoisted_warm_progress<Reporter: self::Reporter>(
    warm: &[partition::WarmEntry<'_>],
    batch: &WarmLinkBatch<'_>,
) {
    for (snapshot_key, _, _, cache_key, _) in warm {
        emit_warm_snapshot_progress::<Reporter>(
            &snapshot_key.pkg_id(),
            batch.template.requester,
            batch.template.progress_reported.contains(*cache_key),
        );
    }
}
