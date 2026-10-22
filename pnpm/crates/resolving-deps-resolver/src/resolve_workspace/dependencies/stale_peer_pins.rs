use super::super::{importers::SortedImporters, options::WorkspaceImporter};
use crate::{
    resolve_dependency_tree::{WorkspaceTreeCtx, importer_direct_wanted_specs},
    resolve_importer::ResolveImporterOptions,
};
use node_semver::{Range, Version};
use pnpm_lockfile::{Lockfile, PkgName};
use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_resolver_base::{
    EXISTING_VERSION_SELECTOR_WEIGHT, VersionSelectorEntry, VersionSelectorType,
    VersionSelectorWithWeight, VersionSelectors, pinned_by_lockfile,
};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::Arc;

/// Re-resolve the auto-installed peers [`find_stale_peer_pins`] finds
/// instead of reusing their lockfile pins. Must run before the importers
/// share `workspace`.
pub(super) fn release_stale_peer_pins(
    workspace: &mut Arc<WorkspaceTreeCtx>,
    sorted: &mut SortedImporters<'_, '_>,
) {
    let Some(lockfile) = workspace.wanted_lockfile().cloned() else {
        return;
    };
    if !sorted.importers.iter().any(|importer| declares_peers(importer)) {
        return;
    }
    let direct_specs = collect_direct_specs(&sorted.importers, &sorted.opts);
    let stale_peer_pins = find_stale_peer_pins(&sorted.importers, &direct_specs, &lockfile);
    for (importer, opts) in sorted.importers.iter().zip(sorted.opts.iter_mut()) {
        if let Some(stale) = stale_peer_pins.get(&importer.id) {
            unpin_stale_peers(opts, stale, &direct_specs.versions);
        }
    }
    let stale_aliases = stale_peer_pins
        .into_iter()
        .map(|(importer_id, stale)| (importer_id, stale.into_keys().collect()))
        .collect();
    Arc::get_mut(workspace)
        .expect("the workspace ctx is not shared before the importers initialize")
        .set_stale_peer_pins(stale_aliases);
}

