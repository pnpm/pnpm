use pnpm_store_dir::SideEffectsOverlay;
use pnpm_tarball::PrefetchResult;
use std::{collections::HashMap, sync::Arc};

/// The overlays of the row at `cache_key`, with an overlay of the
/// package's own files for each empty row when
/// [`super::Rebuilds::restores_empty_builds`] is set.
pub(super) fn restorable_overlays(
    cache_key: &str,
    prefetch: &PrefetchResult,
    rebuilds: &super::Rebuilds<'_>,
) -> Option<Arc<HashMap<String, SideEffectsOverlay>>> {
    let maps = prefetch.side_effects_maps.get(cache_key);
    let empty_rows = prefetch.side_effects
        .get(cache_key)
        .filter(|_| rebuilds.restores_empty_builds)
        .into_iter()
        .flat_map(|diffs| diffs.iter())
        .filter(|(side_effects_key, diff)| {
            diff.is_empty() && !maps.is_some_and(|maps| maps.contains_key(*side_effects_key))
        })
        .map(|(side_effects_key, _)| side_effects_key)
        .collect::<Vec<_>>();
    let Some(files) = prefetch.cas_paths
        .get(cache_key)
        .filter(|_| !empty_rows.is_empty())
    else {
        return maps.map(Arc::clone);
    };
    let mut overlays = maps.map(|maps| (**maps).clone()).unwrap_or_default();
    for side_effects_key in empty_rows {
        overlays.insert(side_effects_key.clone(), (**files).clone().into());
    }
    Some(Arc::new(overlays))
}

#[cfg(test)]
mod tests;
