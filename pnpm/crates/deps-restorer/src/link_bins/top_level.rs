use super::{existing_commands, read_location_bin_sources};
use pnpm_cmd_shim::{
    BinOrigin, Host, LinkBinsError, LinkBinsOptions, PackageBinSource,
    collect_packages_in_modules_dir, link_bins_of_packages, link_bins_of_packages_with_excludes,
};
use pnpm_package_manifest::parse_manifest_bytes;
use rayon::prelude::*;
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

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
    let direct_set: HashSet<&str> = direct_dep_names
        .iter()
        .map(String::as_str)
        .collect();
    let hoisted_only: Vec<String> = hoisted_dep_names
        .iter()
        .filter(|name| !direct_set.contains(name.as_str()))
        .cloned()
        .collect();
    for source in read_bin_sources(modules_dir, &hoisted_only)? {
        bin_sources.push(source.with_origin(BinOrigin::Hoisted));
    }
    for source in read_location_bin_sources(peer_locations)? {
        bin_sources.push(source.with_origin(BinOrigin::Peer));
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
    let direct_locations = direct_dep_names
        .iter()
        .map(|name| modules_dir.join(name))
        .collect::<HashSet<_>>();
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

/// Link bins from resolved direct-dependency locations without requiring
/// importer symlinks. This is the `symlink: false` counterpart of
/// [`super::direct::link_direct_dep_bins`]: `PnP` still exposes dependency executables in
/// `<modules_dir>/.bin` even though `<modules_dir>/<name>` is absent.
pub fn link_direct_dep_bins_from_locations(
    modules_dir: &Path,
    locations: &[PathBuf],
    link_options: &LinkBinsOptions,
) -> Result<(), LinkBinsError> {
    let bin_sources = read_location_bin_sources(locations)?;
    if bin_sources.is_empty() {
        return Ok(());
    }
    link_bins_of_packages::<Host>(&bin_sources, &modules_dir.join(".bin"), link_options)
}

/// [`link_direct_dep_bins_from_locations`] that leaves every command
/// already in `<modules_dir>/.bin` in place, so the packages at
/// `locations` only add commands nothing else provides.
pub fn link_new_bins_from_locations(
    modules_dir: &Path,
    locations: &[PathBuf],
    link_options: &LinkBinsOptions,
) -> Result<(), LinkBinsError> {
    let bin_sources = read_location_bin_sources(locations)?;
    if bin_sources.is_empty() {
        return Ok(());
    }
    let bins_dir = modules_dir.join(".bin");
    let existing = existing_commands(&bins_dir)?;
    link_bins_of_packages_with_excludes::<Host>(&bin_sources, &bins_dir, &existing, link_options)
}

/// Read each `<modules_dir>/<name>/package.json` and assemble the
/// list of [`PackageBinSource`]s. Same `NotFound`-tolerant /
/// other-IO-fatal policy as [`super::direct::link_direct_dep_bins`]; factored out
/// so [`link_top_level_bins`] can reuse the read pass for both
/// direct and hoisted candidate lists.
pub(super) fn read_bin_sources(
    modules_dir: &Path,
    dep_names: &[String],
) -> Result<Vec<PackageBinSource>, LinkBinsError> {
    let locations: Vec<PathBuf> = dep_names
        .iter()
        .map(|name| modules_dir.join(name))
        .collect();
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
