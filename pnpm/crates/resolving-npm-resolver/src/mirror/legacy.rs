//! Mirrors an older pnpm wrote, which this version no longer reads.

use std::path::{Path, PathBuf};

use super::{encode_pkg_name, get_legacy_registry_name, get_registry_name};

/// Mirror directories of the NDJSON-era cache layout. The indexed layout
/// moved every mirror to a new version directory, so nothing reads these.
pub const LEGACY_META_DIRS: [&str; 3] =
    ["v11/metadata", "v11/metadata-full", "v11/metadata-full-filtered"];

/// The descriptor-scoped mirror root of the NDJSON-era cache layout.
pub const LEGACY_PRIVATE_META_ROOT: &str = "v11/metadata-private";

/// Paths where an older pnpm may have mirrored `pkg_name` for the current
/// `meta_dir`: the previous version directory, under the current registry
/// key and under the host-only key that predates it. Empty when `meta_dir`
/// has no legacy counterpart.
#[must_use]
pub fn get_legacy_pkg_mirror_paths(
    cache_dir: &Path,
    meta_dir: &str,
    registry: &str,
    pkg_name: &str,
) -> Vec<PathBuf> {
    let Some(legacy_meta_dir) = LEGACY_META_DIRS
        .iter()
        .find(|legacy| legacy_meta_dir_suffix(legacy) == legacy_meta_dir_suffix(meta_dir))
    else {
        return Vec::new();
    };
    let file_name = format!("{}.jsonl", encode_pkg_name(pkg_name));
    [get_registry_name(registry).ok(), get_legacy_registry_name(registry)]
        .into_iter()
        .flatten()
        .map(|registry_name| {
            cache_dir
                .join(legacy_meta_dir)
                .join(registry_name)
                .join(&file_name)
        })
        .collect()
}

/// The first of [`get_legacy_pkg_mirror_paths`] that exists on disk.
pub async fn find_legacy_pkg_mirror(
    cache_dir: &Path,
    meta_dir: &str,
    registry: &str,
    pkg_name: &str,
) -> Option<PathBuf> {
    for path in get_legacy_pkg_mirror_paths(cache_dir, meta_dir, registry, pkg_name) {
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Some(path);
        }
    }
    None
}

fn legacy_meta_dir_suffix(meta_dir: &str) -> &str {
    meta_dir.split_once('/').map_or(meta_dir, |(_, suffix)| suffix)
}
