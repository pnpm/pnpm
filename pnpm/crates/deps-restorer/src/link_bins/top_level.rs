use super::read_location_bin_sources;
use pnpm_cmd_shim::{
    BinOrigin, Host, LinkBinsError, LinkBinsOptions, PackageBinSource, PreparedPackageBins,
    link_bins_of_packages, link_bins_of_packages_precomputed,
};
use pnpm_package_manifest::parse_manifest_bytes;
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(test)]
mod tests;

/// Top-level bin link that links direct-dep candidates, publicly hoisted
/// aliases, and auto-installed peer dependencies in a single
/// [`link_bins_of_packages`] pass so direct dependencies take precedence
/// over publicly hoisted packages, which take precedence over auto-installed peers.
///
/// Direct deps come from the importer's dependency groups, hoisted
/// aliases from the hoist result, and peers from their resolved slots.
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
    peer_locations: &[PathBuf],
    link_options: &LinkBinsOptions,
) -> Result<(), LinkBinsError> {
    link_top_level_bins_cached(
        modules_dir,
        direct_dep_names,
        hoisted_dep_names,
        peer_locations,
        link_options,
        None,
    )
}

pub(crate) fn link_top_level_bins_cached(
    modules_dir: &Path,
    direct_dep_names: &[String],
    hoisted_dep_names: &[String],
    peer_locations: &[PathBuf],
    link_options: &LinkBinsOptions,
    cached: Option<&HashMap<PathBuf, PreparedPackageBins>>,
) -> Result<(), LinkBinsError> {
    let mut bin_sources: Vec<PackageBinSource> = Vec::new();
    // Tag direct deps as `Direct` and hoisted as `Hoisted` so the
    // single downstream `pick_winner` call resolves conflicts via
    // the new [`BinOrigin`] tier.
    for source in read_bin_sources(modules_dir, direct_dep_names, cached)? {
        bin_sources.push(source.with_origin(BinOrigin::Direct));
    }
    // Skip hoisted aliases that already appear under a direct
    // name. Reading the same `package.json` twice wouldn't change
    // the outcome — `pick_winner` would pick the Direct copy
    // anyway — but the work is wasted, so de-duplicate here by
    // filtering out hoisted candidates whose name already appears
    // in the direct set.
    let direct_set: HashSet<&str> = direct_dep_names
        .iter()
        .map(String::as_str)
        .collect();
    let hoisted_only: Vec<String> = hoisted_dep_names
        .iter()
        .filter(|name| !direct_set.contains(name.as_str()))
        .cloned()
        .collect();
    for source in read_bin_sources(modules_dir, &hoisted_only, cached)? {
        bin_sources.push(source.with_origin(BinOrigin::Hoisted));
    }
    for source in read_location_bin_sources(peer_locations)? {
        bin_sources.push(source.with_origin(BinOrigin::Peer));
    }
    if bin_sources.is_empty() {
        return Ok(());
    }
    match cached {
        Some(cached) => link_bins_of_packages_precomputed::<Host>(
            &bin_sources,
            cached,
            &modules_dir.join(".bin"),
            link_options,
        ),
        None => {
            link_bins_of_packages::<Host>(&bin_sources, &modules_dir.join(".bin"), link_options)
        }
    }
}
/// Read each `<modules_dir>/<name>/package.json` and assemble the
/// list of [`PackageBinSource`]s. Same `NotFound`-tolerant /
/// other-IO-fatal policy as [`super::link_direct_dep_bins`]; factored out
/// so [`link_top_level_bins`] can reuse the read pass for both
/// direct and hoisted candidate lists.
fn read_bin_sources(
    modules_dir: &Path,
    dep_names: &[String],
    cached: Option<&HashMap<PathBuf, PreparedPackageBins>>,
) -> Result<Vec<PackageBinSource>, LinkBinsError> {
    let locations: Vec<PathBuf> = dep_names
        .iter()
        .map(|name| modules_dir.join(name))
        .collect();
    locations
        .par_iter()
        .filter_map(|location| {
            if let Some(source) = cached.and_then(|sources| sources.get(location)) {
                return Some(Ok(source.source().clone()));
            }
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
