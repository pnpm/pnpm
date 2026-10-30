use super::{CreateVirtualStore, CreateVirtualStoreError, CreateVirtualStoreOutput};
use pnpm_lockfile::PackageKey;
use std::{collections::HashSet, fs, sync::Arc};

pub(super) fn apply(
    installer: &mut CreateVirtualStore<'_>,
    context: &crate::InstallContext<'_>,
    built: &HashSet<PackageKey>,
    captured: crate::CapturedBuildSideEffects,
    fetched: &mut CreateVirtualStoreOutput,
) -> Result<(), CreateVirtualStoreError> {
    let mut captured = captured.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
    for key in built {
        let directory =
            crate::build_modules::slots::virtual_store_dir_for_key(context.linker.layout, key);
        if !directory.is_dir() {
            fetched.fetch_failed.insert(key.clone());
            continue;
        }
        let manifest = read_manifest(&directory)
            .map_err(|source| CreateVirtualStoreError::LoadedBuildIo {
                path: directory.join("package.json"),
                source,
            })?;
        fetched.package_manifests.insert(key.clone(), Arc::new(manifest));
        fetched.package_bins.remove(key);
        fetched.package_bins.remove(&key.without_peer());
        let Some(result) = captured.remove(key) else { continue };
        let cache_key = result.cache_key.clone();
        insert(context, key, result, fetched);
        update_prefetch(installer, key, &cache_key, fetched);
    }
    Ok(())
}

fn insert(
    context: &crate::InstallContext<'_>,
    key: &PackageKey,
    result: crate::BuiltSideEffects,
    fetched: &mut CreateVirtualStoreOutput,
) {
    let crate::BuiltSideEffects { cache_key, diff } = result;
    let overlays = Arc::make_mut(fetched.side_effects.maps.entry(key.clone()).or_default());
    overlays.remove(&cache_key);
    let Some(diff) = diff else { return };
    let base = &fetched.cas_paths_by_pkg_id.as_ref().expect("fetched packages")
        [&super::super::cas_paths_key(key)]
        .cas_paths;
    // See `partition::Rebuilds::restores_empty_builds`.
    let overlay = if diff.is_empty() {
        Some((**base).clone().into())
    } else {
        pnpm_store_dir::side_effects_overlay(&context.config.store_dir, &cache_key, &diff, base)
    };
    if let Some(overlay) = overlay {
        overlays.insert(cache_key.clone(), overlay);
        Arc::make_mut(fetched.side_effects.diffs.entry(key.clone()).or_default())
            .insert(cache_key, diff);
    }
}

/// Records `key`'s result for `cache_key` in its retained prefetch row,
/// which its peer variants share, so the later passes restore it too.
fn update_prefetch(
    installer: &mut CreateVirtualStore<'_>,
    key: &PackageKey,
    cache_key: &str,
    fetched: &CreateVirtualStoreOutput,
) {
    let Some(metadata) =
        installer.entries.packages.and_then(|packages| packages.get(&key.without_peer()))
    else {
        return;
    };
    let Some(row_key) = crate::store_index_key_for_resolution(
        &metadata.resolution,
        &key.without_peer().pkg_id(),
        true,
    ) else {
        return;
    };
    let Some(prefetch) = installer.fetching.cas_prefetch.as_mut() else { return };
    let crate::create_virtual_store::PrefetchTask::Settled(rows) = &mut prefetch.task else {
        return;
    };
    let overlays = Arc::make_mut(rows.side_effects_maps.entry(row_key.clone()).or_default());
    match fetched.side_effects.maps
        .get(key)
        .and_then(|overlays| overlays.get(cache_key))
    {
        Some(overlay) => overlays.insert(cache_key.to_owned(), overlay.clone()),
        None => overlays.remove(cache_key),
    };
    let diffs = Arc::make_mut(rows.side_effects.entry(row_key).or_default());
    match fetched.side_effects.diffs
        .get(key)
        .and_then(|diffs| diffs.get(cache_key))
    {
        Some(diff) => diffs.insert(cache_key.to_owned(), diff.clone()),
        None => diffs.remove(cache_key),
    };
}

fn read_manifest(directory: &std::path::Path) -> std::io::Result<serde_json::Value> {
    match fs::read(directory.join("package.json")) {
        Ok(bytes) => {
            pnpm_package_manifest::parse_manifest_bytes(&bytes).map_err(std::io::Error::other)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(error) => Err(error),
    }
}