/// Per importer and by alias, the locked versions of the auto-installed
/// peers (peer dependencies it does not also declare as a dependency)
/// that a workspace project's specifier for that package rejects,
/// although the specifier overlaps the peer range. Such a peer has to be
/// re-resolved, or it stays on its first resolution while the projects
/// that provide it move on (pnpm/pnpm#11800, pnpm/pnpm#17058).
fn find_stale_peer_pins(
    importers: &[&WorkspaceImporter<'_>],
    direct_specs: &DirectSpecs,
    lockfile: &Lockfile,
) -> HashMap<String, HashMap<String, Version>> {
    let mut stale = HashMap::<String, HashMap<String, Version>>::default();
    for importer in importers.iter().filter(|importer| declares_peers(importer)) {
        let peers = importer_stale_peer_pins(importer, lockfile, &direct_specs.ranges);
        if !peers.is_empty() {
            stale.insert(importer.id.clone(), peers);
        }
    }
    stale
}

/// The specifiers the importers declare for each package in their
/// `dependencies`, `devDependencies` and `optionalDependencies`, with
/// `catalog:` specifiers replaced by the catalog entry.
#[derive(Default)]
struct DirectSpecs {
    /// The distinct ranges.
    ranges: HashMap<String, HashSet<Range>>,
    /// The exact versions among them.
    versions: HashMap<String, HashSet<String>>,
}

fn collect_direct_specs(
    importers: &[&WorkspaceImporter<'_>],
    opts: &[ResolveImporterOptions],
) -> DirectSpecs {
    let mut direct_specs = DirectSpecs::default();
    for (importer, importer_opts) in importers.iter().zip(opts) {
        let Ok(wanted) = importer_direct_wanted_specs(
            importer.manifest,
            [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
            false,
            &importer_opts.resolution.catalogs,
            importer_opts.resolution.catalogs_dir.as_deref(),
        ) else {
            continue;
        };
        for spec in wanted {
            if let Ok(version) = exact_version(&spec.bare_specifier) {
                direct_specs.versions
                    .entry(spec.alias.clone())
                    .or_default()
                    .insert(version.to_string());
            }
            if let Ok(range) = spec.bare_specifier.parse::<Range>() {
                direct_specs.ranges
                    .entry(spec.alias)
                    .or_default()
                    .insert(range);
            }
        }
    }
    direct_specs
}

/// The version an exact specifier such as `1.0.0`, `v1.0.0` or `=1.0.0`
/// names, spelled the way the lockfile weights it.
fn exact_version(spec: &str) -> Result<Version, node_semver::SemverError> {
    spec.trim()
        .trim_start_matches('=')
        .trim_start()
        .parse()
}

fn declares_peers(importer: &WorkspaceImporter<'_>) -> bool {
    importer.manifest
        .dependencies([DependencyGroup::Peer])
        .next()
        .is_some()
}

fn importer_stale_peer_pins(
    importer: &WorkspaceImporter<'_>,
    lockfile: &Lockfile,
    direct_ranges: &HashMap<String, HashSet<Range>>,
) -> HashMap<String, Version> {
    let manifest = importer.manifest;
    let declared = manifest
        .dependencies([DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional])
        .map(|(name, _)| name)
        .collect::<HashSet<_>>();
    manifest
        .dependencies([DependencyGroup::Peer])
        .filter(|(alias, _)| !declared.contains(alias))
        .filter_map(|(alias, peer_spec)| {
            let ranges = direct_ranges.get(alias)?;
            let peer_range = peer_spec.parse::<Range>().ok()?;
            let mut overlapping = ranges
                .iter()
                .filter(|range| range.allows_any(&peer_range))
                .peekable();
            overlapping.peek()?;
            let pinned = locked_importer_version(lockfile, &importer.id, alias)?;
            overlapping
                .any(|range| !range.satisfies(&pinned))
                .then(|| (alias.to_string(), pinned))
        })
        .collect()
}

fn locked_importer_version(lockfile: &Lockfile, importer_id: &str, alias: &str) -> Option<Version> {
    let name = alias.parse::<PkgName>().ok()?;
    let importer = lockfile.importers.get(importer_id)?;
    [&importer.dependencies, &importer.optional_dependencies, &importer.dev_dependencies]
        .into_iter()
        .find_map(|deps| deps.as_ref()?.get(&name))?
        .version
        .ver_peer()?
        .version_semver()
        .cloned()
}

/// Remove the lockfile's weight from the stale locked versions in the
/// importer's preferred versions, so the peers are picked the way a fresh
/// install picks them rather than back onto the stale versions.
///
/// The versions a project's specifier names exactly lose the lockfile's
/// weight too. Their specifier already weights them, and the lockfile's
/// weight would outrank a higher version that only a new specifier names.
fn unpin_stale_peers(
    opts: &mut ResolveImporterOptions,
    stale: &HashMap<String, Version>,
    exact_versions: &HashMap<String, HashSet<String>>,
) {
    let preferred_versions = Arc::make_mut(&mut opts.base_opts.version.preferred_versions);
    for (name, pinned) in stale {
        let Some(selectors) = preferred_versions.get_mut(name) else { continue };
        let pinned = pinned.to_string();
        let named = exact_versions
            .get(name)
            .into_iter()
            .flatten();
        for version in std::iter::once(&pinned).chain(named) {
            remove_lockfile_weight(selectors, version);
        }
    }
}

fn remove_lockfile_weight(selectors: &mut VersionSelectors, version: &str) {
    let Some(VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
        selector_type: VersionSelectorType::Version,
        weight,
    })) = selectors.get_mut(version)
    else {
        return;
    };
    if !pinned_by_lockfile(*weight) {
        return;
    }
    *weight -= EXISTING_VERSION_SELECTOR_WEIGHT;
    if *weight == 0 {
        selectors.remove(version);
    }
}

#[cfg(test)]
mod tests;
